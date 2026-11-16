use appcui::prelude::*;
use crate::config::workflow::PaletteShortcut;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum PaletteResult { Selected(usize), ForwardPrefix }

#[ModalWindow(response = PaletteResult, events = WindowEvents)]
pub(super) struct CommandPalette {
    labels: Vec<String>,
    query: String,
    selected: usize,
    prefix: Key,
    display: Handle<TextArea>,
}
impl CommandPalette {
    pub fn new(labels: Vec<String>, prefix: Key) -> Self {
        let mut palette = Self { base: ModalWindow::new("Desktop commands", layout!("a:c,w:74,h:20"), window::Flags::Sizeable),
            labels, query: String::new(), selected: 0, prefix, display: Handle::None };
        palette.display = palette.add(TextArea::new("Type to search; Enter selects; Escape cancels.\n",layout!("l:1,r:1,t:0,b:0"),textarea::Flags::ReadOnly));
        palette.refresh(palette.size().height);
        palette
    }
    fn matches(&self) -> Vec<usize> {
        let words = self.query.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>();
        self.labels.iter().enumerate().filter_map(|(index,label)| {
            let label = label.to_lowercase();
            words.iter().all(|word| label.contains(word)).then_some(index)
        }).collect()
    }
    fn refresh(&mut self, height: u32) {
        let matches = self.matches();
        self.selected = self.selected.min(matches.len().saturating_sub(1));
        let rows = height.saturating_sub(6).max(1) as usize;
        let start = self.selected.saturating_sub(rows.saturating_sub(1));
        let mut text = format!("Search: {}_\nEnter selects | Escape cancels | {}{} forwards\n\n",self.query,self.prefix.modifier.name(),self.prefix.code.name());
        for (row,index) in matches.iter().enumerate().skip(start).take(rows) {
            let label = self.labels[*index].chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>();
            text.push_str(&format!("{} {label}\n",if row == self.selected { ">" } else { " " }));
        }
        if matches.is_empty() { text.push_str("No matching commands.\n"); }
        let display = self.display;
        if let Some(display) = self.control_mut(display) { display.set_text(&text); }
    }
}
impl WindowEvents for CommandPalette {
    fn on_layout_changed(&mut self, _: Rect, layout: Rect) { self.refresh(layout.height()); }
    fn on_activate(&mut self) { self.refresh(self.size().height); }
    fn on_preview_key_pressed(&mut self, key: Key, character: char) -> EventProcessStatus {
        if key == self.prefix { self.exit_with(PaletteResult::ForwardPrefix); return EventProcessStatus::Processed; }
        match key.code {
            KeyCode::Escape => self.exit(),
            KeyCode::Enter => if let Some(index) = self.matches().get(self.selected).copied() { self.exit_with(PaletteResult::Selected(index)); },
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => self.selected = self.selected.saturating_add(1),
            KeyCode::PageUp => self.selected = self.selected.saturating_sub(10),
            KeyCode::PageDown => self.selected = self.selected.saturating_add(10),
            KeyCode::Home => self.selected = 0,
            KeyCode::End => self.selected = self.matches().len().saturating_sub(1),
            KeyCode::Backspace => { self.query.pop(); self.selected = 0; },
            _ if character >= ' ' && !character.is_control() && !key.modifier.contains(KeyModifier::Ctrl) && !key.modifier.contains(KeyModifier::Alt) && self.query.len()+character.len_utf8() <= 512 => {
                self.query.push(character); self.selected = 0;
            }
            _ => {},
        }
        self.refresh(self.size().height);
        EventProcessStatus::Processed
    }
}

#[ModalWindow(response = PaletteShortcut, events = ButtonEvents+WindowEvents)]
pub(super) struct ShortcutDialog { selector: Handle<KeySelector>, save: Handle<Button> }
impl ShortcutDialog {
    pub fn new(current: Key) -> Self {
        let mut dialog = Self { base: ModalWindow::new("Desktop command shortcut",layout!("a:c,w:58,h:10"),window::Flags::None),selector: Handle::None,save: Handle::None };
        dialog.add(Label::new("Press a modified key or function key.\nEnter saves; Escape cancels.",layout!("l:1,r:1,t:0,h:2")));
        dialog.selector = dialog.add(KeySelector::new(current,layout!("l:2,r:2,t:3,h:1"),keyselector::Flags::None));
        dialog.save = dialog.add(Button::new("&Save",layout!("a:b,w:12")));
        dialog.base.request_focus_for_control(dialog.selector);
        dialog
    }
    fn save_binding(&mut self) {
        if let Some(key) = self.control(self.selector).map(KeySelector::key) {
            if let Some(binding) = crate::config::workflow::PaletteShortcut::from_key(key) { self.exit_with(binding); }
            else { dialogs::error("Invalid desktop shortcut","Choose a modified key or function key. Ctrl+C remains a child interrupt."); }
        }
    }
}
impl WindowEvents for ShortcutDialog { fn on_accept(&mut self) { self.save_binding(); } }
impl ButtonEvents for ShortcutDialog { fn on_pressed(&mut self, _: Handle<Button>) -> EventProcessStatus { self.save_binding(); EventProcessStatus::Processed } }

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc,Mutex};
    #[test]
    fn palette_initial_layout_shows_commands_without_waiting_for_a_key() {
        let _lock = crate::test_support::UI_LOCK.lock().unwrap();
        let app = App::debug(80,24,"").build().unwrap();
        let mut palette = CommandPalette::new(vec!["New terminal".into(),"Launch: Other app".into()],key!("Ctrl+Space").into());
        WindowEvents::on_layout_changed(&mut palette,Rect::new(0,0,0,0),Rect::new(0,0,73,19));
        assert!(palette.control(palette.display).unwrap().text().contains("Launch: Other app"));
        app.run();
    }
    #[CustomControl(overwrite = OnKeyPressed)]
    struct KeyConsumer { log: Arc<Mutex<Vec<&'static str>>> }
    impl OnKeyPressed for KeyConsumer {
        fn on_key_pressed(&mut self, key: Key, character: char) -> EventProcessStatus {
            if key.value() == key!("Ctrl+Space") { self.log.lock().unwrap().push("leaked prefix"); }
            else if character == 'a' { self.log.lock().unwrap().push("ordinary key"); }
            EventProcessStatus::Processed
        }
    }
    #[Desktop(events = DesktopEvents)]
    struct PreviewDesktop { log: Arc<Mutex<Vec<&'static str>>>, modal: bool }
    impl DesktopEvents for PreviewDesktop {
        fn on_start(&mut self) {
            if self.modal {
                let result = CommandPalette::new(vec!["One".into()],key!("Ctrl+Space").into()).show();
                assert_eq!(result,Some(PaletteResult::ForwardPrefix)); self.log.lock().unwrap().push("modal prefix");
            } else {
                let mut window = Window::new("Consumes keys",layout!("a:c,w:30,h:10"),window::Flags::None);
                let control = window.add(KeyConsumer { base: ControlBase::new(Layout::fill(),true),log: self.log.clone() });
                window.request_focus_for_control(control); self.add_window(window);
            }
        }
        fn on_preview_key_pressed(&mut self, key: Key, _: char) -> EventProcessStatus {
            if key.value() != key!("Ctrl+Space") { return EventProcessStatus::Ignored; }
            self.log.lock().unwrap().push("desktop prefix"); EventProcessStatus::Processed
        }
    }
    #[test]
    fn desktop_preview_precedes_consuming_children_and_respects_modal_input() {
        let _lock = crate::test_support::UI_LOCK.lock().unwrap();
        for (modal,script,expected) in [
            (false,"Key.Pressed(Ctrl+Space)\nKey.TypeText('a')",vec!["desktop prefix","ordinary key"]),
            (true,"Key.Pressed(Ctrl+Space)",vec!["modal prefix"]),
        ] {
            let log = Arc::new(Mutex::new(Vec::new()));
            App::debug(80,24,script).desktop(PreviewDesktop { base: Desktop::new(),log: log.clone(),modal }).build().unwrap().run();
            assert_eq!(*log.lock().unwrap(),expected);
        }
    }
    #[test]
    fn palette_search_keeps_action_indices_and_matches_each_word() {
        let _lock = crate::test_support::UI_LOCK.lock().unwrap();
        let app = App::debug(80,24,"").build().unwrap();
        let mut palette = CommandPalette::new(vec!["Launch: Text editor".into(), "Focus: Terminal (hidden)".into(), "Close: Terminal".into()], key!("Ctrl+Space").into());
        palette.query = "terminal focus".into();
        assert_eq!(palette.matches(), [1]);
        palette.query = "EDITOR".into();
        assert_eq!(palette.matches(), [0]);
        app.run();
    }
    #[test]
    fn palette_keyboard_search_cancel_and_prefix_work_in_modal() {
        let _lock = crate::test_support::UI_LOCK.lock().unwrap();
        for (script, expected) in [
            ("Key.TypeText('terminal')\nKey.Pressed(Enter)", Some(PaletteResult::Selected(1))),
            ("Key.Pressed(Escape)", None),
            ("Key.Pressed(Ctrl+Space)", Some(PaletteResult::ForwardPrefix)),
        ] {
            let app = App::debug(80,24,script).build().unwrap();
            assert_eq!(CommandPalette::new(vec!["Launch: Editor".into(), "Focus: Terminal".into()], key!("Ctrl+Space").into()).show(), expected);
            app.run();
        }
    }
}
