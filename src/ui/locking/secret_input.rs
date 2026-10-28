use crate::runtime::MAX_PASSWORD_BYTES;
use appcui::prelude::*;
use zeroize::{Zeroize, Zeroizing};

#[CustomControl(overwrite = OnPaint+OnKeyPressed)]
pub(crate) struct SecretInput {
    pub(crate) value: Zeroizing<String>,
    selected_all: bool,
}
impl SecretInput {
    pub(crate) fn new(layout: Layout) -> Self {
        Self {
            base: ControlBase::new(layout, true),
            value: Zeroizing::new(String::with_capacity(MAX_PASSWORD_BYTES)),
            selected_all: false,
        }
    }
    pub(crate) fn clear(&mut self) {
        self.value.zeroize();
        self.selected_all = false;
    }
}
impl OnPaint for SecretInput {
    fn on_paint(&self, surface: &mut Surface, theme: &Theme) {
        let attr = if self.selected_all {
            theme.editor.pressed_or_selected
        } else if self.has_focus() {
            theme.editor.focused
        } else {
            theme.editor.normal
        };
        surface.clear(Character::with_attributes(' ', attr));
        let width = surface.size().width;
        let count = (self.value.chars().count() as u32).min(width.saturating_sub(2));
        surface.fill_horizontal_line_with_size(1, 0, count, Character::with_attributes('*', attr));
        if self.has_focus() {
            surface.set_cursor((count + 1).min(width - 1) as i32, 0);
        }
    }
}
impl OnKeyPressed for SecretInput {
    fn on_key_pressed(&mut self, key: Key, character: char) -> EventProcessStatus {
        match key.value() {
            key!("Ctrl+U") => {
                self.clear();
                return EventProcessStatus::Processed;
            }
            key!("Ctrl+A") => {
                self.selected_all = true;
                return EventProcessStatus::Processed;
            }
            key!("Back") => {
                if self.selected_all {
                    self.clear();
                } else {
                    self.value.pop();
                }
                return EventProcessStatus::Processed;
            }
            _ => {}
        }
        if !key
            .modifier
            .contains_one(KeyModifier::Alt | KeyModifier::Ctrl)
            && !character.is_control()
        {
            if self.selected_all {
                self.clear();
            }
            if self.value.len() + character.len_utf8() <= MAX_PASSWORD_BYTES {
                self.value.push(character);
            }
            return EventProcessStatus::Processed;
        }
        EventProcessStatus::Ignored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_input_masks_unicode_and_supports_clearing() {
        let mut input = SecretInput::new(layout!("x:0,y:0,w:20,h:1"));
        for ch in "local 🔒".chars() {
            OnKeyPressed::on_key_pressed(&mut input, Key::None, ch);
        }
        let mut surface = Surface::new(20, 1);
        input.on_paint(&mut surface, &Theme::default());
        let painted: String = (0..20).map(|x| surface.char(x, 0).unwrap().code).collect();
        assert!(painted.contains("*******"));
        assert!(!painted.contains("local"));
        assert!(!painted.contains('🔒'));
        OnKeyPressed::on_key_pressed(&mut input, key!("Back").into(), '\0');
        assert_eq!(input.value.as_str(), "local ");
        OnKeyPressed::on_key_pressed(&mut input, key!("Ctrl+U").into(), '\0');
        assert!(input.value.is_empty());
    }
}
