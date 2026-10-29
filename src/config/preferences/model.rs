use crate::runtime::LockCredential;
use appcui::prelude::Color;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Rgb(pub u8, pub u8, pub u8);
impl TryFrom<String> for Rgb {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let bytes = value.as_bytes();
        if bytes.len() != 7 || bytes[0] != b'#' || !bytes[1..].iter().all(u8::is_ascii_hexdigit) {
            return Err("Use a color in #RRGGBB format (for example #F6D9EE).".into());
        }
        Ok(Self(
            u8::from_str_radix(&value[1..3], 16).unwrap(),
            u8::from_str_radix(&value[3..5], 16).unwrap(),
            u8::from_str_radix(&value[5..7], 16).unwrap(),
        ))
    }
}
impl From<Rgb> for String {
    fn from(value: Rgb) -> Self {
        format!("#{:02X}{:02X}{:02X}", value.0, value.1, value.2)
    }
}
impl Rgb {
    pub fn color(self) -> Color {
        Color::RGB(self.0, self.1, self.2)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopPalette {
    pub background: Rgb,
    pub surface: Rgb,
    pub text: Rgb,
    pub muted_text: Rgb,
    pub border: Rgb,
    pub accent: Rgb,
    pub selection: Rgb,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalPalette {
    pub foreground: Rgb,
    pub background: Rgb,
    pub ansi: [Rgb; 16],
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preferences {
    pub desktop: DesktopPalette,
    pub terminal: TerminalPalette,
    #[serde(default, skip_serializing_if = "super::AsciiBackground::is_empty")]
    pub ascii_background: super::AsciiBackground,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_lock: Option<LockCredential>,
    #[serde(default)]
    pub panel: PanelPreferences,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            ascii_background: super::AsciiBackground::default(),
            terminal_lock: None,
            panel: PanelPreferences::default(),
            desktop: DesktopPalette {
                background: Rgb(246, 217, 238),
                surface: Rgb(255, 247, 252),
                text: Rgb(63, 42, 74),
                muted_text: Rgb(132, 104, 144),
                border: Rgb(177, 140, 181),
                accent: Rgb(232, 164, 201),
                selection: Rgb(200, 181, 234),
            },
            terminal: TerminalPalette {
                foreground: Rgb(63, 42, 74),
                background: Rgb(255, 247, 252),
                // Plum, rose, sage, ochre, blue, mauve, teal, and pearl.
                ansi: [
                    Rgb(63, 42, 74),
                    Rgb(177, 67, 104),
                    Rgb(77, 128, 94),
                    Rgb(157, 117, 47),
                    Rgb(100, 110, 177),
                    Rgb(157, 89, 166),
                    Rgb(63, 131, 142),
                    Rgb(218, 199, 219),
                    Rgb(132, 104, 144),
                    Rgb(219, 112, 151),
                    Rgb(126, 172, 139),
                    Rgb(203, 164, 96),
                    Rgb(153, 159, 215),
                    Rgb(199, 146, 205),
                    Rgb(119, 183, 193),
                    Rgb(255, 247, 252),
                ],
            },
        }
    }
}
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_light_femboy() {
        let p = Preferences::default();
        assert_eq!(p.desktop.background, Rgb(246, 217, 238));
        assert_eq!(p.desktop.surface, Rgb(255, 247, 252));
        assert_eq!(p.desktop.text, Rgb(63, 42, 74));
        assert_eq!(p.desktop.accent, Rgb(232, 164, 201));
        assert_eq!(p.desktop.selection, Rgb(200, 181, 234));
        assert_ne!(p.terminal.ansi[0], p.terminal.ansi[8]);
    }

    #[test]
    fn validates_rgb_without_accepting_partial_or_non_ascii_values() {
        assert_eq!(
            Rgb::try_from("#aBcD09".to_owned()).unwrap(),
            Rgb(171, 205, 9)
        );
        assert_eq!(String::from(Rgb(171, 205, 9)), "#ABCD09");
        for invalid in [
            "red", "123456", "#123", "#1234567", "#GG0000", "#é1234", " #123456",
        ] {
            assert!(Rgb::try_from(invalid.to_owned()).is_err(), "{invalid}");
        }
    }
}
