use crate::ui::common::traits::*;
use crate::input::Key;
pub trait DesktopEvents {
    /// Preview a key before focused children. Modal dialogs and open menus retain their input.
    fn on_preview_key_pressed(&mut self, _key: Key, _character: char) -> EventProcessStatus {
        EventProcessStatus::Ignored
    }
    fn on_start(&mut self) { }
    fn on_close(&mut self) -> ActionRequest {
        ActionRequest::Allow
    }
    fn on_update_window_count(&mut self, _count: usize) {

    }
}
