use crate::config::{Shortcut, catalog::AppId};
use crate::runtime::files;
use appcui::prelude::*;
use std::path::Path;

struct OpenChoice { id: AppId, label: String }
impl listview::ListItem for OpenChoice {
    fn columns_count() -> u16 { 1 }
    fn column(_: u16) -> Column { Column::new("Application",64,TextAlignment::Left) }
    fn render_method(&self, _: u16) -> Option<listview::RenderMethod<'_>> { Some(listview::RenderMethod::Text(&self.label)) }
}
fn choices(path: &Path, shortcuts: &[Shortcut]) -> Vec<OpenChoice> {
    let native = if files::is_image(path) { Some((AppId::ImageViewer,"Image Viewer")) }
        else if files::is_pdf(path) { Some((AppId::PdfReader,"PDF Reader")) }
        else if files::is_audio(path) { Some((AppId::MusicPlayer,"Music Player")) }
        else if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("asciipaint")) { Some((AppId::AsciiPaint,"ASCII Paint")) }
        else { None };
    let mut result = Vec::new();
    if let Some((id,name)) = native { result.push(OpenChoice { id, label: format!("{name} (native)") }); }
    result.extend(shortcuts.iter().enumerate().map(|(index,shortcut)| OpenChoice {
        id: AppId::External(index), label: format!("{} (shortcut)",shortcut.name),
    }));
    result
}
#[ModalWindow(response = AppId, events = ListViewEvents<OpenChoice>+ButtonEvents)]
pub(super) struct OpenWithDialog { listing: Handle<ListView<OpenChoice>>, cancel: Handle<Button> }
impl OpenWithDialog {
    pub fn new(path: &Path, shortcuts: &[Shortcut]) -> Self {
        let mut dialog = Self {
            base: ModalWindow::new("Open with — select an application",layout!("a:c,w:74,h:20"),window::Flags::Sizeable),
            listing: Handle::None, cancel: Handle::None,
        };
        dialog.add(Label::new(&path.file_name().unwrap_or(path.as_os_str()).to_string_lossy(),layout!("l:1,r:1,t:0,h:1")));
        let mut listing = ListView::new(layout!("l:1,r:1,t:2,b:3"),listview::Flags::ScrollBars);
        listing.add_items(choices(path,shortcuts));
        dialog.listing = dialog.add(listing);
        dialog.cancel = dialog.add(Button::new("&Cancel",layout!("a:b,w:14")));
        dialog.base.request_focus_for_control(dialog.listing);
        dialog
    }
}
impl ListViewEvents<OpenChoice> for OpenWithDialog {
    fn on_item_action(&mut self, handle: Handle<ListView<OpenChoice>>, index: usize) -> EventProcessStatus {
        if let Some(id) = self.control(handle).and_then(|list| list.item(index)).map(|choice| choice.id) { self.exit_with(id); }
        EventProcessStatus::Processed
    }
}
impl ButtonEvents for OpenWithDialog {
    fn on_pressed(&mut self, _: Handle<Button>) -> EventProcessStatus { self.exit(); EventProcessStatus::Processed }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::catalog;
    use crate::test_support::UI_LOCK;
    #[test]
    fn choices_filter_native_viewers_but_include_every_external_with_stable_ids() {
        let mut shortcuts = catalog::with_starters(vec![]);
        shortcuts[0].name = "Image Viewer".into();
        let image = choices(Path::new("/tmp/文.PNG"),&shortcuts);
        assert_eq!(image.len(),4);
        assert_eq!(image[0].id,AppId::ImageViewer);
        assert_eq!(image[1].id,AppId::External(0));
        assert_ne!(image[0].label,image[1].label);
        for (path,native) in [("a.PDF",Some(AppId::PdfReader)),("a.ogg",Some(AppId::MusicPlayer)),("a.asciipaint",Some(AppId::AsciiPaint)),("a.txt",None)] {
            let values = choices(Path::new(path),&shortcuts);
            assert_eq!(values.len(),shortcuts.len()+usize::from(native.is_some()));
            if let Some(native) = native { assert_eq!(values[0].id,native); }
        }
    }
    #[test]
    fn chooser_scrolls_to_last_shortcut_and_escape_cancels_at_80x24() {
        let _lock = UI_LOCK.lock().unwrap();
        let mut shortcuts = catalog::with_starters(vec![]);
        let prototype = shortcuts[0].clone();
        shortcuts.extend((0..60).map(|i| { let mut s = prototype.clone(); s.name = format!("App {i}"); s }));
        let app = App::debug(80,24,"Key.Pressed(End)\nKey.Pressed(Enter)").build().unwrap();
        assert_eq!(OpenWithDialog::new(Path::new("file.txt"),&shortcuts).show(),Some(AppId::External(62)));
        app.run();
        let app = App::debug(80,24,"Key.Pressed(Escape)").build().unwrap();
        assert_eq!(OpenWithDialog::new(Path::new("file.txt"),&shortcuts).show(),None);
        app.run();
    }
}
