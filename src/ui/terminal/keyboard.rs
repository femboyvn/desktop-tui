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
    clipboard: Option<crate::runtime::clipboard::Clipboard>,
    status: String,
    dirty: bool,
}

impl CustomKeyboardControl {
    pub(super) fn new(tx: Sender<Input>, rx: Receiver<Output>, parser: Rc<RefCell<TerminalParser>>, layout: Layout) -> Self {
        Self {
            should_exit: false,
            base: ControlBase::new(layout, true),
            input: InputQueue::new(tx.clone()), input_error: None,
            tx, rx, parser, clipboard: None, status: String::new(), dirty: false,
        }
    }
    pub(super) fn enqueue(&mut self, bytes: Vec<u8>, motion: bool) {
        let result = if motion { self.input.motion(bytes) } else { self.input.data(bytes) };
        if let Err(error) = result { self.input_error = Some(error); }
    }

    pub(super) fn terminal_action(&mut self, action: super::TerminalAction) {
        self.status.clear();
        match action {
            super::TerminalAction::CopyMode => self.parser.borrow_mut().start_copy(),
            super::TerminalAction::Search => self.parser.borrow_mut().start_search(),
            super::TerminalAction::Live => self.parser.borrow_mut().live(),
            super::TerminalAction::Paste => {
                if self.parser.borrow().copy_mode() || self.parser.borrow().search_mode() {
                    self.status = "Exit Copy/Search mode before pasting to the child".into();
                } else if let Err(error) = self.clipboard().and_then(|clipboard| clipboard.paste()) { self.status = error; }
                else { self.status = "Reading clipboard…".into(); }
            },
        }
        self.dirty = true;
    }
    fn clipboard(&mut self) -> Result<&crate::runtime::clipboard::Clipboard, String> {
        if self.clipboard.is_none() { self.clipboard = Some(crate::runtime::clipboard::Clipboard::new()?); }
        self.clipboard.as_ref().ok_or_else(|| "Clipboard unavailable".into())
    }
    pub(super) fn close_clipboard(&mut self) { self.clipboard = None; }
    pub(super) fn tick(&mut self) -> bool {
        let result = self.clipboard.as_ref().and_then(|clipboard| clipboard.poll());
        if let Some(result) = result {
            self.status = result.status;
            if let Some(text) = result.text {
                if self.parser.borrow().copy_mode() || self.parser.borrow().search_mode() {
                    self.status = "Paste cancelled because Copy/Search mode was entered".into();
                } else {
                    let bytes = self.parser.borrow().paste(&text);
                    if bytes.len() > 256 * 1024 { self.status = "Paste exceeds the terminal's 256 KiB input limit".into(); }
                    else {
                        // Clipboard admission is fallible without turning a
                        // busy live child into a terminal-wide input failure.
                        match self.input.data(bytes) {
                            Ok(()) => {},
                            Err(InputError::Full) => self.status = "Paste refused: child input queue is busy; try again after it drains".into(),
                            Err(error @ InputError::Closed) => self.input_error = Some(error),
                        }
                    }
                }
            }
            self.dirty = true;
        }
        self.dirty |= self.parser.borrow_mut().search_step();
        std::mem::take(&mut self.dirty)
    }
    pub(super) fn status(&self) -> String {
        let mode = self.parser.borrow().status();
        match (mode.is_empty(), self.status.is_empty()) {
            (true, _) => self.status.clone(), (_, true) => mode, _ => format!("{} | {mode}", self.status),
        }
    }
    fn handle_key(&mut self, key: Key, character: char) {
        self.dirty = true;
        if key.modifier == KeyModifier::Shift && matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) {
            let rows = self.size().height.saturating_sub(1).max(1) as isize;
            self.parser.borrow_mut().scroll(if key.code == KeyCode::PageUp { rows } else { -rows });
            return;
        }
        if self.parser.borrow().search_mode() {
            match key.code {
                KeyCode::Escape => self.parser.borrow_mut().exit_mode(),
                KeyCode::Enter => self.parser.borrow_mut().search_next(key.modifier.contains(KeyModifier::Shift)),
                KeyCode::Backspace => { let mut query = self.parser.borrow().search_query().to_owned(); query.pop(); self.parser.borrow_mut().set_search_query(query); },
                _ if !character.is_control() && character != '\0' && !key.modifier.contains(KeyModifier::Ctrl) && !key.modifier.contains(KeyModifier::Alt) => {
                    let mut query = self.parser.borrow().search_query().to_owned(); query.push(character);
                    self.parser.borrow_mut().set_search_query(query);
                },
                _ => {},
            }
            return;
        }
        if self.parser.borrow().copy_mode() {
            match key.code {
                KeyCode::Escape => self.parser.borrow_mut().exit_mode(),
                KeyCode::Space => self.parser.borrow_mut().mark_selection(),
                KeyCode::Left => self.parser.borrow_mut().move_caret(-1, 0),
                KeyCode::Right => self.parser.borrow_mut().move_caret(1, 0),
                KeyCode::Up => self.parser.borrow_mut().move_caret(0, -1),
                KeyCode::Down => self.parser.borrow_mut().move_caret(0, 1),
                KeyCode::PageUp => self.parser.borrow_mut().move_caret(0, -(self.size().height as isize).max(1)),
                KeyCode::PageDown => self.parser.borrow_mut().move_caret(0, (self.size().height as isize).max(1)),
                KeyCode::Home => self.parser.borrow_mut().move_caret(-(self.size().width as isize), 0),
                KeyCode::End => self.parser.borrow_mut().move_caret(self.size().width as isize, 0),
                KeyCode::Enter => {
                    let selection = self.parser.borrow().selected_text();
                    self.status = match selection {
                        Ok(text) => match self.clipboard().and_then(|clipboard| clipboard.copy(text)) {
                            Ok(()) => "Copying…".into(), Err(error) => error,
                        },
                        Err(error) => error.into(),
                    };
                },
                _ => {},
            }
            return;
        }
        let application_cursor = self.parser.borrow().application_cursor_keys();
        if let Some(data) = key_bytes(key, character, application_cursor) { self.enqueue(data, false); }
    }

    fn handle_mouse(&mut self, event: &MouseEvent, size: Size) -> EventProcessStatus {
        if self.parser.borrow().copy_mode() {
            match event {
                MouseEvent::Pressed(data) | MouseEvent::DoubleClick(data) => {
                    self.parser.borrow_mut().select_at(data.x.max(0) as usize, data.y.max(0) as usize, true);
                },
                MouseEvent::Drag(data) | MouseEvent::Released(data) => {
                    self.parser.borrow_mut().select_at(data.x.max(0) as usize, data.y.max(0) as usize, false);
                },
                MouseEvent::WheelAt(data) => {
                    let rows = if data.direction == appcui::input::MouseWheelDirection::Up { 3 } else { -3 };
                    self.parser.borrow_mut().scroll(rows);
                },
                _ => {},
            }
            self.dirty = true;
            return EventProcessStatus::Processed;
        }
        if self.parser.borrow().search_mode() { return EventProcessStatus::Processed; }
        let bytes = mouse_bytes(&mut self.parser.borrow_mut(), event, size);
        if let Some((bytes, motion)) = bytes {
            self.enqueue(bytes, motion);
            EventProcessStatus::Processed
        } else { EventProcessStatus::Ignored }
    }
}

impl OnKeyPressed for CustomKeyboardControl {
    fn on_key_pressed(&mut self, key: Key, character: char) -> EventProcessStatus {
        if !self.has_focus() {
            return EventProcessStatus::Ignored;
        }

        self.handle_key(key, character);

        EventProcessStatus::Processed
    }

    fn on_paste(&mut self, text: &str) -> EventProcessStatus {
        if !self.has_focus() { return EventProcessStatus::Ignored; }
        if self.parser.borrow().search_mode() {
            let mut query = self.parser.borrow().search_query().to_owned();
            let remaining = 256usize.saturating_sub(query.chars().count());
            query.extend(text.chars().take(remaining));
            self.parser.borrow_mut().set_search_query(query);
            self.dirty = true;
            return EventProcessStatus::Processed;
        }
        if self.parser.borrow().copy_mode() { return EventProcessStatus::Processed; }
        let bytes = self.parser.borrow_mut().paste(text);
        self.enqueue(bytes, false);
        EventProcessStatus::Processed
    }
}

impl OnMouseEvent for CustomKeyboardControl {
    fn on_mouse_event(&mut self, event: &MouseEvent) -> EventProcessStatus { self.handle_mouse(event, self.size()) }
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
    fn copy_outcome_precedes_instructions_and_new_modes_clear_stale_feedback() {
        use crate::runtime::clipboard::{Clipboard,ClipboardResult};
        let _lock = crate::test_support::UI_LOCK.lock().unwrap();
        let app = App::debug(40,12,"Paint.Enable(false)").build().unwrap();
        let parser = Rc::new(RefCell::new(TerminalParser::with_palette(10,4,
            &crate::config::preferences::Preferences::default().terminal,None)));
        let (tx,_input) = async_channel::bounded(1); let (_output,rx) = async_channel::bounded(1);
        let mut control = CustomKeyboardControl::new(tx,rx,parser,Layout::fill());
        control.terminal_action(super::super::TerminalAction::CopyMode);
        control.clipboard = Some(Clipboard::with_result(ClipboardResult {
            text: None,status: "Copied to internal clipboard (system clipboard unavailable)".into(),
        }));
        control.tick();
        let copied = control.status();
        control.terminal_action(super::super::TerminalAction::Search);
        let search = control.status();
        control.terminal_action(super::super::TerminalAction::Live);
        let live = control.status();
        app.run();
        assert!(copied.starts_with("Copied to internal clipboard"),"{copied}");
        assert!(copied.contains("Copy"),"mode remains visible after the outcome");
        assert!(search.starts_with("Search"),"{search}");
        assert!(!search.contains("Copied"));
        assert!(!live.contains("Copied"));
    }

    #[test]
    fn review_clipboard_backpressure_refusal_preserves_earlier_input_and_child() {
        use crate::runtime::clipboard::{Clipboard,ClipboardResult};
        let _lock = crate::test_support::UI_LOCK.lock().unwrap();
        let app = App::debug(40,12,"Paint.Enable(false)").build().unwrap();
        let parser = Rc::new(RefCell::new(TerminalParser::with_palette(10,4,
            &crate::config::preferences::Preferences::default().terminal,None)));
        let (tx,input) = async_channel::bounded(1); let (_output,rx) = async_channel::bounded(1);
        tx.try_send(Input::Data(b"busy".to_vec())).unwrap();
        let mut control = CustomKeyboardControl::new(tx,rx,parser,Layout::fill());
        control.enqueue(vec![b'a';128*1024],false);
        control.clipboard = Some(Clipboard::with_result(ClipboardResult { text: Some("b".repeat(200*1024)),status: "System paste".into() }));
        assert!(control.tick());
        let error = control.input_error;
        let status = control.status.clone();
        let child_open = !control.tx.is_closed() && !control.should_exit;
        assert_eq!(input.try_recv().unwrap(),Input::Data(b"busy".to_vec()));
        let mut retained = Vec::new();
        loop {
            control.input.flush().unwrap();
            let Ok(Input::Data(bytes)) = input.try_recv() else { break; };
            retained.extend(bytes);
        }
        input.close();
        control.clipboard = Some(Clipboard::with_result(ClipboardResult { text: Some("closed".into()),status: "System paste".into() }));
        control.tick();
        let closed_error = control.input_error;
        app.run();
        assert_eq!(closed_error,Some(InputError::Closed),"a disconnected child still uses the existing cleanup path");
        assert_eq!(error,None,"clipboard refusal must not become a fatal child input error");
        assert!(child_open);
        assert!(status.contains("Paste refused"),"{status}");
        assert_eq!(retained,vec![b'a';128*1024],"existing input survives in order; no partial paste");
    }
    #[test]
    fn explicit_copy_mode_owns_selection_and_restores_child_mouse_reporting() {
        use appcui::input::{MouseButton, MouseEventData};
        let _lock = crate::test_support::UI_LOCK.lock().unwrap();
        let app = App::debug(40, 12, "Paint.Enable(false)").build().unwrap();
        let parser = Rc::new(RefCell::new(TerminalParser::with_palette(10, 4,
            &crate::config::preferences::Preferences::default().terminal, None)));
        parser.borrow_mut().advance_bytes("A中e\u{301}\x1b[?1000h\x1b[?1006h".as_bytes());
        let (tx, input) = async_channel::bounded(8); let (_output, rx) = async_channel::bounded(1);
        let mut control = CustomKeyboardControl::new(tx,rx,parser.clone(),Layout::fill());
        let at = |x,y| MouseEventData { x,y,button: MouseButton::Left,modifier: KeyModifier::None };
        control.handle_mouse(&MouseEvent::Pressed(at(2,1)),Size::new(10,4));
        assert!(matches!(input.try_recv().unwrap(),Input::Data(bytes) if bytes == b"\x1b[<0;3;2M"));
        parser.borrow_mut().start_copy();
        control.handle_mouse(&MouseEvent::Pressed(at(2,0)),Size::new(10,4));
        control.handle_mouse(&MouseEvent::Drag(at(3,0)),Size::new(10,4));
        assert_eq!(parser.borrow().selected_text().unwrap(),"中e\u{301}");
        assert!(input.is_empty());
        control.handle_key(Key { code: KeyCode::Escape,modifier: KeyModifier::None },'\0');
        control.handle_mouse(&MouseEvent::Pressed(at(2,1)),Size::new(10,4));
        assert!(matches!(input.try_recv().unwrap(),Input::Data(bytes) if bytes == b"\x1b[<0;3;2M"));
        app.run();
    }
    #[test]
    fn ctrl_c_queues_interrupt_without_requesting_window_close() {
        let _lock = crate::test_support::UI_LOCK.lock().unwrap();
        let app = App::debug(40, 12, "Paint.Enable(false)").build().unwrap();
        let parser = Rc::new(RefCell::new(TerminalParser::with_palette(10, 4,
            &crate::config::preferences::Preferences::default().terminal, None)));
        let (tx, input) = async_channel::bounded(1); let (_output, rx) = async_channel::bounded(1);
        let mut control = CustomKeyboardControl::new(tx, rx, parser.clone(), Layout::fill());
        control.handle_key(Key { code: KeyCode::C, modifier: KeyModifier::Ctrl }, '\0');
        assert!(!control.should_exit);
        assert!(matches!(input.try_recv().unwrap(), Input::Data(bytes) if bytes == [3]));
        parser.borrow_mut().start_copy();
        control.handle_key(Key { code: KeyCode::Escape, modifier: KeyModifier::None }, '\0');
        assert!(!parser.borrow().copy_mode());
        assert!(input.is_empty());
        app.run();
    }
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
