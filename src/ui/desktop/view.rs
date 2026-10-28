use self::mydesktop::Commands;
use super::confirm_system_action::ConfirmSystemAction;
use super::help::help_window;
use super::panel::{self, BarLayout};
use crate::config::preferences::Preferences;
use crate::config::Shortcut;
use crate::platform::linux::{StatusMonitor, SystemAction, SystemActionRunner, SystemActions};
use crate::ui::locking::{LockSetupDialog, TerminalLockScreen};
use crate::ui::preferences::PreferencesDialog;
use crate::ui::terminal::TuiWindow;
use crate::ui::build_theme;
use appcui::prelude::appbar::MenuButton;
use appcui::prelude::menu::{Command, SingleChoice};
use appcui::prelude::*;
use appcui::ui::appbar::Side;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[Desktop(
    events = [AppBarEvents, MenuEvents, DesktopEvents, TimerEvents],
    overwrite = OnPaint+OnResize,
    commands = [Help, Preference, TerminalLock, ScreenLock, Shutdown, Restart, Exit, NoArrange, Cascade, Vertical, Horizontal, Grid, AppVisibilityToggle, OpenApp, CloseApp, AppCommand, SelectApp, None]
)]
pub struct MyDesktop {
    pub preferences: Preferences,
    settings_path: PathBuf,
    startup_warning: Option<String>,
    system_actions: SystemActions,
    terminal_locked: bool,
    pub arrange_method: Option<desktop::ArrangeWindowsMethod>,
    pub desktop_menu: Handle<MenuButton>,
    pub arrange_menu: Handle<MenuButton>,
    pub separator: Handle<appbar::Separator>,
    pub app_menues: Vec<Handle<Menu>>,
    pub app_menu_buttons: Vec<Handle<MenuButton>>,
    pub shortcuts: Vec<Shortcut>,
    pub app_windows: HashMap<usize, Handle<TuiWindow>>,
    pub status_label: Handle<appbar::Label>,
    status: StatusMonitor,
    apps_button: Handle<MenuButton>,
    app_selectors: Vec<Handle<Command>>,
}

impl MyDesktop {
    pub fn new(
        shortcuts: Vec<Shortcut>,
        preferences: Preferences,
        settings_path: PathBuf,
        startup_warning: Option<String>,
        runner: Arc<dyn SystemActionRunner>,
    ) -> Self {
        Self {
            base: Desktop::new(),
            preferences,
            settings_path,
            startup_warning,
            system_actions: SystemActions::new(runner),
            terminal_locked: false,
            arrange_method: None,
            desktop_menu: Handle::None,
            separator: Handle::None,
            arrange_menu: Handle::None,
            app_menues: vec![Handle::None; shortcuts.len()],
            app_menu_buttons: vec![Handle::None; shortcuts.len()],
            app_windows: HashMap::new(),
            status_label: Handle::None,
            status: StatusMonitor::default(),
            apps_button: Handle::None,
            app_selectors: Vec::new(),
            shortcuts,
        }
    }

    pub fn apply_preferences(&mut self, preferences: Preferences) {
        App::set_theme(build_theme(&preferences.desktop));
        for handle in self.app_windows.values().copied().collect::<Vec<_>>() {
            if let Some(window) = self.window_mut(handle) {
                window.apply_palette(&preferences.terminal);
            }
        }
        self.preferences = preferences;
        self.refresh_panel();
    }

    fn panel_caption(&self) -> (String, String, BarLayout) {
        let now = chrono::Local::now().naive_local();
        let pending = self.system_actions.is_pending();
        let full = panel::render(
            &self.preferences.panel,
            &self.status.snapshot,
            now,
            usize::MAX,
            pending,
        );
        let widths = self
            .shortcuts
            .iter()
            .map(|shortcut| {
                // Count hotkey markers conservatively so escaped ampersands never
                // make the actual menus wider than the reserved space.
                (shortcut.name.chars().count().max(1) + 2).min(255)
            })
            .collect::<Vec<_>>();
        let layout = panel::bar_layout(self.terminal_size().width as usize, full.len(), &widths);
        let caption = panel::render(
            &self.preferences.panel,
            &self.status.snapshot,
            now,
            layout.status_width,
            pending,
        );
        (caption, full, layout)
    }
    fn refresh_panel(&mut self) -> bool {
        if self.status_label.is_none() || self.terminal_locked {
            return false;
        }
        let (caption, full, _) = self.panel_caption();
        let handle = self.status_label;
        if let Some(label) = self.appbar().get_mut(handle) {
            let changed = label.caption() != caption;
            if changed {
                label.set_caption(&caption);
            }
            label.set_tooltip(&full.trim().replace(" | ", "\n"));
            changed
        } else {
            false
        }
    }

    fn hide_for_terminal_lock(&mut self) -> Vec<(Handle<TuiWindow>, bool)> {
        self.terminal_locked = true;
        let mut visibility = Vec::new();
        for handle in self.app_windows.values().copied().collect::<Vec<_>>() {
            if let Some(window) = self.window_mut(handle) {
                visibility.push((handle, window.is_visible()));
                window.set_visible(false);
            }
        }
        visibility
    }

    fn restore_after_terminal_unlock(&mut self, visibility: Vec<(Handle<TuiWindow>, bool)>) {
        for (handle, visible) in visibility {
            if let Some(window) = self.window_mut(handle) {
                window.set_visible(visible);
            }
        }
        self.terminal_locked = false;
        self.refresh_panel();
    }

    fn lock_terminal(&mut self) {
        let credential = match Preferences::stored_credential(&self.settings_path) {
            Ok(Some(credential)) => credential,
            Ok(None) => {
                let Some(credential) = LockSetupDialog::new().show() else {
                    return;
                };
                credential
            }
            Err(error) => {
                dialogs::error(
                    "Terminal Lock unavailable",
                    &format!(
                        "{error:#}\nNo lock was started. Repair the settings file and its permissions before trying again."
                    ),
                );
                return;
            }
        };
        let visibility = self.hide_for_terminal_lock();
        // Only a verified password restores the desktop. Backend shutdown is not an unlock.
        if TerminalLockScreen::new(credential).show() == Some(true) {
            self.restore_after_terminal_unlock(visibility);
        }
    }

    fn request_system_action(&mut self, action: SystemAction) {
        if self.system_actions.is_pending() {
            dialogs::error(
                "System request pending",
                "Wait for the current system request to finish before sending another.",
            );
            return;
        }
        let confirmed =
            action == SystemAction::Lock || ConfirmSystemAction::new(action).show() == Some(true);
        if let Err(error) = self.system_actions.request(action, confirmed) {
            dialogs::error("System request failed", &error);
        }
    }

    pub fn create_window(
        &mut self,
        index: usize,
        command: String,
        args: Vec<String>,
    ) -> anyhow::Result<()> {
        let app_name = self.shortcuts[index].name.clone();
        let window = self.shortcuts[index].window.clone();
        let terminal = self.shortcuts[index].terminal.clone();

        let window = TuiWindow::new(
            &app_name,
            command,
            args,
            window,
            terminal,
            &self.preferences.terminal,
        )?;

        let win_handle = self.add_window(window);
        self.app_windows.insert(index, win_handle);

        Ok(())
    }
}

impl OnPaint for MyDesktop {
    fn on_paint(&self, surface: &mut Surface, theme: &Theme) {
        surface.clear(theme.desktop.character);
    }
}
impl OnResize for MyDesktop {
    fn on_resize(&mut self, _old_size: Size, _new_size: Size) {
        if self.terminal_locked || self.status_label.is_none() {
            return;
        }
        let handle = self.status_label;
        if let Some(label) = self.appbar().get_mut(handle) {
            // AppCUI truncates the shown appbar items on resize without asking
            // us to rebuild them. Force refresh even with an empty/unchanged
            // caption, so Apps grouping and newly available menus are rebuilt.
            let caption = label.caption().to_owned();
            label.set_caption(&caption);
        }
    }
}

impl DesktopEvents for MyDesktop {
    fn on_start(&mut self) {
        let shortcuts = self.shortcuts.clone();
        self.status_label = self.appbar().add(appbar::Label::new("", 0, Side::Right));
        let mut desktop_menu = Menu::new();

        desktop_menu.add(Command::new("Help", Key::None, Commands::Help));
        desktop_menu.add(Command::new("Preference", Key::None, Commands::Preference));
        desktop_menu.add(menu::Separator::new());
        desktop_menu.add(Command::new(
            "Terminal Lock",
            Key::None,
            Commands::TerminalLock,
        ));
        desktop_menu.add(Command::new("Screen Lock", Key::None, Commands::ScreenLock));
        desktop_menu.add(Command::new("Shutdown", Key::None, Commands::Shutdown));
        desktop_menu.add(Command::new("Restart", Key::None, Commands::Restart));
        desktop_menu.add(menu::Separator::new());
        desktop_menu.add(Command::new("Exit", Key::None, Commands::Exit));

        let desktop_menu_button =
            self.appbar()
                .add(MenuButton::new("Desktop", desktop_menu, 1, Side::Left));

        let mut tilling_menu = Menu::new();

        tilling_menu.add(SingleChoice::new(
            "No arrangement",
            Key::None,
            Commands::NoArrange,
            true,
        ));
        tilling_menu.add(SingleChoice::new(
            "Cascade",
            Key::None,
            Commands::Cascade,
            false,
        ));
        tilling_menu.add(SingleChoice::new(
            "Vertical",
            Key::None,
            Commands::Vertical,
            false,
        ));
        tilling_menu.add(SingleChoice::new(
            "Horizontal",
            Key::None,
            Commands::Horizontal,
            false,
        ));
        tilling_menu.add(SingleChoice::new("Grid", Key::None, Commands::Grid, false));

        let arrange_menu_button =
            self.appbar()
                .add(MenuButton::new("Tilling", tilling_menu, 2, Side::Left));

        let separator = self.appbar().add(appbar::Separator::new(3, Side::Left));
        let mut apps = Menu::new();
        self.app_selectors.clear();

        let mut app_menues = vec![Handle::<Menu>::None; shortcuts.len()];
        let mut app_menu_buttons = vec![Handle::<MenuButton>::None; shortcuts.len()];
        for (index, shortcut) in shortcuts.iter().enumerate() {
            self.app_selectors.push(apps.add(Command::new(
                &shortcut.name,
                Key::None,
                Commands::SelectApp,
            )));
            let mut menu = Menu::new();

            menu.add(Command::new(
                "Hide",
                Key::None,
                Commands::AppVisibilityToggle,
            ));
            menu.add(Command::new("Start", Key::None, Commands::OpenApp));
            menu.add(Command::new("Close", Key::None, Commands::CloseApp));

            if !shortcut.taskbar.additional_commands.is_empty() {
                menu.add(menu::Separator::new());
            }

            for command in &shortcut.taskbar.additional_commands {
                menu.add(Command::new(&command.name, Key::None, Commands::AppCommand));
            }

            app_menues[index] = self.register_menu(menu);
            app_menu_buttons[index] = self.appbar().add(MenuButton::with_handle(
                &shortcut.name,
                app_menues[index],
                (5usize + index).min(255) as u8,
                Side::Left,
            ));
        }

        self.apps_button = self
            .appbar()
            .add(MenuButton::new("Apps", apps, 4, Side::Left));

        self.desktop_menu = desktop_menu_button;
        self.arrange_menu = arrange_menu_button;
        self.separator = separator;
        self.app_menues = app_menues;
        self.app_menu_buttons = app_menu_buttons;
        self.refresh_panel();

        let timer = self.timer().expect("Failed to get timer");
        timer.start(Duration::from_millis(100));
        if let Some(warning) = self.startup_warning.take() {
            dialogs::error("Preferences warning", &warning);
        }
    }

    fn on_update_window_count(&mut self, _count: usize) {
        if self.terminal_locked {
            return;
        }
        let m = self.arrange_method;

        if let Some(method) = m {
            self.arrange_windows(method);
        }
    }

    fn on_close(&mut self) -> ActionRequest {
        // Escape can arrive separately as the prefix of an Alt key sequence.
        // Leave application shutdown to the explicit Desktop > Exit command.
        ActionRequest::Deny
    }
}

impl AppBarEvents for MyDesktop {
    fn on_update(&self, app_bar: &mut AppBar) {
        if self.terminal_locked {
            return;
        }
        let (caption, full, layout) = self.panel_caption();
        if let Some(label) = app_bar.get_mut(self.status_label) {
            if label.caption() != caption {
                label.set_caption(&caption);
            }
            label.set_tooltip(&full.trim().replace(" | ", "\n"));
        }
        if !caption.is_empty() {
            app_bar.show(self.status_label);
        }
        app_bar.show(self.desktop_menu);
        app_bar.show(self.arrange_menu);
        app_bar.show(self.separator);

        if layout.grouped_apps {
            app_bar.show(self.apps_button);
        } else {
            for app_menu in self.app_menu_buttons.iter() {
                app_bar.show(*app_menu);
            }
        }
    }
}

impl MenuEvents for MyDesktop {
    fn on_command(&mut self, menu: Handle<Menu>, item: Handle<Command>, command: Commands) {
        if self.terminal_locked {
            return;
        }
        match command {
            Commands::SelectApp => {
                if let Some(index) = self
                    .app_selectors
                    .iter()
                    .position(|selector| *selector == item)
                {
                    if let Some(menu) = self.app_menues.get(index) {
                        self.show_menu(*menu, 0, 0, None);
                    }
                }
            }
            Commands::Help => {
                let handle = self.add_window(help_window());
                if let Some(window) = self.window_mut(handle) {
                    window.focus_categories();
                }
            }
            Commands::Preference => {
                if let Some(preferences) =
                    PreferencesDialog::new(&self.preferences, self.settings_path.clone()).show()
                {
                    self.apply_preferences(preferences);
                }
            }
            Commands::ScreenLock => self.request_system_action(SystemAction::Lock),
            Commands::TerminalLock => self.lock_terminal(),
            Commands::Shutdown => self.request_system_action(SystemAction::Shutdown),
            Commands::Restart => self.request_system_action(SystemAction::Restart),
            Commands::Exit => {
                for window in self.app_windows.clone().values() {
                    if let Some(win) = self.window_mut(*window) {
                        win.close_command();
                    }
                }

                self.close()
            }
            Commands::OpenApp
            | Commands::CloseApp
            | Commands::AppVisibilityToggle
            | Commands::AppCommand => {
                let mut app = None;

                for (index, app_menu) in self.app_menues.iter().enumerate() {
                    if &menu == app_menu {
                        app = Some(index);
                    }
                }

                if let Some(index) = app {
                    let win_handle = match self.app_windows.get(&index) {
                        Some(win_handle) => Some(*win_handle),
                        None => {
                            match command {
                                Commands::OpenApp => {
                                    let command = self.shortcuts[index].command.clone();
                                    let args = self.shortcuts[index].args.clone();
                                    self.create_window(index, command, args).ok();
                                }
                                Commands::AppCommand => {
                                    let shortcut = self.shortcuts[index].clone();
                                    let item = self.menuitem_mut(menu, item).unwrap();

                                    for command in shortcut.taskbar.additional_commands {
                                        if item.caption() == command.name {
                                            self.create_window(
                                                index,
                                                command.command,
                                                command.args,
                                            )
                                            .ok();
                                            break;
                                        }
                                    }
                                }
                                _ => {}
                            }

                            None
                        }
                    };

                    let mut visibility_item = None;

                    if let Some(win_handle) = win_handle {
                        if let Some(window) = self.window_mut(win_handle) {
                            match command {
                                Commands::AppVisibilityToggle => match window.is_visible() {
                                    true => {
                                        window.set_visible(false);
                                        visibility_item = Some("Show");
                                    }
                                    false => {
                                        window.set_visible(true);
                                        visibility_item = Some("Hide");
                                    }
                                },
                                Commands::OpenApp => {}
                                Commands::CloseApp => {
                                    window.close_command();
                                    self.app_windows.remove(&index);
                                }
                                _ => {}
                            }
                        } else {
                            match command {
                                Commands::OpenApp => {
                                    let command = self.shortcuts[index].command.clone();
                                    let args = self.shortcuts[index].args.clone();
                                    self.create_window(index, command, args).ok();
                                }
                                _ => {}
                            }
                        }
                    }

                    if let Some(name) = visibility_item {
                        let item = self.menuitem_mut(menu, item).unwrap();
                        item.set_caption(name);
                    }
                }
            }
            _ => {}
        }
    }

    fn on_select(&mut self, _menu: Handle<Menu>, _item: Handle<SingleChoice>, command: Commands) {
        if self.terminal_locked {
            return;
        }
        match command {
            Commands::NoArrange => self.arrange_method = None,
            Commands::Cascade => self.arrange_method = Some(desktop::ArrangeWindowsMethod::Cascade),
            Commands::Vertical => {
                self.arrange_method = Some(desktop::ArrangeWindowsMethod::Vertical)
            }
            Commands::Horizontal => {
                self.arrange_method = Some(desktop::ArrangeWindowsMethod::Horizontal)
            }
            Commands::Grid => self.arrange_method = Some(desktop::ArrangeWindowsMethod::Grid),
            _ => {}
        }
        let m = self.arrange_method;

        if let Some(method) = m {
            self.arrange_windows(method);
        }
    }
}

impl TimerEvents for MyDesktop {
    fn on_update(&mut self, _: u64) -> EventProcessStatus {
        if self.terminal_locked {
            return EventProcessStatus::Ignored;
        }
        let settings = &self.preferences.panel;
        self.status.poll(
            Instant::now(),
            settings.enabled
                && (settings.show_network
                    || settings.show_battery
                    || settings.show_cpu
                    || settings.show_ram
                    || settings.show_disk
                    || settings.show_network_usage),
        );
        let mut changed = self.refresh_panel();
        if let Some((action, result)) = self.system_actions.poll() {
            changed = true;
            if let Err(error) = result {
                dialogs::error(action.label(), &error);
            }
        }

        if changed {
            EventProcessStatus::Processed
        } else {
            EventProcessStatus::Ignored
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::preferences::Rgb;
    use crate::test_support::{TestDir, UI_LOCK};
    use virtual_terminal::{Input, Output};
    #[test]
    fn resizing_regroups_and_restores_apps_with_panel_enabled_or_disabled() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        struct OfflineStatus;
        impl crate::platform::linux::StatusReader for OfflineStatus {
            fn read(&self) -> crate::platform::linux::StatusSnapshot {
                crate::platform::linux::StatusSnapshot {
                    network: crate::platform::linux::NetworkStatus::Offline,
                    battery: None,
                    ..Default::default()
                }
            }
        }
        // These are the frames of fresh desktops started directly at each size.
        // A resize must produce the same reachable menus as a fresh desktop.
        for (enabled, wide, narrow) in [
            (false, 0xC20267277BB8D639u64, 0xD9D452254B0140u64),
            (true, 0xDBF44FED245B6A1Fu64, 0x23B98BF63E9CD826u64),
        ] {
            let mut shortcuts = Vec::new();
            for name in [
                "First Application",
                "Second Application",
                "Third Application",
            ] {
                let mut shortcut: Shortcut =
                    toml::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shortcuts/sh.toml"))).unwrap();
                shortcut.name = name.into();
                shortcuts.push(shortcut);
            }
            let mut preferences = Preferences::default();
            preferences.panel.enabled = enabled;
            preferences.panel.show_clock = false;
            preferences.panel.show_date = false;
            preferences.panel.show_battery = false;
            preferences.panel.show_cpu = false;
            preferences.panel.show_ram = false;
            preferences.panel.show_disk = false;
            preferences.panel.show_network_usage = false;
            let mut desktop = MyDesktop::new(
                shortcuts,
                preferences,
                dir.path().join("preferences.toml"),
                None,
                Arc::new(NoSystemActions),
            );
            desktop.status = StatusMonitor::new(Arc::new(OfflineStatus));
            desktop.status.snapshot.network = crate::platform::linux::NetworkStatus::Offline;
            let script = format!(
                "Paint('initial')\nCheckHash({wide:#X})\nResize(80,24)\nPaint('narrow')\nCheckHash({narrow:#X})\nResize(100,24)\nPaint('wide restored')\nCheckHash({wide:#X})"
            );
            let app = App::debug(100, 24, &script)
                .desktop(desktop)
                .app_bar()
                .build()
                .unwrap();
            app.run();
        }
    }
    struct ProcessCleanup(Vec<(async_channel::Sender<Input>, u32)>);
    impl Drop for ProcessCleanup {
        fn drop(&mut self) {
            for (tx, _) in &self.0 {
                let _ = tx.try_send(Input::Data(b"\n\n".to_vec()));
            }
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while self
                .0
                .iter()
                .any(|(_, pid)| std::path::Path::new("/proc").join(pid.to_string()).exists())
                && std::time::Instant::now() < deadline
            {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
    struct NoSystemActions;
    impl SystemActionRunner for NoSystemActions {
        fn run(&self, _action: SystemAction) -> Result<(), String> {
            panic!("system actions must not run in this test")
        }
    }
    fn first_cell(window: &mut TuiWindow) -> Character {
        let canvas = window.canvas;
        *window
            .control_mut(canvas)
            .unwrap()
            .drawing_surface_mut()
            .char(0, 0)
            .unwrap()
    }
    fn pump_until(window: &mut TuiWindow, expected: char) {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while first_cell(window).code != expected {
            TimerEvents::on_update(window, 0);
            assert!(
                std::time::Instant::now() < deadline,
                "terminal did not produce {expected}"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn terminal_lock_reuses_saved_password_and_never_downgrades_broken_settings() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let path = dir.path().join("preferences.toml");
        let mut preferences = Preferences::default();
        preferences.terminal_lock =
            Some(crate::runtime::LockCredential::new("reusable", "reusable").unwrap());
        preferences.save(&path, None).unwrap();
        for (password, remains_locked) in [("wrong", true), ("reusable", false)] {
            // Simulate a fresh instance whose in-memory palette has no credential.
            let mut desktop = MyDesktop::new(
                vec![],
                Preferences::default(),
                path.clone(),
                None,
                Arc::new(NoSystemActions),
            );
            let app = App::debug(80,24,&format!("Key.TypeText('{password}')\nKey.Pressed(Enter)\nKey.Pressed(Escape)\nPaint('end')")).build().unwrap();
            desktop.lock_terminal();
            assert_eq!(desktop.terminal_locked, remains_locked);
            app.run();
        }
        std::fs::write(&path, "terminal_lock = 'broken'").unwrap();
        let mut desktop =
            MyDesktop::new(vec![], preferences, path, None, Arc::new(NoSystemActions));
        let app = App::debug(80,24,"Key.Pressed(Enter)\nKey.TypeText('temporary')\nKey.Pressed(Tab)\nKey.TypeText('temporary')\nKey.Pressed(Enter)\nPaint('end')").build().unwrap();
        desktop.lock_terminal();
        assert!(
            !desktop.terminal_locked,
            "invalid storage must show an error, never permit temporary-password setup"
        );
        app.run();
    }
    #[test]
    fn applying_preferences_recolors_visible_and_hidden_windows_without_restarting_apps() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _entered = runtime.enter();
        let dir = TestDir::new();
        let mut shortcut: Shortcut = toml::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shortcuts/sh.toml"))).unwrap();
        shortcut.command = "/bin/sh".into();
        shortcut.args = vec![
            "-c".into(),
            r"printf '\033[31mA'; read answer; printf '\033[H\033[0mZ'; read answer".into(),
        ];
        shortcut.window.size = Some(crate::config::WindowSize {
            width: 30,
            height: 8,
        });
        shortcut.terminal.background_color = None;
        let mut overridden = shortcut.clone();
        overridden.terminal.background_color =
            Some(crate::config::BackgroundColor { r: 7, g: 8, b: 9 });
        let old = Preferences::default();
        let mut desktop = MyDesktop::new(
            vec![shortcut.clone(), overridden],
            old.clone(),
            dir.path().join("preferences.toml"),
            None,
            Arc::new(NoSystemActions),
        );
        let app = App::debug(80, 24, "Paint('live windows')").build().unwrap();
        for index in 0..2 {
            desktop
                .create_window(index, shortcut.command.clone(), shortcut.args.clone())
                .unwrap();
        }
        let handles = [desktop.app_windows[&0], desktop.app_windows[&1]];
        let mut pids = vec![];
        let mut cleanup = ProcessCleanup(vec![]);
        for handle in handles {
            let window = desktop.window_mut(handle).unwrap();
            let ckc = window.custom_keyboard_control;
            let rx = window.control(ckc).unwrap().rx.clone();
            match rx.recv_blocking().unwrap() {
                Output::Pid(pid) => {
                    pids.push(pid);
                    cleanup
                        .0
                        .push((window.control(ckc).unwrap().tx.clone(), pid));
                }
                other => panic!("{other:?}"),
            }
            pump_until(window, 'A');
        }
        desktop.window_mut(handles[1]).unwrap().set_visible(false);
        let mut next = old;
        next.terminal.ansi[1] = Rgb(11, 12, 13);
        next.terminal.background = Rgb(21, 22, 23);
        next.terminal.foreground = Rgb(31, 32, 33);
        desktop.apply_preferences(next.clone());
        for (index, handle) in handles.into_iter().enumerate() {
            let window = desktop.window_mut(handle).unwrap();
            assert_eq!(first_cell(window).code, 'A');
            assert_eq!(first_cell(window).foreground, next.terminal.ansi[1].color());
            assert_eq!(
                first_cell(window).background,
                if index == 0 {
                    next.terminal.background.color()
                } else {
                    Color::RGB(7, 8, 9)
                }
            );
            assert_eq!(window.is_visible(), index == 0);
            let control = window.control(window.custom_keyboard_control).unwrap();
            control
                .tx
                .send_blocking(Input::Data(b"\n".to_vec()))
                .unwrap();
            pump_until(window, 'Z');
            assert_eq!(
                first_cell(window).foreground,
                next.terminal.foreground.color()
            );
        }
        let visibility = desktop.hide_for_terminal_lock();
        assert!(desktop.terminal_locked);
        assert!(DesktopEvents::on_close(&mut desktop) == ActionRequest::Deny);
        MenuEvents::on_command(&mut desktop, Handle::None, Handle::None, Commands::Exit);
        MenuEvents::on_command(
            &mut desktop,
            Handle::None,
            Handle::None,
            Commands::Preference,
        );
        MenuEvents::on_select(&mut desktop, Handle::None, Handle::None, Commands::Grid);
        assert!(desktop.arrange_method.is_none());
        for handle in handles {
            let window = desktop.window_mut(handle).unwrap();
            assert!(!window.is_visible());
            assert!(
                !window
                    .control(window.custom_keyboard_control)
                    .unwrap()
                    .tx
                    .is_closed()
            );
            assert_eq!(first_cell(window).code, 'Z');
        }
        for pid in &pids {
            assert!(std::path::Path::new("/proc").join(pid.to_string()).exists());
        }
        desktop.restore_after_terminal_unlock(visibility);
        assert!(!desktop.terminal_locked);
        assert!(desktop.window_mut(handles[0]).unwrap().is_visible());
        assert!(!desktop.window_mut(handles[1]).unwrap().is_visible());
        // Existing arrangement and explicit Exit still work with the updated windows.
        desktop.arrange_windows(desktop::ArrangeWindowsMethod::Grid);
        MenuEvents::on_command(&mut desktop, Handle::None, Handle::None, Commands::Exit);
        app.run();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        for pid in pids {
            while std::path::Path::new("/proc").join(pid.to_string()).exists() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "Exit left application {pid} running"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
    #[test]
    fn empty_desktop_stays_open_for_escape_and_alt_keys() {
        // Capture the debug backend's paint probes in a subprocess: an early
        // application exit would otherwise skip the rest of the script silently.
        const CHILD_ENV: &str = "DESKTOP_TUI_INPUT_TEST_CHILD";
        if std::env::var_os(CHILD_ENV).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "ui::desktop::view::tests::empty_desktop_stays_open_for_escape_and_alt_keys", "--nocapture"])
                .env(CHILD_ENV, "1")
                .output()
                .unwrap();

            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert_eq!(stdout.matches("desktop-input-alive").count(), 2, "Desktop exited before the input probe");
            assert!(!stdout.contains("desktop-exit-failed"), "Desktop > Exit did not close the application");
            return;
        }

        let _ui_lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let mut script = String::from("Paint.Enable(false)\nKey.Modifier(Alt)\n");
        for key in ['A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M',
                    'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
                    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9'] {
            script.push_str(&format!("Key.Pressed(Alt+{})\n", key));
        }
        script.push_str("Key.Pressed(Alt+Left)\nKey.Pressed(Alt+F4)\n");
        script.push_str("Key.Modifier(None)\nKey.Pressed(Escape)\nKey.Pressed(A)\n");
        script.push_str("Mouse.Click(4,0,left)\nKey.Pressed(Escape)\n");
        script.push_str("Paint.Enable(true)\nPaint('desktop-input-alive')\n");
        script.push_str("Mouse.Click(4,0,left)\nKey.Pressed(Down)\nKey.Pressed(End)\nKey.Pressed(Enter)\nPaint('desktop-exit-failed')\n");

        for shortcuts in [vec![], vec![toml::from_str::<Shortcut>(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shortcuts/helix.toml"))).unwrap()]] {
            let app = App::debug(80, 25, &script)
                .desktop(MyDesktop::new(
                    shortcuts,
                    Preferences::default(),
                    dir.path().join("preferences.toml"),
                    None,
                    Arc::new(NoSystemActions),
                ))
                .app_bar()
                .color_schema(false)
                .build()
                .unwrap();

            app.run();
        }
    }
}
