use crate::linux_status::{NetworkStatus, StatusSnapshot};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateFormat {
    #[default]
    Short,
    Iso,
    DayMonth,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PanelPreferences {
    pub enabled: bool,
    pub show_clock: bool,
    pub show_date: bool,
    pub show_network: bool,
    pub show_battery: bool,
    pub use_24_hour: bool,
    pub show_seconds: bool,
    pub show_interface: bool,
    pub show_cpu: bool,
    pub show_ram: bool,
    pub show_disk: bool,
    pub show_network_usage: bool,
    pub date_format: DateFormat,
}
impl Default for PanelPreferences {
    fn default() -> Self {
        Self {
            enabled: true,
            show_clock: true,
            show_date: true,
            show_network: true,
            show_battery: true,
            use_24_hour: true,
            show_seconds: false,
            show_interface: false,
            show_cpu: true,
            show_ram: true,
            show_disk: true,
            show_network_usage: true,
            date_format: DateFormat::Short,
        }
    }
}
/// Only complete indicators are shown. Clock and connection take precedence on
/// small screens; no partial date or percentage can be mistaken for a real value.
pub fn render(
    settings: &PanelPreferences,
    status: &StatusSnapshot,
    now: NaiveDateTime,
    width: usize,
    pending: bool,
) -> String {
    // Order is visual order; priority controls which complete readings survive
    // when even the compact panel cannot fit.
    let mut parts: Vec<(usize, usize, String, String)> = Vec::new();
    if pending {
        parts.push((0, 0, "System pending".into(), "Busy".into()));
    }
    if settings.enabled {
        if settings.show_clock {
            let format = match (settings.use_24_hour, settings.show_seconds) {
                (true, false) => "%H:%M",
                (true, true) => "%H:%M:%S",
                (false, false) => "%I:%M %p",
                (false, true) => "%I:%M:%S %p",
            };
            let text = now.format(format).to_string();
            parts.push((1, 1, text.clone(), text));
        }
        if settings.show_date {
            let format = match settings.date_format {
                DateFormat::Short => "%a %d %b",
                DateFormat::Iso => "%Y-%m-%d",
                DateFormat::DayMonth => "%d/%m",
            };
            parts.push((
                2,
                3,
                now.format(format).to_string(),
                now.format("%d/%m").to_string(),
            ));
        }
        if settings.show_network {
            let compact = match &status.network {
                NetworkStatus::Checking => "Net...".into(),
                NetworkStatus::Unavailable => "Net?".into(),
                NetworkStatus::Offline => "Offline".into(),
                NetworkStatus::Connected {
                    wireless: true,
                    signal,
                    ..
                } => signal.map_or_else(|| "WiFi".into(), |value| format!("W{value}%")),
                NetworkStatus::Connected { .. } => "Net".into(),
            };
            parts.push((
                3,
                2,
                network_caption(&status.network, settings.show_interface),
                compact,
            ));
        }
        if settings.show_battery {
            if let Some(battery) = &status.battery {
                let suffix = if battery.charging { "+" } else { "" };
                parts.push((
                    4,
                    4,
                    format!("Bat {}%{suffix}", battery.percent),
                    format!("B{}%{suffix}", battery.percent),
                ));
            }
        }
        for (order, enabled, label, short, value) in [
            (
                5,
                settings.show_cpu,
                "CPU",
                "C",
                status.resources.cpu_percent,
            ),
            (
                6,
                settings.show_ram,
                "RAM",
                "M",
                status.resources.ram.as_ref().map(|usage| usage.percent()),
            ),
            (
                7,
                settings.show_disk,
                "Disk",
                "D",
                status.resources.disk.as_ref().map(|usage| usage.percent()),
            ),
        ] {
            if enabled {
                let (full, compact) = value.map_or_else(
                    || (format!("{label} N/A"), format!("{short}?")),
                    |value| (format!("{label} {value}%"), format!("{short}{value}%")),
                );
                parts.push((order, order, full, compact));
            }
        }
        if settings.show_network_usage {
            let (full, compact) = status.resources.network.as_ref().map_or_else(
                || ("Net I/O N/A".into(), "v?^?".into()),
                |rate| {
                    (
                        format!(
                            "Rx {} Tx {}",
                            format_rate(rate.received_per_second, false),
                            format_rate(rate.transmitted_per_second, false)
                        ),
                        format!(
                            "v{}^{}",
                            format_rate(rate.received_per_second, true),
                            format_rate(rate.transmitted_per_second, true)
                        ),
                    )
                },
            );
            parts.push((8, 8, full, compact));
        }
    }
    let available = width.saturating_sub(1);
    let full = parts
        .iter()
        .map(|part| part.2.as_str())
        .collect::<Vec<_>>()
        .join(" | ");
    if full.len() <= available {
        return if full.is_empty() { full } else { full + " " };
    }
    parts.sort_by_key(|part| part.1);
    let mut selected = Vec::new();
    let mut used = 0;
    for (order, _, _, text) in parts {
        let length = text.len() + usize::from(!selected.is_empty());
        if used + length <= available {
            used += length;
            selected.push((order, text));
        }
    }
    selected.sort_by_key(|part| part.0);
    let text = selected
        .into_iter()
        .map(|part| part.1)
        .collect::<Vec<_>>()
        .join(" ");
    if text.is_empty() { text } else { text + " " }
}

fn format_rate(bytes: u64, compact: bool) -> String {
    let units = [
        ("B/s", "B"),
        ("KiB/s", "K"),
        ("MiB/s", "M"),
        ("GiB/s", "G"),
        ("TiB/s", "T"),
        ("PiB/s", "P"),
    ];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < units.len() {
        value /= 1024.0;
        unit += 1;
    }
    let number = if unit == 0 || value >= 10.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned()
    };
    if compact {
        format!("{number}{}", units[unit].1)
    } else {
        format!("{number} {}", units[unit].0)
    }
}

fn network_caption(status: &NetworkStatus, show_interface: bool) -> String {
    match status {
        NetworkStatus::Checking => "Net ...".to_owned(),
        NetworkStatus::Unavailable => "Net N/A".to_owned(),
        NetworkStatus::Offline => "Net offline".to_owned(),
        NetworkStatus::Connected {
            interface,
            wireless,
            signal,
        } => {
            let mut text = if *wireless {
                signal.map_or_else(
                    || "WiFi up".to_owned(),
                    |percent| format!("WiFi {percent}%"),
                )
            } else {
                "Net up".to_owned()
            };
            if show_interface {
                let interface: String = interface
                    .chars()
                    .filter(char::is_ascii_graphic)
                    .take(15)
                    .collect();
                if !interface.is_empty() {
                    text.push(' ');
                    text.push_str(&interface);
                }
            }
            text
        }
    }
}

pub struct BarLayout {
    pub status_width: usize,
    pub grouped_apps: bool,
}
pub fn bar_layout(width: usize, desired_status: usize, app_widths: &[usize]) -> BarLayout {
    // Desktop + Tilling + separator = 19. AppCUI requires more than five
    // free columns even when every item is placed on the left.
    let available = width.saturating_sub(6);
    let grouped_apps = !app_widths.is_empty()
        && 19 + app_widths.iter().sum::<usize>() + desired_status > available;
    let menu_width = 19
        + if grouped_apps {
            6
        } else {
            app_widths.iter().sum::<usize>()
        };
    BarLayout {
        status_width: desired_status
            .min(128)
            .min(available.saturating_sub(menu_width)),
        grouped_apps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linux_status::{BatteryStatus, NetworkStatus, StatusSnapshot};
    use crate::preferences::Preferences;
    use crate::test_support::TestDir;
    fn now() -> chrono::NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 10, 27)
            .unwrap()
            .and_hms_opt(14, 5, 9)
            .unwrap()
    }
    #[test]
    fn resource_indicators_have_full_and_bounded_compact_readings() {
        use crate::resource_usage::{NetworkRate, ResourceSnapshot, Usage};
        let settings = PanelPreferences::default();
        let status = StatusSnapshot {
            network: NetworkStatus::Connected {
                interface: "wlan0".into(),
                wireless: true,
                signal: Some(75),
            },
            battery: Some(BatteryStatus {
                percent: 84,
                charging: true,
            }),
            resources: ResourceSnapshot {
                cpu_percent: Some(18),
                ram: Some(Usage {
                    used_bytes: 600,
                    total_bytes: 1000,
                }),
                disk: Some(Usage {
                    used_bytes: 600,
                    total_bytes: 900,
                }),
                network: Some(NetworkRate {
                    received_per_second: 3072,
                    transmitted_per_second: 1024,
                }),
            },
        };
        assert_eq!(
            render(&settings, &status, now(), 300, false),
            "14:05 | Tue 27 Oct | WiFi 75% | Bat 84%+ | CPU 18% | RAM 60% | Disk 67% | Rx 3 KiB/s Tx 1 KiB/s "
        );
        let compact = render(&settings, &status, now(), 49, false);
        for value in [
            "14:05", "27/10", "W75%", "B84%+", "C18%", "M60%", "D67%", "v3K^1K",
        ] {
            assert!(compact.contains(value), "{compact}");
        }
        assert!(compact.len() <= 49);
        for width in 0..128 {
            assert!(render(&settings, &status, now(), width, false).len() <= width);
        }
        let mut hidden = settings;
        hidden.show_cpu = false;
        hidden.show_ram = false;
        hidden.show_disk = false;
        hidden.show_network_usage = false;
        let caption = render(&hidden, &status, now(), 300, false);
        assert!(
            !caption.contains("CPU")
                && !caption.contains("RAM")
                && !caption.contains("Disk")
                && !caption.contains("Rx")
        );
    }
    #[test]
    fn bar_groups_applications_before_status_can_hide_their_menus() {
        let normal = bar_layout(80, 30, &[10, 8]);
        assert!(!normal.grouped_apps);
        assert_eq!(normal.status_width, 30);
        let crowded = bar_layout(80, 45, &[20, 20, 20]);
        assert!(crowded.grouped_apps);
        assert_eq!(crowded.status_width, 45);
        for width in 32..160 {
            let layout = bar_layout(width, 100, &[20, 20, 20]);
            assert!(layout.grouped_apps);
            assert!(layout.status_width + 19 + 6 + 6 <= width);
        }
        assert_eq!(bar_layout(80, 0, &[]).status_width, 0);
    }
    #[test]
    fn formats_clock_date_network_and_battery_with_configurable_visibility() {
        let mut settings = PanelPreferences {
            show_cpu: false,
            show_ram: false,
            show_disk: false,
            show_network_usage: false,
            ..Default::default()
        };
        let status = StatusSnapshot {
            network: NetworkStatus::Connected {
                interface: "wlan0".into(),
                wireless: true,
                signal: Some(75),
            },
            battery: Some(BatteryStatus {
                percent: 84,
                charging: true,
            }),
            ..Default::default()
        };
        assert_eq!(
            render(&settings, &status, now(), 100, false),
            "14:05 | Tue 27 Oct | WiFi 75% | Bat 84%+ "
        );
        settings.use_24_hour = false;
        settings.show_seconds = true;
        settings.show_interface = true;
        settings.date_format = DateFormat::Iso;
        assert_eq!(
            render(&settings, &status, now(), 100, false),
            "02:05:09 PM | 2026-10-27 | WiFi 75% wlan0 | Bat 84%+ "
        );
        settings.show_clock = false;
        settings.show_date = false;
        settings.show_battery = false;
        assert_eq!(
            render(&settings, &status, now(), 100, false),
            "WiFi 75% wlan0 "
        );
        settings.enabled = false;
        assert_eq!(render(&settings, &status, now(), 100, false), "");
        assert_eq!(
            render(&settings, &status, now(), 100, true),
            "System pending "
        );
    }
    #[test]
    fn compact_panel_is_bounded_and_never_invents_a_connection_or_battery() {
        let settings = PanelPreferences {
            show_cpu: false,
            show_ram: false,
            show_disk: false,
            show_network_usage: false,
            ..Default::default()
        };
        let offline = StatusSnapshot {
            network: NetworkStatus::Offline,
            battery: None,
            ..Default::default()
        };
        assert_eq!(
            render(&settings, &offline, now(), 100, false),
            "14:05 | Tue 27 Oct | Net offline "
        );
        for width in 0..80 {
            let caption = render(&settings, &offline, now(), width, true);
            assert!(caption.is_ascii());
            assert!(caption.len() <= width);
            assert!(!caption.contains("Bat"));
        }
        assert_eq!(
            render(&settings, &offline, now(), 17, false),
            "14:05 Offline "
        );
        let mut unknown = offline;
        unknown.network = NetworkStatus::Unavailable;
        assert!(render(&settings, &unknown, now(), 100, false).contains("Net N/A"));
    }
    #[test]
    fn narrow_panel_keeps_connection_before_optional_interface_name() {
        let mut settings = PanelPreferences {
            show_cpu: false,
            show_ram: false,
            show_disk: false,
            show_network_usage: false,
            ..Default::default()
        };
        settings.show_interface = true;
        let status = StatusSnapshot {
            network: NetworkStatus::Connected {
                interface: "verylongwireless".into(),
                wireless: true,
                signal: Some(80),
            },
            battery: None,
            ..Default::default()
        };
        assert_eq!(
            render(&settings, &status, now(), 20, false),
            "14:05 27/10 W80% "
        );
    }
    #[test]
    fn panel_settings_round_trip_and_legacy_files_keep_defaults() {
        let dir = TestDir::new();
        let path = dir.path().join("preferences.toml");
        let mut p = Preferences::default();
        p.panel.enabled = false;
        p.panel.show_seconds = true;
        p.panel.date_format = DateFormat::DayMonth;
        p.save(&path, None).unwrap();
        assert_eq!(Preferences::load(&path), (p.clone(), None));
        let mut legacy: toml::Value =
            toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        // Existing users already have a Panel section with their choices;
        // only the four new meter fields are absent on upgrade.
        let mut previous_panel = legacy.clone();
        let panel = previous_panel
            .get_mut("panel")
            .unwrap()
            .as_table_mut()
            .unwrap();
        for field in ["show_cpu", "show_ram", "show_disk", "show_network_usage"] {
            panel.remove(field);
        }
        std::fs::write(&path, toml::to_string(&previous_panel).unwrap()).unwrap();
        assert_eq!(Preferences::load(&path), (p.clone(), None));
        legacy.as_table_mut().unwrap().remove("panel");
        std::fs::write(&path, toml::to_string(&legacy).unwrap()).unwrap();
        let (loaded, warning) = Preferences::load(&path);
        assert!(warning.is_none());
        assert_eq!(loaded.panel, PanelPreferences::default());
        assert_eq!(loaded.desktop, p.desktop);
    }
}
