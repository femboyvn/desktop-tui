use crate::preferences::{DesktopPalette, Preferences, Rgb, TerminalPalette};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PalettePreset {
    Femboy,
    Transgender,
    Asexual,
    Nonbinary,
    Monochrome,
}
impl PalettePreset {
    pub const ALL: [Self; 5] = [
        Self::Femboy,
        Self::Transgender,
        Self::Asexual,
        Self::Nonbinary,
        Self::Monochrome,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Femboy => "Femboy",
            Self::Transgender => "Transgender",
            Self::Asexual => "Asexual",
            Self::Nonbinary => "Nonbinary",
            Self::Monochrome => "Monochrome",
        }
    }
    pub fn preferences(self) -> Preferences {
        let rgb = |hex: u32| Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
        let (desktop, foreground, background, ansi) = match self {
            Self::Femboy => return Preferences::default(),
            Self::Transgender => (
                [
                    0xE0F5FF, 0xFFF7FA, 0x2E3D54, 0x667388, 0xD59BB0, 0xF5A9B8, 0x5BCEFA,
                ],
                0x2E3D54,
                0xFFFBFD,
                [
                    0x2E3D54, 0xAE4D70, 0x397D67, 0x956B33, 0x287FA8, 0x96619E, 0x39899C, 0xD8E4ED,
                    0x68788B, 0xD7799A, 0x69A58B, 0xC09961, 0x5BCEFA, 0xC498CA, 0x7DB9C7, 0xFFFFFF,
                ],
            ),
            Self::Asexual => (
                [
                    0x202024, 0x2B2B31, 0xF5F5F5, 0xA3A3A3, 0x696970, 0x800080, 0x5D416E,
                ],
                0xF5F5F5,
                0x202024,
                [
                    0x151519, 0xD37287, 0x8BAF98, 0xC6B389, 0x8B9CBB, 0xB37AC8, 0x8EB7BD, 0xD2D2D2,
                    0x777780, 0xF0A1B1, 0xB1D1BE, 0xE8D6AF, 0xB4C2E0, 0xD6A8E6, 0xB7DCE0, 0xFFFFFF,
                ],
            ),
            Self::Nonbinary => (
                [
                    0xFFF8C5, 0xFFFDFA, 0x2D203A, 0x796586, 0x9C59D1, 0xFFF430, 0xCBA6EB,
                ],
                0x2D203A,
                0xFFFDF2,
                [
                    0x2D203A, 0xA84C65, 0x4E7F52, 0x917126, 0x596EAA, 0x9253B8, 0x3F838A, 0xDCD5E4,
                    0x786785, 0xD77D93, 0x81AF82, 0xC3A34D, 0x8B9ACB, 0xBC8ADB, 0x7BB5BC, 0xFFFFFF,
                ],
            ),
            Self::Monochrome => (
                [
                    0x242424, 0x303030, 0xF0F0F0, 0xB0B0B0, 0x707070, 0x484848, 0x585858,
                ],
                0xF0F0F0,
                0x242424,
                [
                    0x202020, 0x606060, 0x707070, 0x808080, 0x909090, 0xA0A0A0, 0xB0B0B0, 0xD0D0D0,
                    0x707070, 0x909090, 0xA0A0A0, 0xB0B0B0, 0xC0C0C0, 0xD0D0D0, 0xE0E0E0, 0xF8F8F8,
                ],
            ),
        };
        let [
            background_desktop,
            surface,
            text,
            muted_text,
            border,
            accent,
            selection,
        ] = desktop.map(rgb);
        Preferences {
            terminal_lock: None,
            panel: crate::panel::PanelPreferences::default(),
            desktop: DesktopPalette {
                background: background_desktop,
                surface,
                text,
                muted_text,
                border,
                accent,
                selection,
            },
            terminal: TerminalPalette {
                foreground: rgb(foreground),
                background: rgb(background),
                ansi: ansi.map(rgb),
            },
        }
    }
    pub fn matching(preferences: &Preferences) -> Option<Self> {
        Self::ALL.into_iter().find(|preset| {
            let palette = preset.preferences();
            palette.desktop == preferences.desktop && palette.terminal == preferences.terminal
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::preferences::Rgb;
    use crate::test_support::TestDir;
    #[test]
    fn presets_have_distinct_complete_palettes_and_round_trip_settings() {
        let dir = TestDir::new();
        let path = dir.path().join("preferences.toml");
        for (i, preset) in PalettePreset::ALL.into_iter().enumerate() {
            let preferences = preset.preferences();
            assert_eq!(PalettePreset::matching(&preferences), Some(preset));
            assert_eq!(preferences.terminal.ansi.len(), 16);
            for other in &PalettePreset::ALL[..i] {
                assert_ne!(preferences, other.preferences());
            }
            preferences.save(&path, None).unwrap();
            assert_eq!(Preferences::load(&path), (preferences, None));
        }
        assert_eq!(PalettePreset::Femboy.preferences(), Preferences::default());
        let trans = PalettePreset::Transgender.preferences();
        assert_eq!(trans.desktop.accent, Rgb(245, 169, 184));
        assert_eq!(trans.desktop.selection, Rgb(91, 206, 250));
        assert_eq!(
            PalettePreset::Asexual.preferences().desktop.accent,
            Rgb(128, 0, 128)
        );
        assert_eq!(
            PalettePreset::Nonbinary.preferences().desktop.accent,
            Rgb(255, 244, 48)
        );
        let mut edited = trans;
        edited.desktop.text = Rgb(1, 2, 3);
        assert_eq!(PalettePreset::matching(&edited), None);
    }
    #[test]
    fn monochrome_uses_only_neutral_colors_in_both_palettes() {
        let p = PalettePreset::Monochrome.preferences();
        let d = p.desktop;
        let colors = [
            d.background,
            d.surface,
            d.text,
            d.muted_text,
            d.border,
            d.accent,
            d.selection,
            p.terminal.foreground,
            p.terminal.background,
        ]
        .into_iter()
        .chain(p.terminal.ansi);
        for Rgb(r, g, b) in colors {
            assert_eq!(r, g);
            assert_eq!(g, b);
        }
    }
}
