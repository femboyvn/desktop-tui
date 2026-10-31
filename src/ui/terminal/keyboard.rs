use appcui::input::{Key, KeyModifier};
use appcui::prelude::*;
use async_channel::{Receiver, Sender};
use virtual_terminal::{Input, Output};
use super::input_queue::{InputQueue, InputError};
use crate::runtime::{TerminalParser, TerminalMouseKind, TerminalMouseButton};
use std::cell::RefCell;
use std::rc::Rc;

#[CustomControl(overwrite = OnKeyPressed+OnMouseEvent+OnPaint)]
pub(crate) struct CustomKeyboardControl {
    pub(super) should_exit: bool,
    pub(crate) tx: Sender<Input>,
    pub(crate) rx: Receiver<Output>,
    pub(super) input: InputQueue,
    pub(super) input_error: Option<InputError>,
    parser: Rc<RefCell<TerminalParser>>,
}

impl CustomKeyboardControl {
    pub(super) fn new(tx: Sender<Input>, rx: Receiver<Output>, parser: Rc<RefCell<TerminalParser>>, layout: Layout) -> Self {
        Self {
            should_exit: false,
            base: ControlBase::new(layout, true),
            input: InputQueue::new(tx.clone()), input_error: None,
            tx, rx, parser,
        }
    }
    fn enqueue(&mut self, bytes: Vec<u8>, motion: bool) {
        let result = if motion { self.input.motion(bytes) } else { self.input.data(bytes) };
        if let Err(error) = result { self.input_error = Some(error); }
    }

}

impl OnKeyPressed for CustomKeyboardControl {
    fn on_key_pressed(&mut self, key: Key, character: char) -> EventProcessStatus {
        if !self.has_focus() {
            return EventProcessStatus::Ignored;
        }

        if key.modifier == KeyModifier::Ctrl && key.code == KeyCode::C {
            self.should_exit = true;
        }
        else {
            let application_cursor = self.parser.borrow().application_cursor_keys();
            if let Some(data) = key_bytes(key, character, application_cursor) {
                self.enqueue(data, false);
            }
        }

        EventProcessStatus::Processed
    }

    fn on_paste(&mut self, text: &str) -> EventProcessStatus {
        if !self.has_focus() { return EventProcessStatus::Ignored; }
        let bytes = self.parser.borrow_mut().paste(text);
        self.enqueue(bytes, false);
        EventProcessStatus::Processed
    }
}

impl OnMouseEvent for CustomKeyboardControl {
    fn on_mouse_event(&mut self, event: &MouseEvent) -> EventProcessStatus {
        let bytes = mouse_bytes(&mut self.parser.borrow_mut(), event, self.size());
        if let Some((bytes, motion)) = bytes {
            self.enqueue(bytes, motion);
            EventProcessStatus::Processed
        } else { EventProcessStatus::Ignored }
    }
}

impl OnPaint for CustomKeyboardControl {
    fn on_paint(&self, surface: &mut Surface, _theme: &Theme) {
        if self.has_focus() && self.is_visible() && let Some((x, y)) = self.parser.borrow().cursor() {
            surface.set_cursor(x as i32, y as i32);
        }
    }
}

fn key_bytes(key: Key, character: char, application_cursor: bool) -> Option<Vec<u8>> {
    let mut bytes = to_escape_sequence_vec(key, character)?;
    if application_cursor && key.modifier == KeyModifier::None
        && matches!(key.code, KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End) {
        bytes[1] = b'O';
    }
    Some(bytes)
}

fn mouse_bytes(parser: &mut TerminalParser, event: &MouseEvent, size: Size) -> Option<(Vec<u8>, bool)> {
    use appcui::input::{MouseButton, MouseWheelDirection};
    let button = |button| match button {
        MouseButton::Left => TerminalMouseButton::Left,
        MouseButton::Center => TerminalMouseButton::Middle,
        MouseButton::Right => TerminalMouseButton::Right,
        MouseButton::None => TerminalMouseButton::None,
    };
    let (kind, button, x, y, modifier) = match event {
        MouseEvent::Pressed(data) | MouseEvent::DoubleClick(data) => (TerminalMouseKind::Press, button(data.button), data.x, data.y, data.modifier),
        MouseEvent::Released(data) => (TerminalMouseKind::Release, button(data.button), data.x, data.y, data.modifier),
        MouseEvent::Drag(data) | MouseEvent::OverAt(data) => (TerminalMouseKind::Move, button(data.button), data.x, data.y, data.modifier),
        MouseEvent::WheelAt(data) => {
            let button = match data.direction {
                MouseWheelDirection::Up => TerminalMouseButton::WheelUp,
                MouseWheelDirection::Down => TerminalMouseButton::WheelDown,
                MouseWheelDirection::Left => TerminalMouseButton::WheelLeft,
                MouseWheelDirection::Right => TerminalMouseButton::WheelRight,
            };
            (TerminalMouseKind::Press, button, data.x, data.y, data.modifier)
        }
        _ => return None,
    };
    if size.width == 0 || size.height == 0 { return Some((Vec::new(), false)); }
    let (x, y) = if kind == TerminalMouseKind::Release {
        // Capture keeps delivering the matching release outside the control.
        (x.clamp(0, size.width as i32 - 1), y.clamp(0, size.height as i32 - 1))
    } else { (x, y) };
    if x < 0 || y < 0 || x as u32 >= size.width || y as u32 >= size.height { return Some((Vec::new(), false)); }
    let bytes = parser.mouse_event(kind, x as usize, y as usize, button, modifier);
    Some((bytes, kind == TerminalMouseKind::Move))
}

pub fn to_escape_sequence_vec(key: Key, character: char) -> Option<Vec<u8>> {
    use KeyModifier as KM;

    let mut seq = Vec::new();

    // Calculate modifier parameter for CSI sequences
    // (1 = no modifier, 2=Shift, 3=Alt, 4=Alt+Shift, 5=Ctrl, ...)
    let mut mod_param = 1;
    if key.modifier.contains(KM::Shift) {
        mod_param += 1;
    }
    if key.modifier.contains(KM::Alt) {
        mod_param += 2;
    }
    if key.modifier.contains(KM::Ctrl) {
        mod_param += 4;
    }

    match key.code {
        // ----- Single-byte ASCII -----
        KeyCode::Space => seq.push(if key.modifier.contains(KM::Ctrl) { 0 } else { b' ' }),
        KeyCode::Enter => seq.push(b'\r'),
        KeyCode::Escape => seq.push(0x1B),
        KeyCode::Tab => return match key.modifier.contains(KM::Shift) {
            true => Some(b"\x1B[Z".to_vec()),
            false => Some(vec![b'\t'])
        },
        KeyCode::Backspace => seq.push(0x7F),

        // ----- Arrows -----
        KeyCode::Up => return Some(csi_mod(b"A", mod_param)),
        KeyCode::Down => return Some(csi_mod(b"B", mod_param)),
        KeyCode::Right => return Some(csi_mod(b"C", mod_param)),
        KeyCode::Left => return Some(csi_mod(b"D", mod_param)),

        // ----- Navigation -----
        KeyCode::Home => return Some(csi_mod(b"H", mod_param)),
        KeyCode::End => return Some(csi_mod(b"F", mod_param)),
        KeyCode::PageUp => return Some(csi_mod_tilde(5, mod_param)),
        KeyCode::PageDown => return Some(csi_mod_tilde(6, mod_param)),
        KeyCode::Insert => return Some(csi_mod_tilde(2, mod_param)),
        KeyCode::Delete => return Some(csi_mod_tilde(3, mod_param)),

        // ----- Function keys -----
        KeyCode::F1 => return Some(ss3_or_csi(b"P", 11, mod_param)),
        KeyCode::F2 => return Some(ss3_or_csi(b"Q", 12, mod_param)),
        KeyCode::F3 => return Some(ss3_or_csi(b"R", 13, mod_param)),
        KeyCode::F4 => return Some(ss3_or_csi(b"S", 14, mod_param)),
        KeyCode::F5 => return Some(csi_mod_tilde(15, mod_param)),
        KeyCode::F6 => return Some(csi_mod_tilde(17, mod_param)),
        KeyCode::F7 => return Some(csi_mod_tilde(18, mod_param)),
        KeyCode::F8 => return Some(csi_mod_tilde(19, mod_param)),
        KeyCode::F9 => return Some(csi_mod_tilde(20, mod_param)),
        KeyCode::F10 => return Some(csi_mod_tilde(21, mod_param)),
        KeyCode::F11 => return Some(csi_mod_tilde(23, mod_param)),
        KeyCode::F12 => return Some(csi_mod_tilde(24, mod_param)),

        KeyCode::A | KeyCode::B | KeyCode::C | KeyCode::D | KeyCode::E |
        KeyCode::F | KeyCode::G | KeyCode::H | KeyCode::I | KeyCode::J |
        KeyCode::K | KeyCode::L | KeyCode::M | KeyCode::N | KeyCode::O |
        KeyCode::P | KeyCode::Q | KeyCode::R | KeyCode::S | KeyCode::T |
        KeyCode::U | KeyCode::V | KeyCode::W | KeyCode::X | KeyCode::Y | KeyCode::Z |
        KeyCode::N0 | KeyCode::N1 | KeyCode::N2 | KeyCode::N3 | KeyCode::N4 |
        KeyCode::N5 | KeyCode::N6 | KeyCode::N7 | KeyCode::N8 | KeyCode::N9 | KeyCode::None => {
            let fallback = match key.code {
                KeyCode::A => b'a', KeyCode::B => b'b', KeyCode::C => b'c', KeyCode::D => b'd', KeyCode::E => b'e',
                KeyCode::F => b'f', KeyCode::G => b'g', KeyCode::H => b'h', KeyCode::I => b'i', KeyCode::J => b'j',
                KeyCode::K => b'k', KeyCode::L => b'l', KeyCode::M => b'm', KeyCode::N => b'n', KeyCode::O => b'o',
                KeyCode::P => b'p', KeyCode::Q => b'q', KeyCode::R => b'r', KeyCode::S => b's', KeyCode::T => b't',
                KeyCode::U => b'u', KeyCode::V => b'v', KeyCode::W => b'w', KeyCode::X => b'x', KeyCode::Y => b'y', KeyCode::Z => b'z',
                KeyCode::N0 => b'0', KeyCode::N1 => b'1', KeyCode::N2 => b'2', KeyCode::N3 => b'3', KeyCode::N4 => b'4',
                KeyCode::N5 => b'5', KeyCode::N6 => b'6', KeyCode::N7 => b'7', KeyCode::N8 => b'8', KeyCode::N9 => b'9',
                _ => 0,
            };
            if key.modifier.contains(KM::Ctrl) {
                // Letter key codes remain available when the backend delivers a control character.
                let c = if fallback != 0 { fallback } else if character.is_ascii() { character as u8 } else { return None };
                seq.push(if c == b'?' { 0x7f } else { c & 0x1f });
            } else if character != '\0' && !character.is_control() {
                let mut utf8 = [0; 4];
                seq.extend_from_slice(character.encode_utf8(&mut utf8).as_bytes());
            } else if fallback != 0 {
                seq.push(if key.modifier.contains(KM::Shift) { fallback.to_ascii_uppercase() } else { fallback });
            } else {
                return None;
            }

            if key.modifier.contains(KM::Alt) {
                let mut with_alt = vec![0x1B];
                with_alt.extend_from_slice(&seq);
                return Some(with_alt);
            }

            return Some(seq);
        }
    }

    Some(seq)
}

fn csi_mod(final_byte: &[u8], mod_param: u8) -> Vec<u8> {
    if mod_param == 1 {
        // No modifier
        let mut v = vec![0x1B, b'['];
        v.extend_from_slice(final_byte);
        v
    } else {
        format!("\x1B[1;{}{}", mod_param, std::str::from_utf8(final_byte).unwrap())
            .into_bytes()
    }
}

fn csi_mod_tilde(code: u8, mod_param: u8) -> Vec<u8> {
    if mod_param == 1 {
        format!("\x1B[{}~", code).into_bytes()
    } else {
        format!("\x1B[{};{}~", code, mod_param).into_bytes()
    }
}

fn ss3_or_csi(final_byte: &[u8], base_code: u8, mod_param: u8) -> Vec<u8> {
    if mod_param == 1 {
        // Traditional SS3 sequence
        let mut v = vec![0x1B, b'O'];
        v.extend_from_slice(final_byte);
        v
    } else {
        // CSI form with modifiers
        format!("\x1B[{};{}~", base_code, mod_param).into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printable_input_preserves_unicode_case_and_shifted_punctuation() {
        for (code, modifier, character, expected) in [
            (KeyCode::None, KeyModifier::None, 'ế', "ế"),
            (KeyCode::None, KeyModifier::None, '🦊', "🦊"),
            (KeyCode::A, KeyModifier::None, 'A', "A"),
            (KeyCode::N1, KeyModifier::Shift, '!', "!"),
            (KeyCode::None, KeyModifier::Alt, '界', "\x1b界"),
        ] {
            assert_eq!(to_escape_sequence_vec(Key { code, modifier }, character).unwrap(), expected.as_bytes());
        }
    }

    #[test]
    fn navigation_and_control_shortcuts_preserve_terminal_sequences() {
        for (code, modifier, character, expected) in [
            (KeyCode::Left, KeyModifier::Ctrl, '\0', b"\x1b[1;5D".as_slice()),
            (KeyCode::Delete, KeyModifier::Alt | KeyModifier::Shift, '\0', b"\x1b[3;4~".as_slice()),
            (KeyCode::L, KeyModifier::Ctrl, 'l', b"\x0c".as_slice()),
            (KeyCode::L, KeyModifier::Alt, 'l', b"\x1bl".as_slice()),
            (KeyCode::None, KeyModifier::Ctrl, '[', b"\x1b".as_slice()),
            (KeyCode::Space, KeyModifier::Ctrl, ' ', b"\0".as_slice()),
        ] {
            assert_eq!(to_escape_sequence_vec(Key { code, modifier }, character).unwrap(), expected);
        }
    }

    #[test]
    fn mouse_double_click_keeps_two_complete_clicks_and_release_outside_is_delivered() {
        use appcui::input::{MouseButton, MouseEventData};
        let mut parser = TerminalParser::with_palette(10, 5, &crate::config::preferences::Preferences::default().terminal, None);
        parser.advance_bytes(b"\x1b[?1000h\x1b[?1006h");
        let size = Size::new(10, 5);
        let at = |x, y| MouseEventData { x, y, button: MouseButton::Left, modifier: KeyModifier::Ctrl };
        assert_eq!(mouse_bytes(&mut parser, &MouseEvent::Pressed(at(2, 1)), size).unwrap().0, b"\x1b[<16;3;2M");
        assert_eq!(mouse_bytes(&mut parser, &MouseEvent::Released(at(2, 1)), size).unwrap().0, b"\x1b[<16;3;2m");
        assert_eq!(mouse_bytes(&mut parser, &MouseEvent::DoubleClick(at(2, 1)), size).unwrap().0, b"\x1b[<16;3;2M");
        assert_eq!(mouse_bytes(&mut parser, &MouseEvent::Released(at(20, -1)), size).unwrap().0, b"\x1b[<16;10;1m");
    }

    #[test]
    fn application_cursor_keys_follow_child_modes_without_changing_modified_keys() {
        let key = Key { code: KeyCode::Up, modifier: KeyModifier::None };
        assert_eq!(key_bytes(key, '\0', true).unwrap(), b"\x1bOA");
        assert_eq!(key_bytes(key, '\0', false).unwrap(), b"\x1b[A");
        assert_eq!(key_bytes(Key { modifier: KeyModifier::Ctrl, ..key }, '\0', true).unwrap(), b"\x1b[1;5A");
    }

    #[test]
    fn child_cursor_disappears_when_its_control_loses_focus_or_window_is_hidden() {
        let _lock = crate::test_support::UI_LOCK.lock().unwrap();
        let script = "Paint.Enable(false)\nMouse.Click(3,3,left)\nCheckCursor(4,3)\nMouse.Click(3,7,left)\nCheckCursor(hidden)";
        let mut app = App::debug(40, 16, script).build().unwrap();
        let parser = Rc::new(RefCell::new(TerminalParser::with_palette(10, 4,
            &crate::config::preferences::Preferences::default().terminal, None)));
        parser.borrow_mut().advance_bytes(b"\x1b[2;3H");
        let (tx, _rx) = async_channel::bounded(1);
        let (_output, rx) = async_channel::bounded(1);
        let mut window = Window::new("Terminal", LayoutBuilder::new().x(1).y(1).width(20).height(10).build(), window::Flags::None);
        let terminal = window.add(CustomKeyboardControl::new(tx, rx, parser.clone(),
            LayoutBuilder::new().x(0).y(0).width(10).height(4).build()));
        window.add(Button::new("Other", LayoutBuilder::new().x(0).y(5).width(10).height(1).build()));
        window.request_focus_for_control(terminal);
        app.add_window(window);
        app.run();

        let mut app = App::debug(40, 16, "Paint.Enable(false)\nCheckCursor(hidden)").build().unwrap();
        let (tx, _rx) = async_channel::bounded(1);
        let (_output, rx) = async_channel::bounded(1);
        let mut window = Window::new("Hidden", LayoutBuilder::new().x(1).y(1).width(20).height(10).build(), window::Flags::None);
        window.add(CustomKeyboardControl::new(tx, rx, parser, Layout::fill()));
        window.set_visible(false);
        app.add_window(window);
        app.run();
    }
}
