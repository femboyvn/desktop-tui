use super::image_viewer::home_directory;
use crate::runtime::files::{self, Entry, FileKind, Listing};
use crate::runtime::latest::LatestWorker;
use appcui::prelude::*;
use std::path::PathBuf;
use std::collections::VecDeque;
use std::time::Duration;

impl listview::ListItem for Entry {
    fn columns_count() -> u16 { 4 }
    fn column(index: u16) -> Column {
        let (name, width) = match index { 0 => ("Name", 30), 1 => ("Type", 12), 2 => ("Bytes", 12), _ => ("Modified", 18) };
        Column::new(name, width, TextAlignment::Left)
    }
    fn render_method(&self, index: u16) -> Option<listview::RenderMethod<'_>> {
        Some(listview::RenderMethod::Text(match index {
            0 => &self.name,
            1 => match self.kind { FileKind::Directory => "Folder", FileKind::File => "File", FileKind::Other => "Other" },
            2 => &self.size,
            _ => &self.modified,
        }))
    }
}

#[Window(events = TimerEvents+WindowEvents+TextFieldEvents+ButtonEvents+ListViewEvents<Entry>)]
pub(crate) struct BrowserWindow {
    path: PathBuf,
    path_field: Handle<TextField>,
    listing: Handle<ListView<Entry>>,
    up: Handle<Button>,
    refresh: Handle<Button>,
    hidden_button: Handle<Button>,
    hidden: bool,
    closing: bool,
    worker: LatestWorker<(PathBuf, bool), Result<Listing, String>>,
    open_path: Option<PathBuf>,
    pending_entries: VecDeque<Entry>,
}
impl BrowserWindow {
    pub fn new() -> anyhow::Result<Self> { Self::at(home_directory()) }
    fn at(path: PathBuf) -> anyhow::Result<Self> {
        let worker = LatestWorker::new(|(path, hidden): (PathBuf, bool)| files::list_directory(&path, hidden).map_err(|e| format!("{e:#}")))?;
        let mut window = Self {
            base: Window::new("File Browser — Enter Open | Alt+U Parent | Alt+R Refresh | Alt+H Hidden", layout!("a:c,w:90%,h:80%"), window::Flags::Sizeable),
            path, path_field: Handle::None, listing: Handle::None, up: Handle::None, refresh: Handle::None, hidden_button: Handle::None,
            hidden: false, closing: false, worker, open_path: None, pending_entries: VecDeque::new(),
        };
        let path_text = window.path.to_string_lossy().into_owned();
        window.path_field = window.add(TextField::new(&path_text, layout!("l:0,r:0,t:0,h:1"), textfield::Flags::None));
        window.up = window.add(Button::with_type("&Up", layout!("x:0,y:1,w:10"), button::Type::Flat));
        window.refresh = window.add(Button::with_type("&Refresh", layout!("x:11,y:1,w:12"), button::Type::Flat));
        window.hidden_button = window.add(Button::with_type("&Hidden: off", layout!("x:24,y:1,w:16"), button::Type::Flat));
        window.listing = window.add(ListView::<Entry>::new(layout!("l:0,r:0,t:3,b:0"), listview::Flags::ScrollBars));
        // Keep directories first: user sorting is deliberately disabled for this view.
        window.timer().ok_or_else(|| anyhow::anyhow!("File Browser timer unavailable"))?.start(Duration::from_millis(40));
        window.navigate(window.path.clone());
        Ok(window)
    }
    fn navigate(&mut self, path: PathBuf) {
        let target = if path.is_absolute() { path } else { self.path.join(path) };
        self.pending_entries.clear();
        self.worker.submit((target, self.hidden));
    }
    fn parent(&mut self) { if let Some(parent) = self.path.parent() { self.navigate(parent.to_owned()); } }
    fn refresh(&mut self) { self.navigate(self.path.clone()); }
    fn toggle_hidden(&mut self) {
        self.hidden = !self.hidden;
        let button = self.hidden_button;
        let hidden = self.hidden;
        if let Some(button) = self.control_mut(button) { button.set_caption(if hidden { "&Hidden: on" } else { "&Hidden: off" }); }
        self.refresh();
    }
    pub fn take_open_path(&mut self) -> Option<PathBuf> { self.open_path.take() }
    pub fn is_closing(&self) -> bool { self.closing }
    pub fn shutdown(&mut self) { self.closing = true; self.worker.close(); self.close(); }
}
impl TextFieldEvents for BrowserWindow {
    fn on_validate(&mut self, _: Handle<TextField>, text: &str) -> EventProcessStatus {
        // Unedited display text can be lossy for non-UTF-8 directories; retain the original OS path.
        let path = if text == self.path.to_string_lossy() { self.path.clone() } else { PathBuf::from(text) };
        self.navigate(path);
        EventProcessStatus::Processed
    }
}
impl ButtonEvents for BrowserWindow {
    fn on_pressed(&mut self, handle: Handle<Button>) -> EventProcessStatus {
        if handle == self.up { self.parent(); }
        else if handle == self.refresh { self.refresh(); }
        else if handle == self.hidden_button { self.toggle_hidden(); }
        else { return EventProcessStatus::Ignored; }
        EventProcessStatus::Processed
    }
}
impl ListViewEvents<Entry> for BrowserWindow {
    fn on_item_action(&mut self, handle: Handle<ListView<Entry>>, index: usize) -> EventProcessStatus {
        if let Some(entry) = self.control(handle).and_then(|list| list.item(index)).cloned() {
            match entry.kind {
                FileKind::Directory => self.navigate(entry.path),
                FileKind::File => self.open_path = Some(entry.path),
                FileKind::Other => dialogs::error("Cannot open entry", "Choose a directory or regular file."),
            }
        }
        EventProcessStatus::Processed
    }
}
impl TimerEvents for BrowserWindow {
    fn on_update(&mut self, _: u64) -> EventProcessStatus {
        if self.closing || !self.is_visible() { return EventProcessStatus::Ignored; }
        let mut changed = false;
        match self.worker.poll() {
            Some(Ok(listing)) => {
                self.path = listing.path;
                let field = self.path_field;
                let text = self.path.to_string_lossy().into_owned();
                if let Some(field) = self.control_mut(field) { field.set_text(&text); }
                let handle = self.listing;
                if let Some(list) = self.control_mut(handle) { list.clear(); list.request_focus(); }
                self.pending_entries = listing.entries.into();
                changed = true;
            }
            Some(Err(error)) => {
                dialogs::error("Cannot read directory", &format!("{error}\nUse the path field, Up, or Refresh to try again."));
                changed = true;
            }
            None => {}
        }
        if !self.pending_entries.is_empty() {
            // Large directories are inserted incrementally to keep callbacks short.
            let batch = self.pending_entries.drain(..self.pending_entries.len().min(256)).collect();
            let handle = self.listing;
            if let Some(list) = self.control_mut(handle) { list.add_items(batch); }
            changed = true;
        }
        if changed { EventProcessStatus::Processed } else { EventProcessStatus::Ignored }
    }
}
impl WindowEvents for BrowserWindow {
    fn on_cancel(&mut self) -> ActionRequest { self.closing = true; self.worker.close(); ActionRequest::Allow }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestDir, UI_LOCK};
    fn pump_until(window: &mut BrowserWindow, ready: impl Fn(&BrowserWindow) -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !ready(window) {
            TimerEvents::on_update(window, 0);
            assert!(std::time::Instant::now() < deadline, "directory result did not arrive");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn hidden_browser_defers_failed_results_without_opening_a_dialog() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut window = BrowserWindow::at(dir.path().join("missing secret path")).unwrap();
        window.set_visible(false);
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            assert!(matches!(TimerEvents::on_update(&mut window, 0), EventProcessStatus::Ignored));
            if let Some(result) = window.worker.poll() { assert!(result.is_err()); break; }
            assert!(std::time::Instant::now() < deadline, "hidden window consumed its pending result");
            std::thread::sleep(Duration::from_millis(2));
        }
        window.shutdown();
        app.run();
    }
    #[test]
    fn navigates_parent_refreshes_toggles_hidden_and_emits_original_file_paths() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let child = dir.path().join("child 文 folder");
        std::fs::create_dir(&child).unwrap();
        std::fs::write(dir.path().join("image.PNG"), b"image").unwrap();
        std::fs::write(dir.path().join(".hidden"), b"hidden").unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut window = BrowserWindow::at(dir.path().to_owned()).unwrap();
        pump_until(&mut window, |window| window.control(window.listing).unwrap().items_count() == 2);
        let listing = window.listing;
        ListViewEvents::<Entry>::on_item_action(&mut window, listing, 0);
        pump_until(&mut window, |window| window.path == child);
        window.parent();
        pump_until(&mut window, |window| window.path == dir.path());
        window.toggle_hidden();
        pump_until(&mut window, |window| window.control(window.listing).unwrap().items_count() == 3);
        std::fs::write(dir.path().join("new.txt"), b"new").unwrap();
        window.refresh();
        pump_until(&mut window, |window| window.control(window.listing).unwrap().items_count() == 4);
        let list = window.control(window.listing).unwrap();
        let image_index = (0..list.items_count()).find(|index| list.item(*index).unwrap().name == "image.PNG").unwrap();
        ListViewEvents::<Entry>::on_item_action(&mut window, listing, image_index);
        assert_eq!(window.take_open_path(), Some(dir.path().join("image.PNG")));
        window.shutdown();
        app.run();
    }
}
