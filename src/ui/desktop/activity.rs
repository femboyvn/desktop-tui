use crate::runtime::activity::{ActivityLog, Entry};
use appcui::prelude::*;
use std::time::Duration;

#[ModalWindow(response = bool, events = ButtonEvents+TimerEvents+ListBoxEvents)]
pub(super) struct ActivityDialog {
    activity: ActivityLog,
    revision: Option<u64>,
    rows: Vec<Entry>,
    selected: Option<u64>,
    entries: Handle<ListBox>,
    detail: Handle<TextArea>,
    clear: Handle<Button>,
    close: Handle<Button>,
}

impl ActivityDialog {
    pub fn new(activity: ActivityLog) -> Self {
        let mut dialog = Self {
            base: ModalWindow::new("Activity",layout!("a:c,w:70,h:18"),window::Flags::Sizeable),
            activity,revision: None,rows: Vec::new(),selected: None,entries: Handle::None,detail: Handle::None,clear: Handle::None,close: Handle::None,
        };
        let mut entries = ListBox::new(layout!("l:1,r:1,t:0,b:7"),listbox::Flags::ScrollBars);
        entries.set_empty_message("No recent activity.");
        dialog.entries = dialog.add(entries);
        dialog.detail = dialog.add(TextArea::new("\n",layout!("l:1,r:1,b:2,h:4"),textarea::Flags::ReadOnly | textarea::Flags::ScrollBars));
        dialog.clear = dialog.add(Button::new("C&lear",layout!("l:1,b:0,w:12")));
        dialog.close = dialog.add(Button::new("&Close",layout!("r:1,b:0,w:12")));
        dialog.base.request_focus_for_control(dialog.entries);
        dialog.refresh();
        if let Some(timer) = dialog.timer() { timer.start(Duration::from_millis(100)); }
        dialog
    }

    fn refresh(&mut self) -> bool {
        let Some((revision,entries)) = self.activity.read_since(self.revision) else { return false; };
        self.revision = Some(revision);
        let index = entries.iter().position(|entry| Some(entry.id) == self.selected).unwrap_or(0);
        let labels: Vec<_> = entries.iter().map(|entry| {
            let time = chrono::DateTime::<chrono::Local>::from(entry.time).format("%b %d %H:%M:%S");
            format!("{time}  {}  {}",entry.source.label(),entry.status.label())
        }).collect();
        self.rows = entries;
        let handle = self.entries;
        if let Some(list) = self.control_mut(handle) {
            list.clear();
            for label in &labels { list.add(label); }
            if !labels.is_empty() { list.set_index(index); }
        }
        self.show_details(index);
        let handle = self.clear;
        if let Some(button) = self.control_mut(handle) { button.set_enabled(!labels.is_empty()); }
        true
    }

    fn show_details(&mut self, index: usize) -> bool {
        let entry = self.rows.get(index);
        let selected = entry.map(|entry| entry.id);
        if self.selected == selected { return false; }
        self.selected = selected;
        let text = entry.map(|entry| entry.detail.clone()).filter(|text| !text.is_empty()).unwrap_or_else(|| "\n".into());
        let handle = self.detail;
        if let Some(view) = self.control_mut(handle) { view.set_text(&text); }
        true
    }
}

impl ListBoxEvents for ActivityDialog {
    fn on_current_item_changed(&mut self, handle: Handle<ListBox>, index: usize) -> EventProcessStatus {
        if handle == self.entries && self.show_details(index) { EventProcessStatus::Processed } else { EventProcessStatus::Ignored }
    }
}

impl ButtonEvents for ActivityDialog {
    fn on_pressed(&mut self, handle: Handle<Button>) -> EventProcessStatus {
        if handle == self.clear { self.activity.clear(); self.refresh(); }
        else if handle == self.close { self.exit_with(true); }
        else { return EventProcessStatus::Ignored; }
        EventProcessStatus::Processed
    }
}

impl TimerEvents for ActivityDialog {
    fn on_update(&mut self, _: u64) -> EventProcessStatus {
        if self.refresh() { EventProcessStatus::Processed } else { EventProcessStatus::Ignored }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::activity::{Source, Status};

    #[test]
    fn activity_view_marks_entries_read_updates_and_clears_history() {
        let _lock = crate::test_support::UI_LOCK.lock().unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)").timers_count(8).build().unwrap();
        let activity = ActivityLog::default();
        activity.record(Source::Files,Status::Completed,"Copied to fixture");
        let mut dialog = ActivityDialog::new(activity.clone());
        assert_eq!(activity.unread(),0);
        assert_eq!(dialog.rows[0].source,Source::Files);
        assert!(dialog.control(dialog.detail).unwrap().text().contains("Copied to fixture"));
        activity.record(Source::AiChat,Status::Failed,"Response failed.");
        assert!(TimerEvents::on_update(&mut dialog,0) == EventProcessStatus::Processed);
        assert_eq!(dialog.rows[0].status,Status::Failed);
        let list = dialog.entries;
        assert_eq!(dialog.control(list).unwrap().index(),1);
        assert!(dialog.control(dialog.detail).unwrap().text().contains("Copied to fixture"));
        ListBoxEvents::on_current_item_changed(&mut dialog,list,0);
        assert!(dialog.control(dialog.detail).unwrap().text().contains("Response failed."));
        let clear = dialog.clear;
        ButtonEvents::on_pressed(&mut dialog,clear);
        assert!(dialog.rows.is_empty());
        assert!(activity.read_since(None).unwrap().1.is_empty());
        drop(dialog); app.run();
    }
}
