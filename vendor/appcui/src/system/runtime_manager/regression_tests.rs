use super::*;

#[ModalWindow(response = bool, events = TimerEvents, internal=true)]
struct TimerModal {}
impl TimerEvents for TimerModal {}

#[test]
fn modal_registration_connects_a_timer_started_before_showing() {
    let app = App::debug(80,24,"Paint.Enable(false)").timers_count(4).build().unwrap();
    let mut modal = TimerModal { base: ModalWindow::new("Worker",layout!("a:c,w:30,h:10"),window::Flags::None) };
    modal.timer().unwrap().start(std::time::Duration::from_millis(50));
    let timer = modal.base.timer_handle;
    let runtime = RuntimeManager::get();
    let handle = runtime.add_modal_window(modal);
    let receiver = runtime.timers_manager.control_handle(timer.index() as u8);
    app.run();
    assert_eq!(receiver,handle.cast::<()>(),"modal worker timers must route to the registered window");
}

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

#[test]
fn removing_window_menu_releases_handles_and_closes_its_popup() {
    let app = App::debug(100,30,"Paint.Enable(false)").app_bar().build().unwrap();
    let runtime = RuntimeManager::get();
    let menu = runtime.add_menu(Menu::new());
    let button = runtime.appbar.as_mut().unwrap().add(appbar::MenuButton::with_handle("Window", menu, 1, appbar::Side::Left));
    runtime.show_menu(menu, Handle::None, 5, 2, 0, None);
    assert!(runtime.opened_menu_handle == menu);
    let mut owner = ControlBase::new(layout!("d:f"), false);
    assert!(owner.unregister_menu(menu));
    assert!(runtime.opened_menu_handle.is_none());
    assert!(runtime.get_menu(menu).is_none());
    assert!(runtime.appbar.as_mut().unwrap().remove(button));
    assert!(runtime.appbar.as_ref().unwrap().get(button).is_none());
    assert!(!owner.unregister_menu(menu));
    app.run();
}

#[CustomControl(overwrite=OnKeyPressed+OnMouseEvent, internal=true)]
struct InputProbe {
    keys: Vec<(Key, char)>,
    mouse: Vec<MouseEvent>,
    pastes: Vec<String>,
    accept_paste: bool,
    hide_on_character: Option<char>,
}
impl InputProbe {
    fn new() -> Self {
        Self { base: ControlBase::new(layout!("d:f"), true), keys: Vec::new(), mouse: Vec::new(), pastes: Vec::new(), accept_paste: false, hide_on_character: None }
    }
}
impl OnKeyPressed for InputProbe {
    fn on_paste(&mut self, text: &str) -> EventProcessStatus {
        if self.accept_paste {
            self.pastes.push(text.to_string());
            EventProcessStatus::Processed
        } else { EventProcessStatus::Ignored }
    }
    fn on_key_pressed(&mut self, key: Key, character: char) -> EventProcessStatus {
        self.keys.push((key, character));
        if self.hide_on_character == Some(character) { self.set_visible(false); }
        EventProcessStatus::Processed
    }
}
impl OnMouseEvent for InputProbe {
    fn on_mouse_event(&mut self, event: &MouseEvent) -> EventProcessStatus {
        self.mouse.push(*event);
        EventProcessStatus::Processed
    }
}

#[test]
fn double_click_preserves_release_of_second_press() {
    let mut app = App::debug(40,12,"Paint.Enable(false)").build().unwrap();
    let mut window = window!("Test,x:1,y:1,w:20,h:8");
    let probe = window.add(InputProbe::new());
    app.add_window(window);
    let runtime = RuntimeManager::get();
    runtime.recompute_layouts();
    runtime.process_mouse_dblclick_event(MouseDoubleClickEvent { modifier: None, x:3,y:3,button:MouseButton::Left });
    runtime.process_mousebuttonup_event(MouseButtonUpEvent { modifier: None, x:3,y:3,button:MouseButton::Left });
    let events = runtime.get_control(probe).unwrap().mouse.clone();
    app.run();
    assert!(events.iter().any(|event| matches!(event, MouseEvent::DoubleClick(_))));
    assert!(events.iter().any(|event| matches!(event, MouseEvent::Released(_))));
}

#[test]
fn paste_prefers_whole_payload_handler_and_falls_back_to_focused_characters() {
    let mut app = App::debug(40,12,"Paint.Enable(false)").build().unwrap();
    let mut window = window!("Test,x:1,y:1,w:30,h:8");
    let probe = window.add(InputProbe::new());
    app.add_window(window);
    let runtime = RuntimeManager::get();
    runtime.recompute_layouts();
    runtime.process_system_event(SystemEvent::Paste("ก界e\u{301}".to_string()));
    let characters = runtime.get_control(probe).unwrap().keys.clone();
    runtime.get_control_mut(probe).unwrap().accept_paste = true;
    runtime.process_system_event(SystemEvent::Paste("whole\nbody".to_string()));
    let pastes = runtime.get_control(probe).unwrap().pastes.clone();
    let count = runtime.get_control(probe).unwrap().keys.len();
    app.run();
    assert_eq!(characters, vec![(Key::None,'ก'),(Key::None,'界'),(Key::None,'e'),(Key::None,'\u{301}')]);
    assert_eq!(pastes, vec!["whole\nbody"]);
    assert_eq!(count,4);
}

#[test]
fn paste_does_not_escape_modal_menu_or_hidden_control_boundaries() {
    let mut app = App::debug(40,12,"Paint.Enable(false)").build().unwrap();
    let mut window = window!("Test,x:1,y:1,w:30,h:8");
    let background = window.add(InputProbe::new());
    app.add_window(window);
    let mut modal = ModalWindow::<()>::new("Lock",layout!("a:c,w:20,h:8"),window::Flags::None);
    let mut probe = InputProbe::new();
    probe.hide_on_character = Some('!');
    let focused = modal.add(probe);
    let runtime = RuntimeManager::get();
    let modal = runtime.add_window(modal);
    runtime.modal_windows.push(modal.cast());
    runtime.recompute_layouts();
    runtime.process_system_event(SystemEvent::Paste("!secret".to_string()));
    let focused_keys = runtime.get_control(focused).unwrap().keys.clone();
    let background_keys = runtime.get_control(background).unwrap().keys.clone();
    runtime.modal_windows.clear();
    let menu = runtime.get_menus().add(Menu::new());
    runtime.show_menu(menu, Handle::None, 2, 2, 0, None);
    runtime.process_system_event(SystemEvent::Paste("menu".to_string()));
    let background_count = runtime.get_control(background).unwrap().keys.len();
    app.run();
    assert_eq!(focused_keys, vec![(Key::None,'!')]);
    assert!(background_keys.is_empty());
    assert_eq!(background_count,0);
}

#[test]
fn mouse_metadata_is_local_to_each_event_including_wheel_and_motion() {
    let mut app = App::debug(40,12,"Paint.Enable(false)").build().unwrap();
    let mut window = window!("Test,x:1,y:1,w:30,h:8");
    let probe = window.add(InputProbe::new());
    app.add_window(window);
    let runtime = RuntimeManager::get();
    runtime.recompute_layouts();
    runtime.key_modifier = KeyModifier::Alt;
    runtime.process_mousebuttondown_event(MouseButtonDownEvent { modifier: Some(KeyModifier::Ctrl),x:3,y:3,button:MouseButton::Left });
    runtime.process_mousebuttonup_event(MouseButtonUpEvent { modifier: Some(KeyModifier::Shift),x:3,y:3,button:MouseButton::Left });
    runtime.process_mousewheel_event(MouseWheelEvent { modifier: Some(KeyModifier::None),x:4,y:3,direction:MouseWheelDirection::Down });
    runtime.process_mousemove_event(MouseMoveEvent { modifier: Some(KeyModifier::Ctrl),x:5,y:3,button:MouseButton::None });
    let events = runtime.get_control(probe).unwrap().mouse.clone();
    app.run();
    assert!(events.iter().any(|event| matches!(event, MouseEvent::Pressed(data) if data.modifier == KeyModifier::Ctrl)));
    assert!(events.iter().any(|event| matches!(event, MouseEvent::Released(data) if data.modifier == KeyModifier::Shift)));
    assert!(events.iter().any(|event| matches!(event, MouseEvent::WheelAt(data) if data.x == 2 && data.y == 1 && data.modifier == KeyModifier::None)));
    assert!(events.iter().any(|event| matches!(event, MouseEvent::OverAt(data) if data.x == 3 && data.y == 1 && data.modifier == KeyModifier::Ctrl)));
}

#[test]
fn clipped_control_mouse_coordinates_follow_its_origin() {
    let mut app = App::debug(40,12,"Paint.Enable(false)").build().unwrap();
    let mut window = window!("Test,x:1,y:1,w:30,h:8");
    let mut probe = InputProbe::new();
    probe.base = ControlBase::new(layout!("x:-3,y:0,w:20,h:4"), true);
    let probe = window.add(probe);
    app.add_window(window);
    let runtime = RuntimeManager::get();
    runtime.recompute_layouts();
    runtime.process_mousebuttondown_event(MouseButtonDownEvent { modifier: None,x:3,y:3,button:MouseButton::Left });
    runtime.process_mousemove_event(MouseMoveEvent { modifier: None,x:4,y:3,button:MouseButton::Left });
    runtime.process_mousebuttonup_event(MouseButtonUpEvent { modifier: None,x:4,y:3,button:MouseButton::Left });
    runtime.process_mousewheel_event(MouseWheelEvent { modifier: None,x:4,y:3,direction:MouseWheelDirection::Down });
    runtime.process_mousemove_event(MouseMoveEvent { modifier: None,x:5,y:3,button:MouseButton::None });
    runtime.process_mouse_dblclick_event(MouseDoubleClickEvent { modifier: None,x:3,y:3,button:MouseButton::Left });
    runtime.process_mousebuttonup_event(MouseButtonUpEvent { modifier: None,x:3,y:3,button:MouseButton::Left });
    let events = runtime.get_control(probe).unwrap().mouse.clone();
    app.run();
    assert!(events.iter().any(|event| matches!(event, MouseEvent::Pressed(data) if data.x == 4 && data.y == 1)), "{events:?}");
    assert!(events.iter().any(|event| matches!(event, MouseEvent::Released(data) if data.x == 5 && data.y == 1)));
    assert!(events.iter().any(|event| matches!(event, MouseEvent::Drag(data) if data.x == 5 && data.y == 1)));
    assert!(events.iter().any(|event| matches!(event, MouseEvent::WheelAt(data) if data.x == 5 && data.y == 1)));
    assert!(events.iter().any(|event| matches!(event, MouseEvent::OverAt(data) if data.x == 6 && data.y == 1)));
    assert!(events.iter().any(|event| matches!(event, MouseEvent::DoubleClick(data) if data.x == 4 && data.y == 1)));
}
