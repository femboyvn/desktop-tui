use appcui::input::{Key, KeyCode, KeyModifier};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct PaletteShortcut(String);
impl Default for PaletteShortcut {
    fn default() -> Self { Self("Ctrl+Space".into()) }
}
impl PaletteShortcut {
    pub fn key(&self) -> Key { parse_key(&self.0).unwrap_or_else(|| Key::new(KeyCode::Space,KeyModifier::Ctrl)) }
    pub fn label(&self) -> &str { &self.0 }
    pub fn from_key(key: Key) -> Option<Self> { Self::try_from(format!("{}{}",key.modifier.name(),key.code.name())).ok() }
}
impl TryFrom<String> for PaletteShortcut {
    type Error = String;
    fn try_from(value: String) -> Result<Self,Self::Error> {
        let key = parse_key(&value).ok_or_else(|| "Use a modified key such as Ctrl+Space or a function key such as F12 for desktop commands.".to_owned())?;
        Ok(Self(format!("{}{}",key.modifier.name(),key.code.name())))
    }
}
impl From<PaletteShortcut> for String { fn from(value: PaletteShortcut) -> Self { value.0 } }
fn parse_key(value: &str) -> Option<Key> {
    let mut parts = value.split('+').collect::<Vec<_>>();
    let name = parts.pop()?;
    let code = (0u8..64).map(KeyCode::from).find(|code| code.name().eq_ignore_ascii_case(name))?;
    let mut modifier = KeyModifier::None;
    for part in parts {
        modifier |= match part.to_ascii_lowercase().as_str() { "ctrl" => KeyModifier::Ctrl, "alt" => KeyModifier::Alt, "shift" => KeyModifier::Shift, _ => return None };
    }
    let function = matches!(code,KeyCode::F1|KeyCode::F2|KeyCode::F3|KeyCode::F4|KeyCode::F5|KeyCode::F6|KeyCode::F7|KeyCode::F8|KeyCode::F9|KeyCode::F10|KeyCode::F11|KeyCode::F12);
    if code == KeyCode::None || matches!(code,KeyCode::Escape|KeyCode::Enter) || (code == KeyCode::C && modifier == KeyModifier::Ctrl)
        || (!function && !modifier.contains(KeyModifier::Ctrl) && !modifier.contains(KeyModifier::Alt)) { return None; }
    Some(Key::new(code,modifier))
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct WorkflowPreferences {
    pub palette_shortcut: PaletteShortcut,
    pub file_associations: BTreeMap<String,String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_bindings_preserve_ordinary_typing_and_normal_interrupts() {
        assert_eq!(PaletteShortcut::try_from("ctrl+space".to_owned()).unwrap().key(),Key::new(KeyCode::Space,KeyModifier::Ctrl));
        assert_eq!(PaletteShortcut::try_from("F12".to_owned()).unwrap().key(),Key::new(KeyCode::F12,KeyModifier::None));
        for invalid in ["a","Shift+A","Ctrl+C","Escape","Enter","Ctrl+unknown"] { assert!(PaletteShortcut::try_from(invalid.to_owned()).is_err(),"{invalid}"); }
        let preferences: WorkflowPreferences = toml::from_str("").unwrap();
        assert_eq!(preferences.palette_shortcut.key(),Key::new(KeyCode::Space,KeyModifier::Ctrl));
    }
}
