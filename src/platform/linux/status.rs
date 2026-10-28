use super::resource_usage::{ResourceSampler, ResourceSnapshot};
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum NetworkStatus {
    #[default]
    Checking,
    Connected {
        interface: String,
        wireless: bool,
        signal: Option<u8>,
    },
    Offline,
    Unavailable,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatteryStatus {
    pub percent: u8,
    pub charging: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusSnapshot {
    pub network: NetworkStatus,
    pub battery: Option<BatteryStatus>,
    pub resources: ResourceSnapshot,
}

pub trait StatusReader: Send + Sync {
    fn read(&self) -> StatusSnapshot;
}
pub struct LinuxStatusReader {
    root: PathBuf,
    resources: std::sync::Mutex<ResourceSampler>,
}
impl Default for LinuxStatusReader {
    fn default() -> Self {
        Self::at(Path::new("/"))
    }
}
impl LinuxStatusReader {
    pub fn at(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
            resources: std::sync::Mutex::new(ResourceSampler::default()),
        }
    }
    fn network(&self) -> NetworkStatus {
        let Ok(entries) = std::fs::read_dir(self.root.join("sys/class/net")) else {
            return NetworkStatus::Unavailable;
        };
        let mut links = Vec::new();
        let mut unreadable = false;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == "lo" {
                continue;
            }
            let path = entry.path();
            let wireless = path.join("wireless").exists() || path.join("phy80211").exists();
            let state = read_text(&path.join("operstate"));
            let carrier = read_text(&path.join("carrier"));
            let physical = path.join("device").exists();
            if (wireless || physical)
                && (state.is_empty() || (state == "unknown" && carrier.is_empty()))
            {
                unreadable = true;
            }
            let active = state == "up" || (state == "unknown" && carrier == "1");
            if active && carrier != "0" {
                links.push((name, wireless, physical));
            }
        }
        links.sort_by(|a, b| a.0.cmp(&b.0));
        let routes = default_routes(
            &read_text(&self.root.join("proc/net/route")),
            &read_text(&self.root.join("proc/net/ipv6_route")),
        );
        let selected = routes
            .iter()
            .find_map(|(_, name)| links.iter().find(|link| &link.0 == name))
            .or_else(|| links.iter().find(|link| link.1))
            .or_else(|| links.iter().find(|link| link.2));
        match selected {
            Some((interface, wireless, _)) => NetworkStatus::Connected {
                interface: interface.clone(),
                wireless: *wireless,
                signal: if *wireless {
                    wireless_signal(&read_text(&self.root.join("proc/net/wireless")), interface)
                } else {
                    None
                },
            },
            None if unreadable => NetworkStatus::Unavailable,
            None => NetworkStatus::Offline,
        }
    }
    fn battery(&self) -> Option<BatteryStatus> {
        let entries = std::fs::read_dir(self.root.join("sys/class/power_supply")).ok()?;
        let mut levels = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if read_text(&path.join("type")) != "Battery"
                || read_text(&path.join("present")) == "0"
                || read_text(&path.join("scope")) == "Device"
            {
                continue;
            }
            let level = read_text(&path.join("capacity"))
                .parse::<u8>()
                .ok()
                .filter(|p| *p <= 100)
                .or_else(|| battery_fraction(&path, "energy_now", "energy_full"))
                .or_else(|| battery_fraction(&path, "charge_now", "charge_full"));
            if let Some(level) = level {
                levels.push((level, read_text(&path.join("status")) == "Charging"));
            }
        }
        if levels.is_empty() {
            None
        } else {
            Some(BatteryStatus {
                percent: (levels.iter().map(|(level, _)| *level as u64).sum::<u64>()
                    / levels.len() as u64) as u8,
                charging: levels.iter().any(|(_, charging)| *charging),
            })
        }
    }
}
impl StatusReader for LinuxStatusReader {
    fn read(&self) -> StatusSnapshot {
        let network = self.network();
        let interface = match &network {
            NetworkStatus::Connected { interface, .. } => Some(interface.as_str()),
            _ => None,
        };
        let resources = self
            .resources
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .read(&self.root, interface);
        StatusSnapshot {
            network,
            battery: self.battery(),
            resources,
        }
    }
}
fn read_text(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .to_owned()
}
fn battery_fraction(path: &Path, now: &str, full: &str) -> Option<u8> {
    let now = read_text(&path.join(now)).parse::<u64>().ok()?;
    let full = read_text(&path.join(full)).parse::<u64>().ok()?;
    if full == 0 || now > full {
        return None;
    }
    Some((u128::from(now) * 100 / u128::from(full)) as u8)
}
fn default_routes(ipv4: &str, ipv6: &str) -> Vec<(u64, String)> {
    let mut routes = Vec::new();
    for line in ipv4.lines().skip(1) {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() >= 8
            && fields[1] == "00000000"
            && fields[7] == "00000000"
            && u32::from_str_radix(fields[3], 16).is_ok_and(|flags| flags & 1 != 0)
        {
            if let Ok(metric) = fields[6].parse() {
                routes.push((metric, fields[0].to_owned()));
            }
        }
    }
    for line in ipv6.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() == 10
            && fields[0] == "00000000000000000000000000000000"
            && fields[1] == "00"
            && u32::from_str_radix(fields[8], 16)
                .is_ok_and(|flags| flags & 1 != 0 && flags & 0x200 == 0)
        {
            if let Ok(metric) = u64::from_str_radix(fields[5], 16) {
                routes.push((metric, fields[9].to_owned()));
            }
        }
    }
    routes.sort();
    routes
}
fn wireless_signal(contents: &str, interface: &str) -> Option<u8> {
    for line in contents.lines() {
        let Some((name, values)) = line.split_once(':') else {
            continue;
        };
        if name.trim() != interface {
            continue;
        }
        let values = values.split_whitespace().collect::<Vec<_>>();
        if let Some(level) = values
            .get(2)
            .and_then(|s| s.trim_end_matches('.').parse::<f64>().ok())
            .filter(|v| v.is_finite() && (-120.0..0.0).contains(v))
        {
            return Some(((level + 100.0) * 2.0).clamp(0.0, 100.0).round() as u8);
        }
        return values
            .get(1)
            .and_then(|s| s.trim_end_matches('.').parse::<f64>().ok())
            .filter(|v| v.is_finite() && (0.0..=70.0).contains(v))
            .map(|v| (v * 100.0 / 70.0).round() as u8);
    }
    None
}

pub struct StatusMonitor {
    reader: Arc<dyn StatusReader>,
    pending: Option<mpsc::Receiver<StatusSnapshot>>,
    next_refresh: Instant,
    pub snapshot: StatusSnapshot,
}
impl Default for StatusMonitor {
    fn default() -> Self {
        Self::new(Arc::new(LinuxStatusReader::default()))
    }
}
impl StatusMonitor {
    pub fn new(reader: Arc<dyn StatusReader>) -> Self {
        Self {
            reader,
            pending: None,
            next_refresh: Instant::now(),
            snapshot: StatusSnapshot::default(),
        }
    }
    pub fn poll(&mut self, now: Instant, enabled: bool) {
        if let Some(receiver) = &self.pending {
            match receiver.try_recv() {
                Ok(snapshot) => {
                    self.snapshot = snapshot;
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.snapshot = StatusSnapshot {
                        network: NetworkStatus::Unavailable,
                        battery: None,
                        ..Default::default()
                    };
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if enabled && self.pending.is_none() && now >= self.next_refresh {
            self.next_refresh = now + Duration::from_secs(3);
            let reader = self.reader.clone();
            let (tx, rx) = mpsc::sync_channel(1);
            if std::thread::Builder::new()
                .name("desktop-status".into())
                .spawn(move || {
                    let _ = tx.send(reader.read());
                })
                .is_ok()
            {
                self.pending = Some(rx);
            } else {
                self.snapshot = StatusSnapshot {
                    network: NetworkStatus::Unavailable,
                    battery: None,
                    ..Default::default()
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    fn write(root: &Path, path: &str, value: &str) {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, value).unwrap();
    }
    #[test]
    fn polling_is_nonblocking_deduplicated_and_can_be_disabled() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct BlockingReader {
            calls: Arc<AtomicUsize>,
            started: mpsc::SyncSender<()>,
            release: std::sync::Mutex<mpsc::Receiver<()>>,
        }
        impl StatusReader for BlockingReader {
            fn read(&self) -> StatusSnapshot {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.started.send(()).unwrap();
                self.release.lock().unwrap().recv().unwrap();
                StatusSnapshot {
                    network: NetworkStatus::Offline,
                    battery: None,
                    ..Default::default()
                }
            }
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let (started, started_rx) = mpsc::sync_channel(1);
        let (release_tx, release) = mpsc::channel();
        let mut monitor = StatusMonitor::new(Arc::new(BlockingReader {
            calls: calls.clone(),
            started,
            release: std::sync::Mutex::new(release),
        }));
        let now = Instant::now();
        monitor.poll(now, false);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        monitor.poll(now, true);
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        for _ in 0..10 {
            monitor.poll(now + Duration::from_secs(10), true);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        release_tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while monitor.snapshot.network != NetworkStatus::Offline {
            monitor.poll(now, false);
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        monitor.poll(now + Duration::from_secs(1), true);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn linux_snapshot_prefers_default_route_and_reports_wifi_quality_and_battery() {
        let dir = TestDir::new();
        let root = dir.path();
        write(root, "sys/class/net/lo/operstate", "unknown\n");
        write(root, "sys/class/net/docker0/operstate", "up\n");
        write(root, "sys/class/net/eth0/operstate", "up\n");
        std::fs::create_dir(root.join("sys/class/net/eth0/device")).unwrap();
        write(root, "sys/class/net/wlan0/operstate", "up\n");
        std::fs::create_dir(root.join("sys/class/net/wlan0/wireless")).unwrap();
        write(
            root,
            "proc/net/route",
            "Iface Destination Gateway Flags RefCnt Use Metric Mask MTU Window IRTT\nwlan0 00000000 0100000A 0003 0 0 100 00000000 0 0 0\neth0 00000000 0100000A 0003 0 0 600 00000000 0 0 0\n",
        );
        write(
            root,
            "proc/net/wireless",
            "Inter-| sta-| Quality\n wlan0: 0000 35. -75. -256 0 0 0\n",
        );
        write(root, "sys/class/power_supply/BAT0/type", "Battery\n");
        write(root, "sys/class/power_supply/BAT0/capacity", "84\n");
        write(root, "sys/class/power_supply/BAT0/status", "Charging\n");
        let snapshot = LinuxStatusReader::at(root).read();
        assert_eq!(
            snapshot.network,
            NetworkStatus::Connected {
                interface: "wlan0".into(),
                wireless: true,
                signal: Some(50)
            }
        );
        assert_eq!(
            snapshot.battery,
            Some(BatteryStatus {
                percent: 84,
                charging: true
            })
        );
        write(root, "sys/class/net/wlan0/operstate", "dormant\n");
        assert_eq!(
            LinuxStatusReader::at(root).read().network,
            NetworkStatus::Connected {
                interface: "eth0".into(),
                wireless: false,
                signal: None
            }
        );
        write(root, "sys/class/net/eth0/operstate", "down\n");
        assert_eq!(
            LinuxStatusReader::at(root).read().network,
            NetworkStatus::Offline
        );
    }
    #[test]
    fn missing_and_malformed_device_data_are_handled_without_fake_status() {
        let dir = TestDir::new();
        let root = dir.path();
        let missing = LinuxStatusReader::at(root).read();
        assert_eq!(missing.network, NetworkStatus::Unavailable);
        assert_eq!(missing.battery, None);
        assert_eq!(missing.resources.cpu_percent, None);
        assert_eq!(missing.resources.ram, None);
        std::fs::create_dir_all(root.join("sys/class/net/wlan0/wireless")).unwrap();
        assert_eq!(
            LinuxStatusReader::at(root).read().network,
            NetworkStatus::Unavailable
        );
        write(root, "sys/class/net/wlan0/operstate", "up\n");
        write(root, "proc/net/wireless", "wlan0: 0000 NaN -999\n");
        write(root, "sys/class/power_supply/BAT0/type", "Battery\n");
        write(root, "sys/class/power_supply/BAT0/capacity", "999\n");
        let status = LinuxStatusReader::at(root).read();
        assert_eq!(
            status.network,
            NetworkStatus::Connected {
                interface: "wlan0".into(),
                wireless: true,
                signal: None
            }
        );
        assert_eq!(status.battery, None);
        write(root, "sys/class/power_supply/BAT0/capacity", "50\n");
        write(root, "sys/class/power_supply/BAT0/present", "0\n");
        assert_eq!(LinuxStatusReader::at(root).read().battery, None);
    }
}
