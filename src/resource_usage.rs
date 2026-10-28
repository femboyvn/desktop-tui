use std::path::Path;
use std::time::Instant;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Usage {
    pub used_bytes: u64,
    pub total_bytes: u64,
}
impl Usage {
    pub fn percent(&self) -> u8 {
        if self.total_bytes == 0 {
            return 0;
        }
        ((u128::from(self.used_bytes) * 100 + u128::from(self.total_bytes) / 2)
            / u128::from(self.total_bytes))
        .min(100) as u8
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkRate {
    pub received_per_second: u64,
    pub transmitted_per_second: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResourceSnapshot {
    pub cpu_percent: Option<u8>,
    pub ram: Option<Usage>,
    pub disk: Option<Usage>,
    pub network: Option<NetworkRate>,
}
#[derive(Clone, Copy)]
struct CpuTimes {
    total: u128,
    idle: u128,
}
#[derive(Default)]
pub struct ResourceSampler {
    cpu: Option<CpuTimes>,
    network: Option<(String, u64, u64, Instant)>,
    network_uses_sysfs: bool,
}
impl ResourceSampler {
    /// Called only by the status worker. The timestamps describe counter reads,
    /// independently of a potentially slower filesystem-capacity query.
    pub fn read(&mut self, root: &Path, interface: Option<&str>) -> ResourceSnapshot {
        let cpu = std::fs::read_to_string(root.join("proc/stat")).unwrap_or_default();
        // Match the same sysfs interface used for connection status. Some
        // container/session namespaces expose only loopback through /proc.
        let sysfs = interface.and_then(|name| {
            let path = root.join("sys/class/net").join(name).join("statistics");
            let rx = std::fs::read_to_string(path.join("rx_bytes"))
                .ok()?
                .trim()
                .parse()
                .ok()?;
            let tx = std::fs::read_to_string(path.join("tx_bytes"))
                .ok()?
                .trim()
                .parse()
                .ok()?;
            Some((rx, tx))
        });
        let uses_sysfs = sysfs.is_some();
        let counters = sysfs.or_else(|| {
            interface.and_then(|name| {
                let dev = std::fs::read_to_string(root.join("proc/net/dev")).ok()?;
                network_counters(&dev, name)
            })
        });
        if uses_sysfs != self.network_uses_sysfs {
            self.network = None;
        }
        self.network_uses_sysfs = uses_sysfs;
        let now = Instant::now();
        let mem = std::fs::read_to_string(root.join("proc/meminfo")).unwrap_or_default();
        let disk = disk_usage(root);
        self.sample(&cpu, &mem, counters, interface, disk, now)
    }
    fn sample(
        &mut self,
        cpu: &str,
        mem: &str,
        counters: Option<(u64, u64)>,
        interface: Option<&str>,
        disk: Option<Usage>,
        now: Instant,
    ) -> ResourceSnapshot {
        let current_cpu = cpu_times(cpu);
        let cpu_percent = current_cpu.zip(self.cpu).and_then(|(current, previous)| {
            let total = current.total.checked_sub(previous.total)?;
            let idle = current.idle.checked_sub(previous.idle)?;
            if total == 0 || idle > total {
                return None;
            }
            Some((((total - idle) * 100 + total / 2) / total) as u8)
        });
        self.cpu = current_cpu;
        let current_network = interface
            .zip(counters)
            .map(|(name, (rx, tx))| (name.to_owned(), rx, tx, now));
        let network = current_network
            .as_ref()
            .zip(self.network.as_ref())
            .and_then(|(current, previous)| {
                if current.0 != previous.0 {
                    return None;
                }
                let elapsed = now.checked_duration_since(previous.3)?.as_nanos();
                if elapsed == 0 {
                    return None;
                }
                let rate = |current: u64, previous: u64| -> Option<u64> {
                    (u128::from(current.checked_sub(previous)?) * 1_000_000_000 / elapsed)
                        .try_into()
                        .ok()
                };
                Some(NetworkRate {
                    received_per_second: rate(current.1, previous.1)?,
                    transmitted_per_second: rate(current.2, previous.2)?,
                })
            });
        self.network = current_network;
        ResourceSnapshot {
            cpu_percent,
            ram: memory_usage(mem),
            disk,
            network,
        }
    }
}
fn cpu_times(contents: &str) -> Option<CpuTimes> {
    let fields = contents
        .lines()
        .find(|line| line.starts_with("cpu "))?
        .split_whitespace()
        .skip(1)
        .collect::<Vec<_>>();
    if fields.len() < 4 {
        return None;
    }
    let values = fields
        .iter()
        .take(8)
        .map(|value| value.parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()?;
    let total = values.iter().map(|value| u128::from(*value)).sum();
    let idle = u128::from(values[3]) + u128::from(*values.get(4).unwrap_or(&0));
    Some(CpuTimes { total, idle })
}
fn memory_usage(contents: &str) -> Option<Usage> {
    let field =
        |name: &str| -> Option<&str> { contents.lines().find_map(|line| line.strip_prefix(name)) };
    let kib = |text: &str| -> Option<u64> {
        let mut fields = text.split_whitespace();
        let value = fields.next()?.parse::<u64>().ok()?;
        if fields.next()? != "kB" || fields.next().is_some() {
            return None;
        }
        Some(value)
    };
    let total = kib(field("MemTotal:")?)?;
    if total == 0 {
        return None;
    }
    let available = if let Some(value) = field("MemAvailable:") {
        kib(value)?
    } else {
        let free = kib(field("MemFree:")?)?;
        let buffers = kib(field("Buffers:")?)?;
        let cached = kib(field("Cached:")?)?;
        let reclaim = field("SReclaimable:").map(kib).unwrap_or(Some(0))?;
        let shared = field("Shmem:").map(kib).unwrap_or(Some(0))?;
        let available =
            u128::from(free) + u128::from(buffers) + u128::from(cached) + u128::from(reclaim);
        available
            .saturating_sub(u128::from(shared))
            .min(u128::from(total)) as u64
    };
    Some(Usage {
        used_bytes: total.checked_sub(available)?.checked_mul(1024)?,
        total_bytes: total.checked_mul(1024)?,
    })
}
fn network_counters(contents: &str, interface: &str) -> Option<(u64, u64)> {
    for line in contents.lines() {
        let Some((name, values)) = line.split_once(':') else {
            continue;
        };
        if name.trim() != interface {
            continue;
        }
        let fields = values.split_whitespace().collect::<Vec<_>>();
        if fields.len() != 16 {
            return None;
        }
        return Some((fields[0].parse().ok()?, fields[8].parse().ok()?));
    }
    None
}
fn usage_from_blocks(blocks: u64, free: u64, available: u64, size: u64) -> Option<Usage> {
    if available > free || size == 0 {
        return None;
    }
    let used = blocks.checked_sub(free)?;
    // Match df's percentage: reserved blocks are excluded from usable capacity.
    let total = used.checked_add(available)?;
    if total == 0 {
        return None;
    }
    Some(Usage {
        used_bytes: used.checked_mul(size)?,
        total_bytes: total.checked_mul(size)?,
    })
}
pub fn disk_usage(path: &Path) -> Option<Usage> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut value = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // statvfs initializes the entire output on success; the CString is NUL-terminated.
    if unsafe { libc::statvfs(path.as_ptr(), value.as_mut_ptr()) } != 0 {
        return None;
    }
    let value = unsafe { value.assume_init() };
    usage_from_blocks(
        value.f_blocks as u64,
        value.f_bfree as u64,
        value.f_bavail as u64,
        if value.f_frsize == 0 {
            value.f_bsize as u64
        } else {
            value.f_frsize as u64
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    fn dev(interface: &str, rx: u64, tx: u64) -> String {
        format!("Inter-| Receive | Transmit\n {interface}: {rx} 0 0 0 0 0 0 0 {tx} 0 0 0 0 0 0 0\n")
    }
    #[test]
    fn sysfs_interface_counters_work_when_proc_has_no_matching_interface() {
        let dir = crate::test_support::TestDir::new();
        let stats = dir.path().join("sys/class/net/wlan0/statistics");
        std::fs::create_dir_all(&stats).unwrap();
        std::fs::write(stats.join("rx_bytes"), "1000\n").unwrap();
        std::fs::write(stats.join("tx_bytes"), "2000\n").unwrap();
        let mut sampler = ResourceSampler::default();
        assert!(sampler.read(dir.path(), Some("wlan0")).network.is_none());
        std::fs::write(stats.join("rx_bytes"), "7144\n").unwrap();
        std::fs::write(stats.join("tx_bytes"), "4048\n").unwrap();
        let rates = sampler
            .read(dir.path(), Some("wlan0"))
            .network
            .expect("selected interface must supply its own counters when proc is unavailable");
        assert!(rates.received_per_second > 0);
        assert!(rates.transmitted_per_second > 0);
        // Switching counter sources must establish a fresh baseline too.
        std::fs::create_dir_all(dir.path().join("proc/net")).unwrap();
        std::fs::write(
            dir.path().join("proc/net/dev"),
            dev("wlan0", 1_000_000, 2_000_000),
        )
        .unwrap();
        std::fs::remove_file(stats.join("rx_bytes")).unwrap();
        assert!(sampler.read(dir.path(), Some("wlan0")).network.is_none());
    }
    #[test]
    fn cpu_samples_use_busy_deltas_without_double_counting_guest_time() {
        let mut sampler = ResourceSampler::default();
        let now = Instant::now();
        let first = sampler.sample(
            "cpu 100 0 50 800 50 0 0 0 20 0\n",
            "",
            None,
            None,
            None,
            now,
        );
        assert_eq!(first.cpu_percent, None);
        let second = sampler.sample(
            "cpu 130 0 70 840 60 0 0 0 40 0\n",
            "",
            None,
            None,
            None,
            now + Duration::from_secs(1),
        );
        assert_eq!(second.cpu_percent, Some(50));
        assert_eq!(
            sampler
                .sample("cpu 130 0 70 840 60 0 0 0\n", "", None, None, None, now)
                .cpu_percent,
            None
        );
        assert_eq!(
            sampler
                .sample("cpu 1 0 0 2 0 0 0 0\n", "", None, None, None, now)
                .cpu_percent,
            None
        );
        assert_eq!(
            sampler
                .sample("cpu broken\n", "", None, None, None, now)
                .cpu_percent,
            None
        );
    }
    #[test]
    fn memory_uses_available_and_a_cache_aware_fallback_without_bad_values() {
        let usage =
            memory_usage("MemTotal: 1000 kB\nMemFree: 50 kB\nMemAvailable: 400 kB\n").unwrap();
        assert_eq!(usage.used_bytes, 600 * 1024);
        assert_eq!(usage.total_bytes, 1000 * 1024);
        assert_eq!(usage.percent(), 60);
        assert_eq!(memory_usage("MemTotal: 1000 kB\nMemFree: 100 kB\nBuffers: 50 kB\nCached: 200 kB\nSReclaimable: 100 kB\nShmem: 50 kB\n").unwrap().percent(), 60);
        for bad in [
            "",
            "MemTotal: 0 kB\nMemAvailable: 0 kB\n",
            "MemTotal: 100 kB\nMemAvailable: 101 kB\n",
            "MemTotal: 100 kB\nMemAvailable: broken\n",
            "MemTotal: 100 MB\nMemAvailable: 50 kB\n",
        ] {
            assert!(memory_usage(bad).is_none(), "{bad}");
        }
    }
    #[test]
    fn network_rates_use_elapsed_time_and_reset_on_interface_or_counter_changes() {
        let mut sampler = ResourceSampler::default();
        let now = Instant::now();
        let first = sampler.sample(
            "",
            "",
            network_counters(&dev("wlan0", 1000, 2000), "wlan0"),
            Some("wlan0"),
            None,
            now,
        );
        assert_eq!(first.network, None);
        let second = sampler.sample(
            "",
            "",
            network_counters(&dev("wlan0", 7144, 4048), "wlan0"),
            Some("wlan0"),
            None,
            now + Duration::from_secs(2),
        );
        assert_eq!(
            second.network,
            Some(NetworkRate {
                received_per_second: 3072,
                transmitted_per_second: 1024
            })
        );
        assert_eq!(
            sampler
                .sample(
                    "",
                    "",
                    network_counters(&dev("wlan0", 1, 1), "wlan0"),
                    Some("wlan0"),
                    None,
                    now + Duration::from_secs(3)
                )
                .network,
            None
        );
        assert_eq!(
            sampler
                .sample(
                    "",
                    "",
                    network_counters(&dev("eth0", 1000, 1000), "eth0"),
                    Some("eth0"),
                    None,
                    now + Duration::from_secs(4)
                )
                .network,
            None
        );
        assert_eq!(
            sampler
                .sample(
                    "",
                    "",
                    network_counters(&dev("eth0", 2000, 2000), "eth0"),
                    Some("eth0"),
                    None,
                    now + Duration::from_secs(4)
                )
                .network,
            None
        );
        assert_eq!(
            sampler
                .sample(
                    "",
                    "",
                    network_counters("bad", "eth0"),
                    Some("eth0"),
                    None,
                    now + Duration::from_secs(5)
                )
                .network,
            None
        );
        assert_eq!(
            sampler
                .sample(
                    "",
                    "",
                    network_counters(&dev("eth0", 3000, 3000), "eth0"),
                    Some("eth0"),
                    None,
                    now + Duration::from_secs(6)
                )
                .network,
            None
        );
    }
    #[test]
    fn disk_usage_accounts_for_reserved_blocks_and_handles_missing_mounts() {
        let usage = usage_from_blocks(1000, 400, 300, 4096).unwrap();
        assert_eq!(usage.used_bytes, 600 * 4096);
        assert_eq!(usage.total_bytes, 900 * 4096);
        assert_eq!(usage.percent(), 67);
        assert!(usage_from_blocks(0, 0, 0, 4096).is_none());
        assert!(usage_from_blocks(100, 101, 100, 4096).is_none());
        let dir = crate::test_support::TestDir::new();
        assert!(disk_usage(dir.path()).is_some());
        assert!(disk_usage(&dir.path().join("missing")).is_none());
    }
}
