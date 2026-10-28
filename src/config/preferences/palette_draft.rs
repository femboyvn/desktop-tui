use crate::config::preferences::{Preferences, Rgb};
pub const DESKTOP_ROLES: [&str; 7] = [
    "Background",
    "Surface",
    "Text",
    "Muted text",
    "Border",
    "Accent",
    "Selection",
];
pub const TERMINAL_ROLES: [&str; 18] = [
    "Default foreground",
    "Default background",
    "Black (0)",
    "Red (1)",
    "Green (2)",
    "Yellow (3)",
    "Blue (4)",
    "Magenta (5)",
    "Cyan (6)",
    "White (7)",
    "Bright black (8)",
    "Bright red (9)",
    "Bright green (10)",
    "Bright yellow (11)",
    "Bright blue (12)",
    "Bright magenta (13)",
    "Bright cyan (14)",
    "Bright white (15)",
];
pub struct PaletteDraft {
    pub desktop: [String; 7],
    pub terminal: [String; 18],
}
impl PaletteDraft {
    pub fn new(p: &Preferences) -> Self {
        let d = &p.desktop;
        Self {
            desktop: [
                d.background,
                d.surface,
                d.text,
                d.muted_text,
                d.border,
                d.accent,
                d.selection,
            ]
            .map(String::from),
            terminal: std::array::from_fn(|index| {
                String::from(match index {
                    0 => p.terminal.foreground,
                    1 => p.terminal.background,
                    i => p.terminal.ansi[i - 2],
                })
            }),
        }
    }
    pub fn validate(&self) -> Result<Preferences, String> {
        let mut p = Preferences::default();
        let d = &mut p.desktop;
        let roles = [
            &mut d.background,
            &mut d.surface,
            &mut d.text,
            &mut d.muted_text,
            &mut d.border,
            &mut d.accent,
            &mut d.selection,
        ];
        for (i, color) in roles.into_iter().enumerate() {
            *color = Rgb::try_from(self.desktop[i].clone())
                .map_err(|error| format!("Desktop / {}: {error}", DESKTOP_ROLES[i]))?;
        }
        for (i, value) in self.terminal.iter().enumerate() {
            let color = Rgb::try_from(value.clone())
                .map_err(|error| format!("Terminal / {}: {error}", TERMINAL_ROLES[i]))?;
            match i {
                0 => p.terminal.foreground = color,
                1 => p.terminal.background = color,
                i => p.terminal.ansi[i - 2] = color,
            }
        }
        Ok(p)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn draft_keeps_all_edits_separate_until_validated() {
        let active = Preferences::default();
        let mut draft = PaletteDraft::new(&active);
        draft.desktop[0] = "#123456".into();
        draft.terminal[17] = "#ABCDEF".into();
        let edited = draft.validate().unwrap();
        assert_eq!(String::from(edited.desktop.background), "#123456");
        assert_eq!(String::from(edited.terminal.ansi[15]), "#ABCDEF");
        assert_eq!(String::from(active.desktop.background), "#F6D9EE");
        draft.desktop[6] = "bad".into();
        assert!(draft.validate().unwrap_err().contains("Selection"));
        draft = PaletteDraft::new(&Preferences::default());
        assert_eq!(draft.validate().unwrap(), active);
    }
}
