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

#[test]
fn hovering_parent_categories_switches_open_submenus() {
    let app = App::debug(100,30,"Paint.Enable(false)").build().unwrap();
    #[derive(Copy, Clone)]
    struct TestCommand(u32);
    impl CommandID for TestCommand {}
    impl From<TestCommand> for u32 { fn from(command: TestCommand) -> u32 { command.0 } }
    let mut root = Menu::new();
    let mut first = Menu::new();
    first.add(menu::Command::new("Terminal", Key::None, TestCommand(1)));
    root.add(menu::SubMenu::new("System", first));
    let mut second = Menu::new();
    second.add(menu::Command::new("Image Viewer", Key::None, TestCommand(2)));
    root.add(menu::SubMenu::new("Utility", second));
    let runtime = RuntimeManager::get();
    let root = runtime.get_menus().add(root);
    runtime.show_menu(root, Handle::None, 5, 2, 0, None);
    runtime.process_menu_mouse_click(root, 7, 3);
    let first = runtime.opened_menu_handle;
    assert!(first != root);
    assert!(runtime.process_menu_and_cmdbar_mousemove(7, 4));
    let second = runtime.opened_menu_handle;
    assert!(second != root && second != first, "hovering Utility must replace the System submenu");
    assert!(runtime.process_menu_and_cmdbar_mousemove(7, 3));
    assert!(runtime.opened_menu_handle == first, "hovering back to System must restore its submenu");
    app.run();
}
