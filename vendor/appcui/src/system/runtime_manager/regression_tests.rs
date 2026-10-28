use super::*;

#[test]
fn file_picker_preserves_background_colors_when_requested() {
    let mut theme = Theme::new(crate::system::Themes::Light);
    let original = Character::new('X', Color::RGB(17,34,51), Color::RGB(68,85,102), CharFlags::None);
    theme.desktop.character = original;
    let app = App::debug(100,30,"Paint.Enable(false)").theme(theme).build().unwrap();
    let mut modal = ModalWindow::<()>::new("Open image", layout!("a:c,w:50,h:15"), window::Flags::None);
    modal.preserve_modal_background();
    let runtime = RuntimeManager::get();
    let handle = runtime.add_window(modal);
    runtime.modal_windows.push(handle.cast());
    runtime.recompute_layouts();
    runtime.paint();
    runtime.surface.reset();
    assert_eq!(*runtime.surface.char(1,1).unwrap(), original);
    app.run();
}
