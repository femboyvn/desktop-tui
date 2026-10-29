use self::mydesktop::Commands;
use super::confirm_system_action::ConfirmSystemAction;
use super::help::{help_window, HelpWindow};
use super::panel::{self, BarLayout};
use crate::config::preferences::Preferences;
use crate::config::Shortcut;
use crate::config::catalog::{self, AppId, GameKind};
use crate::platform::linux::{StatusMonitor, SystemAction, SystemActionRunner, SystemActions};
use crate::ui::locking::{LockSetupDialog, TerminalLockScreen};
use crate::ui::preferences::PreferencesDialog;
use crate::ui::terminal::TuiWindow;
use crate::ui::native::{self, ImageWindow, BrowserWindow, MusicWindow, GameWindow};
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
    commands = [Help, Preference, TerminalLock, ScreenLock, Shutdown, Restart, Exit, NoArrange, Cascade, Vertical, Horizontal, Grid, AppVisibilityToggle, OpenApp, CloseApp, AppCommand, SelectWindow, LaunchApplication, FocusWindow, HideWindow, CloseWindow, WindowCommands, BackgroundWindows, None]
)]
pub struct MyDesktop {
    pub preferences: Preferences,
    settings_path: PathBuf,
    startup_warning: Option<String>,
    system_actions: SystemActions,
    terminal_locked: bool,
    pub arrange_method: Option<desktop::ArrangeWindowsMethod>,
    pub desktop_menu: Handle<MenuButton>,
    application_menu: Handle<MenuButton>,
    application_commands: Vec<(Handle<Command>, AppId)>,
    additional_commands: Vec<(Handle<Command>, usize, String, Vec<String>)>,
    pub arrange_menu: Handle<MenuButton>,
    pub separator: Handle<appbar::Separator>,
    visibility_items: Vec<Handle<Command>>,
    pub app_menues: Vec<Handle<Menu>>,
    window_tasks: Vec<WindowTask>,
    window_order: Vec<WindowTarget>,
    help_windows: Vec<Handle<HelpWindow>>,
    application_root: Handle<Menu>,
    apps_menu: Handle<Menu>,
    background_menu: Handle<Menu>,
    background_entry: Handle<Command>,
    window_selectors: Vec<(Handle<Command>, WindowTarget)>,
    pub shortcuts: Vec<Shortcut>,
    pub app_windows: HashMap<usize, Handle<TuiWindow>>,
    image_window: Handle<ImageWindow>,
    browser_window: Handle<BrowserWindow>,
    music_window: Handle<MusicWindow>,
    game_windows: HashMap<GameKind, Handle<GameWindow>>,
    document_windows: Vec<Handle<TuiWindow>>,
    pub status_label: Handle<appbar::Label>,
    status: StatusMonitor,
    apps_button: Handle<MenuButton>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum WindowTarget {
    Terminal(Handle<TuiWindow>), Browser(Handle<BrowserWindow>), Image(Handle<ImageWindow>), Music(Handle<MusicWindow>), Game(Handle<GameWindow>), Help(Handle<HelpWindow>),
}
#[derive(Clone, PartialEq)]
struct WindowInfo { target: WindowTarget, caption: String, visible: bool, shortcut: Option<usize> }
struct WindowTask {
    info: WindowInfo, button: Handle<MenuButton>, menu: Handle<Menu>,
    focus: Handle<Command>, hide: Handle<Command>, close: Handle<Command>, commands: Option<Handle<Command>>,
}
fn taskbar_caption(title: &str) -> String {
    let title = if title.is_empty() { "Window" } else { title };
    let caption = if title.chars().count() > 30 { title.chars().take(29).collect::<String>() + "…" } else { title.to_owned() };
    caption.replace('&', "&&")
}

struct LockVisibility {
    terminals: Vec<(Handle<TuiWindow>, bool)>,
    help: Vec<(Handle<HelpWindow>, bool)>,
    image: Option<(Handle<ImageWindow>, bool)>,
    browser: Option<(Handle<BrowserWindow>, bool)>,
    music: Option<(Handle<MusicWindow>, bool)>,
    games: Vec<(Handle<GameWindow>, bool)>,
}

impl MyDesktop {
    pub fn new(
        shortcuts: Vec<Shortcut>,
        preferences: Preferences,
        settings_path: PathBuf,
        startup_warning: Option<String>,
        runner: Arc<dyn SystemActionRunner>,
    ) -> Self {
        let shortcuts = catalog::with_starters(shortcuts);
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
            application_menu: Handle::None,
            application_commands: Vec::new(),
            additional_commands: Vec::new(),
            visibility_items: vec![Handle::None; shortcuts.len()],
            app_menues: vec![Handle::None; shortcuts.len()],
            window_tasks: Vec::new(), window_order: Vec::new(), help_windows: Vec::new(),
            application_root: Handle::None, apps_menu: Handle::None, background_menu: Handle::None,
            background_entry: Handle::None, window_selectors: Vec::new(),
            app_windows: HashMap::new(),
            status_label: Handle::None,
            image_window: Handle::None,
            browser_window: Handle::None,
            music_window: Handle::None,
            game_windows: HashMap::new(),
            document_windows: Vec::new(),
            status: StatusMonitor::default(),
            apps_button: Handle::None,
            shortcuts,
        }
    }

    pub fn apply_preferences(&mut self, preferences: Preferences) {
        App::set_theme(build_theme(&preferences.desktop));
        for handle in self.terminal_handles() {
            if let Some(window) = self.window_mut(handle) {
                window.apply_palette(&preferences.terminal);
            }
        }
        let image = self.image_window;
        if let Some(window) = self.window_mut(image) { window.apply_background(preferences.desktop.surface); }
        self.preferences = preferences;
        self.refresh_panel();
    }

    fn track_window(&mut self, target: WindowTarget) {
        self.window_order.push(target);
        self.sync_taskbar();
    }
    fn window_info(&self, target: WindowTarget) -> Option<WindowInfo> {
        let (caption, visible, shortcut) = match target {
            WindowTarget::Terminal(handle) => {
                let window = self.windowt(handle)?;
                if window.is_closing() { return None; }
                let shortcut = self.app_windows.iter().find(|(_, value)| **value == handle).map(|(index, _)| *index);
                let caption = shortcut.map_or_else(|| window.title().to_owned(), |index| self.shortcuts[index].name.clone());
                (caption, window.is_visible(), shortcut)
            }
            WindowTarget::Browser(handle) => {
                let window = self.windowt(handle)?;
                if window.is_closing() { return None; }
                ("File Browser".into(), window.is_visible(), None)
            }
            WindowTarget::Image(handle) => {
                let window = self.windowt(handle)?;
                if window.is_closing() { return None; }
                let title = window.title().split(" — ").next().unwrap_or("Image Viewer");
                let caption = if title == "Image Viewer" { title.to_owned() } else { format!("Image Viewer: {title}") };
                (caption, window.is_visible(), None)
            }
            WindowTarget::Music(handle) => {
                let window = self.windowt(handle)?;
                if window.is_closing() { return None; }
                (window.title().to_owned(), window.is_visible(), None)
            }
            WindowTarget::Game(handle) => {
                let window = self.windowt(handle)?;
                if window.is_closing() { return None; }
                (window.title().to_owned(), window.is_visible(), None)
            }
            WindowTarget::Help(handle) => ("Help".into(), self.windowt(handle)?.is_visible(), None),
        };
        Some(WindowInfo { target, caption, visible, shortcut })
    }
    fn focus_window(&mut self, target: WindowTarget) {
        match target {
            WindowTarget::Terminal(handle) => if let Some(window) = self.window_mut(handle) { window.set_visible(true); window.request_focus(); },
            WindowTarget::Browser(handle) => if let Some(window) = self.window_mut(handle) { window.set_visible(true); window.request_focus(); },
            WindowTarget::Image(handle) => if let Some(window) = self.window_mut(handle) { window.set_visible(true); window.request_focus(); },
            WindowTarget::Music(handle) => if let Some(window) = self.window_mut(handle) { window.set_visible(true); window.request_focus(); },
            WindowTarget::Game(handle) => if let Some(window) = self.window_mut(handle) { window.set_visible(true); window.request_focus(); },
            WindowTarget::Help(handle) => if let Some(window) = self.window_mut(handle) { window.set_visible(true); window.request_focus(); },
        }
    }
    fn hide_window(&mut self, target: WindowTarget) {
        match target {
            WindowTarget::Terminal(handle) => if let Some(window) = self.window_mut(handle) { window.set_visible(false); },
            WindowTarget::Browser(handle) => if let Some(window) = self.window_mut(handle) { window.set_visible(false); },
            WindowTarget::Image(handle) => if let Some(window) = self.window_mut(handle) { window.set_visible(false); },
            WindowTarget::Music(handle) => if let Some(window) = self.window_mut(handle) { window.set_visible(false); },
            WindowTarget::Game(handle) => if let Some(window) = self.window_mut(handle) { window.suspend(); window.set_visible(false); },
            WindowTarget::Help(handle) => if let Some(window) = self.window_mut(handle) { window.set_visible(false); },
        }
    }
    fn close_window(&mut self, target: WindowTarget) {
        match target {
            WindowTarget::Terminal(handle) => if let Some(window) = self.window_mut(handle) { window.close_command(); },
            WindowTarget::Browser(handle) => if let Some(window) = self.window_mut(handle) { window.shutdown(); },
            WindowTarget::Image(handle) => if let Some(window) = self.window_mut(handle) { window.shutdown(); },
            WindowTarget::Music(handle) => if let Some(window) = self.window_mut(handle) { window.shutdown(); },
            WindowTarget::Game(handle) => if let Some(window) = self.window_mut(handle) { window.shutdown(); },
            WindowTarget::Help(handle) => if let Some(window) = self.window_mut(handle) { window.close(); },
        }
    }
    fn rebuild_window_menu(&mut self, handle: Handle<Menu>, entries: &[WindowInfo]) {
        if let Some(menu) = self.menu_mut(handle) { menu.clear(); }
        // AppCUI keyboard navigation supports at most128 rows per menu.
        if entries.len() <= 100 {
            for entry in entries {
                let Some(menu) = self.menu_mut(handle) else { return; };
                let item = menu.add(Command::new(&entry.caption.replace('&', "&&"), Key::None, Commands::SelectWindow));
                self.window_selectors.push((item, entry.target));
            }
            return;
        }
        for (page, chunk) in entries.chunks(100).enumerate() {
            let mut menu = Menu::new();
            for entry in chunk {
                let item = menu.add(Command::new(&entry.caption.replace('&', "&&"), Key::None, Commands::SelectWindow));
                self.window_selectors.push((item, entry.target));
            }
            // Register the child before borrowing its parent; registration can
            // grow the menu registry and move the parent's storage.
            let submenu = menu::SubMenu::new(&format!("Windows {}–{}", page * 100 + 1, page * 100 + chunk.len()), menu);
            if let Some(root) = self.menu_mut(handle) { root.add(submenu); }
        }
    }

    fn sync_taskbar(&mut self) -> bool {
        if self.terminal_locked || self.apps_menu.is_none() { return false; }
        let infos: Vec<_> = self.window_order.iter().filter_map(|target| self.window_info(*target)).collect();
        if self.window_tasks.iter().map(|task| &task.info).eq(infos.iter()) { return false; }
        let old = std::mem::take(&mut self.window_tasks);
        for task in old {
            if infos.iter().any(|info| info.target == task.info.target) { self.window_tasks.push(task); }
            else { self.unregister_menu(task.menu); self.appbar().remove(task.button); }
        }
        self.window_order = infos.iter().map(|info| info.target).collect();
        for info in &infos {
            let caption = taskbar_caption(&info.caption);
            if let Some(task) = self.window_tasks.iter_mut().find(|task| task.info.target == info.target) {
                let button = task.button;
                task.info = info.clone();
                if let Some(button) = self.appbar().get_mut(button) { button.set_caption(&caption); }
            } else {
                let mut menu = Menu::new();
                let focus = menu.add(Command::new("Focus", Key::None, Commands::FocusWindow));
                let hide = menu.add(Command::new("Hide (run in background)", Key::None, Commands::HideWindow));
                let close = menu.add(Command::new("Close", Key::None, Commands::CloseWindow));
                let commands = info.shortcut.filter(|index| !self.shortcuts[*index].taskbar.additional_commands.is_empty())
                    .map(|_| menu.add(Command::new("App commands...", Key::None, Commands::WindowCommands)));
                let menu = self.register_menu(menu);
                let button = self.appbar().add(MenuButton::with_handle(&caption, menu, 6, Side::Left));
                self.window_tasks.push(WindowTask { info: info.clone(), button, menu, focus, hide, close, commands });
            }
        }
        self.window_selectors.clear();
        let visible: Vec<_> = infos.iter().filter(|info| info.visible).cloned().collect();
        let hidden: Vec<_> = infos.iter().filter(|info| !info.visible).cloned().collect();
        self.rebuild_window_menu(self.apps_menu, &visible);
        self.rebuild_window_menu(self.background_menu, &hidden);
        let root = self.application_root;
        let item = self.background_entry;
        if let Some(item) = self.menuitem_mut(root, item) { item.set_enabled(!hidden.is_empty()); }
        let status = self.status_label;
        if let Some(label) = self.appbar().get_mut(status) { let caption = label.caption().to_owned(); label.set_caption(&caption); }
        true
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
        let widths = self.window_tasks.iter().filter(|task| task.info.visible)
            .map(|task| taskbar_caption(&task.info.caption).chars().count() + 2).collect::<Vec<_>>();
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

    fn terminal_handles(&self) -> Vec<Handle<TuiWindow>> {
        self.app_windows.values().copied().chain(self.document_windows.iter().copied()).collect()
    }
    fn hide_for_terminal_lock(&mut self) -> LockVisibility {
        self.terminal_locked = true;
        let mut visibility = LockVisibility { terminals: Vec::new(), help: Vec::new(), image: None, browser: None, music: None, games: Vec::new() };
        for handle in self.terminal_handles() {
            if let Some(window) = self.window_mut(handle) {
                visibility.terminals.push((handle, window.is_visible()));
                window.set_visible(false);
            }
        }
        let image = self.image_window;
        if let Some(window) = self.window_mut(image) {
            visibility.image = Some((image, window.is_visible()));
            window.set_visible(false);
        }
        let browser = self.browser_window;
        if let Some(window) = self.window_mut(browser) {
            visibility.browser = Some((browser, window.is_visible()));
            window.set_visible(false);
        }
        let music = self.music_window;
        if let Some(window) = self.window_mut(music) {
            visibility.music = Some((music, window.is_visible()));
            window.set_visible(false);
        }
        for handle in self.game_windows.values().copied().collect::<Vec<_>>() {
            if let Some(window) = self.window_mut(handle) {
                visibility.games.push((handle, window.is_visible()));
                window.suspend();
                window.set_visible(false);
            }
        }
        for handle in self.help_windows.clone() {
            if let Some(window) = self.window_mut(handle) { visibility.help.push((handle, window.is_visible())); window.set_visible(false); }
        }
        visibility
    }
    fn restore_after_terminal_unlock(&mut self, visibility: LockVisibility) {
        for (handle, visible) in visibility.terminals {
            if let Some(window) = self.window_mut(handle) { window.set_visible(visible); }
        }
        if let Some((handle, visible)) = visibility.image
            && let Some(window) = self.window_mut(handle) { window.set_visible(visible); }
        if let Some((handle, visible)) = visibility.browser
            && let Some(window) = self.window_mut(handle) { window.set_visible(visible); }
        if let Some((handle, visible)) = visibility.music
            && let Some(window) = self.window_mut(handle) { window.set_visible(visible); }
        for (handle, visible) in visibility.games { if let Some(window) = self.window_mut(handle) { window.set_visible(visible); } }
        for (handle, visible) in visibility.help { if let Some(window) = self.window_mut(handle) { window.set_visible(visible); } }
        self.terminal_locked = false;
        self.sync_taskbar();
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

    fn open_image(&mut self, path: Option<PathBuf>) {
        let handle = self.image_window;
        if let Some(window) = self.window_mut(handle)
            && !window.is_closing() {
            if let Some(path) = path { window.open(path); }
            window.set_visible(true);
            window.request_focus();
            return;
        }
        let Some(path) = path.or_else(native::select_image) else { return; };
        match ImageWindow::new(path, self.preferences.desktop.surface) {
            Ok(window) => { self.image_window = self.add_window(window); self.track_window(WindowTarget::Image(self.image_window)); }
            Err(error) => dialogs::error("Cannot open Image Viewer", &format!("{error:#}")),
        }
    }
    fn open_browser(&mut self) {
        let handle = self.browser_window;
        if let Some(window) = self.window_mut(handle)
            && !window.is_closing() {
            window.set_visible(true);
            window.request_focus();
            return;
        }
        match BrowserWindow::new() {
            Ok(window) => { self.browser_window = self.add_window(window); self.track_window(WindowTarget::Browser(self.browser_window)); }
            Err(error) => dialogs::error("Cannot open File Browser", &format!("{error:#}")),
        }
    }
    fn open_music(&mut self) {
        let handle = self.music_window;
        if let Some(window) = self.window_mut(handle)
            && !window.is_closing() {
            window.set_visible(true);
            window.request_focus();
            return;
        }
        match MusicWindow::new() {
            Ok(window) => { self.music_window = self.add_window(window); self.track_window(WindowTarget::Music(self.music_window)); }
            Err(error) => dialogs::error("Cannot open Music Player", &format!("{error:#}")),
        }
    }
    fn open_document(&mut self, path: PathBuf) -> anyhow::Result<()> {
        let editor = self.shortcuts.iter().find(|shortcut| shortcut.name == "Text editor")
            .ok_or_else(|| anyhow::anyhow!("Configure a Text editor shortcut to open this file."))?;
        let window = TuiWindow::new_with_document(
            &format!("Text editor — {}", path.file_name().unwrap_or(path.as_os_str()).to_string_lossy()),
            editor.command.clone(), editor.args.clone(), editor.window.clone(), editor.terminal.clone(),
            &self.preferences.terminal, Some(&path),
        )?;
        let handle = self.add_window(window);
        self.document_windows.push(handle);
        self.track_window(WindowTarget::Terminal(handle));
        Ok(())
    }

    fn open_game(&mut self, kind: GameKind) {
        if let Some(handle) = self.game_windows.get(&kind).copied()
            && let Some(window) = self.window_mut(handle)
            && !window.is_closing() {
            window.set_visible(true);
            window.request_focus();
            return;
        }
        match GameWindow::new(kind) {
            Ok(mut window) => {
                window.constrain_to(self.desktop_rect().size());
                let handle = self.add_window(window);
                self.game_windows.insert(kind, handle);
                self.track_window(WindowTarget::Game(handle));
            }
            Err(error) => dialogs::error(&format!("Cannot open {}", kind.name()), &format!("{error:#}")),
        }
    }

    fn launch_application(&mut self, id: AppId) {
        match id {
            AppId::External(index) => {
                let shortcut = self.shortcuts[index].clone();
                self.launch_external(index, shortcut.command, shortcut.args);
            }
            AppId::ImageViewer => self.open_image(None),
            AppId::FileBrowser => self.open_browser(),
            AppId::MusicPlayer => self.open_music(),
            AppId::Game(kind) => self.open_game(kind),
        }
    }

    fn update_visibility_caption(&mut self, index: usize) {
        if let (Some(menu), Some(item)) = (self.app_menues.get(index).copied(), self.visibility_items.get(index).copied())
            && let Some(item) = self.menuitem_mut(menu, item) { item.set_caption("Hide"); }
    }
    fn launch_external(&mut self, index: usize, command: String, args: Vec<String>) {
        if let Some(handle) = self.app_windows.get(&index).copied()
            && let Some(window) = self.window_mut(handle)
            && !window.is_closing() {
            window.set_visible(true);
            window.request_focus();
            self.update_visibility_caption(index);
            return;
        }
        if let Err(error) = self.create_window(index, command, args) {
            dialogs::error("Unable to launch application", &format!("{error:#}"));
        } else { self.update_visibility_caption(index); }
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
        self.track_window(WindowTarget::Terminal(win_handle));

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
        let available = self.desktop_rect().size();
        for handle in self.game_windows.values().copied().collect::<Vec<_>>() {
            if let Some(window) = self.window_mut(handle) { window.constrain_to(available); }
        }
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

        let apps = catalog::applications(&shortcuts);
        let mut application_menu = Menu::new();
        for category in catalog::categories(&apps) {
            let mut submenu = Menu::new();
            let entries: Vec<_> = apps.iter().filter(|app| app.category == category).collect();
            if entries.is_empty() {
                let mut empty = Command::new("No apps yet", Key::None, Commands::None);
                empty.set_enabled(false);
                submenu.add(empty);
            }
            for app in entries {
                let item = submenu.add(Command::new(&app.name.replace('&', "&&"), Key::None, Commands::LaunchApplication));
                self.application_commands.push((item, app.id));
            }
            application_menu.add(menu::SubMenu::new(&category.replace('&', "&&"), submenu));
        }
        application_menu.add(menu::Separator::new());
        let mut background = Command::new("Background windows", Key::None, Commands::BackgroundWindows);
        background.set_enabled(false);
        self.background_entry = application_menu.add(background);
        self.application_root = self.register_menu(application_menu);
        let root = self.application_root;
        self.application_menu = self.appbar().add(MenuButton::with_handle("Application", root, 2, Side::Left));

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
                .add(MenuButton::new("Tilling", tilling_menu, 3, Side::Left));

        let separator = self.appbar().add(appbar::Separator::new(4, Side::Left));

        let mut app_menues = vec![Handle::<Menu>::None; shortcuts.len()];
        for (index, shortcut) in shortcuts.iter().enumerate() {
            let mut menu = Menu::new();

            self.visibility_items[index] = menu.add(Command::new(
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
                let item = menu.add(Command::new(&command.name.replace('&', "&&"), Key::None, Commands::AppCommand));
                self.additional_commands.push((item, index, command.command.clone(), command.args.clone()));
            }

            app_menues[index] = self.register_menu(menu);
        }
        self.apps_menu = self.register_menu(Menu::new());
        self.background_menu = self.register_menu(Menu::new());
        let apps = self.apps_menu;
        self.apps_button = self.appbar().add(MenuButton::with_handle("Apps", apps, 5, Side::Left));

        self.desktop_menu = desktop_menu_button;
        self.arrange_menu = arrange_menu_button;
        self.separator = separator;
        self.app_menues = app_menues;
        self.sync_taskbar();
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
        app_bar.show(self.application_menu);
        app_bar.show(self.arrange_menu);
        app_bar.show(self.separator);

        if layout.grouped_apps {
            app_bar.show(self.apps_button);
        } else {
            for task in self.window_tasks.iter().filter(|task| task.info.visible) { app_bar.show(task.button); }
        }
    }
}

impl MenuEvents for MyDesktop {
    fn on_menu_open(&self, menu: &mut Menu) {
        if self.terminal_locked { return; }
        if let Some(task) = self.window_tasks.iter().find(|task| menu.get(task.focus).is_some()) {
            match task.info.target {
                WindowTarget::Terminal(handle) => Window::update_control(handle, |window| { window.request_focus(); }),
                WindowTarget::Browser(handle) => Window::update_control(handle, |window| { window.request_focus(); }),
                WindowTarget::Image(handle) => Window::update_control(handle, |window| { window.request_focus(); }),
                WindowTarget::Music(handle) => Window::update_control(handle, |window| { window.request_focus(); }),
                WindowTarget::Game(handle) => Window::update_control(handle, |window| { window.request_focus(); }),
                WindowTarget::Help(handle) => Window::update_control(handle, |window| { window.request_focus(); }),
            }
        }
    }

    fn on_command(&mut self, menu: Handle<Menu>, item: Handle<Command>, command: Commands) {
        if self.terminal_locked {
            return;
        }
        match command {
            Commands::LaunchApplication => {
                if let Some(id) = self.application_commands.iter().find(|entry| entry.0 == item).map(|entry| entry.1) {
                    self.launch_application(id);
                }
            }
            Commands::SelectWindow => {
                if let Some(target) = self.window_selectors.iter().find(|entry| entry.0 == item).map(|entry| entry.1) {
                    let actions = self.window_tasks.iter().find(|task| task.info.target == target && task.info.visible).map(|task| task.menu);
                    self.focus_window(target);
                    if let Some(menu) = actions { self.show_menu(menu, 0, 1, None); }
                }
            }
            Commands::BackgroundWindows => self.show_menu(self.background_menu, 10, 1, None),
            Commands::FocusWindow | Commands::HideWindow | Commands::CloseWindow | Commands::WindowCommands => {
                if let Some(info) = self.window_tasks.iter().find(|task| task.menu == menu &&
                    (task.focus == item || task.hide == item || task.close == item || task.commands == Some(item))).map(|task| task.info.clone()) {
                    match command {
                        Commands::FocusWindow => self.focus_window(info.target),
                        Commands::HideWindow => self.hide_window(info.target),
                        Commands::CloseWindow => self.close_window(info.target),
                        Commands::WindowCommands => if let Some(index) = info.shortcut { self.show_menu(self.app_menues[index], 0, 1, None); },
                        _ => {}
                    }
                }
            }
            Commands::Help => {
                let handle = self.add_window(help_window());
                self.help_windows.push(handle);
                self.track_window(WindowTarget::Help(handle));
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
                for handle in self.terminal_handles() {
                    if let Some(win) = self.window_mut(handle) {
                        win.close_command();
                    }
                }

                let handle = self.image_window;
                if let Some(window) = self.window_mut(handle) { window.shutdown(); }
                let handle = self.browser_window;
                if let Some(window) = self.window_mut(handle) { window.shutdown(); }
                let handle = self.music_window;
                if let Some(window) = self.window_mut(handle) { window.shutdown(); }
                for handle in self.game_windows.values().copied().collect::<Vec<_>>() {
                    if let Some(window) = self.window_mut(handle) { window.shutdown(); }
                }
                self.close();
                return;
            }
            Commands::OpenApp
            | Commands::CloseApp
            | Commands::AppVisibilityToggle
            | Commands::AppCommand => {
                let Some(index) = self.app_menues.iter().position(|handle| *handle == menu) else { return; };
                let handle = self.app_windows.get(&index).copied().unwrap_or(Handle::None);
                match command {
                    Commands::OpenApp => self.launch_application(AppId::External(index)),
                    Commands::CloseApp => {
                        if let Some(window) = self.window_mut(handle) { window.close_command(); }
                        self.app_windows.remove(&index);
                    }
                    Commands::AppVisibilityToggle => {
                        if let Some(window) = self.window_mut(handle) {
                            let visible = !window.is_visible();
                            window.set_visible(visible);
                            if visible { window.request_focus(); }
                            if let Some(item) = self.menuitem_mut(menu, item) { item.set_caption(if visible { "Hide" } else { "Show" }); }
                        }
                    }
                    Commands::AppCommand => {
                        // Command handles, rather than captions, select additional commands.
                        let extra = self.additional_commands.iter().find(|entry| entry.0 == item).cloned();
                        if let Some((_, _, command, args)) = extra {
                            let shortcut = self.shortcuts[index].clone();
                            match TuiWindow::new(&shortcut.name, command, args, shortcut.window, shortcut.terminal, &self.preferences.terminal) {
                                Ok(window) => { let handle = self.add_window(window); self.document_windows.push(handle); self.track_window(WindowTarget::Terminal(handle)); }
                                Err(error) => dialogs::error("Unable to launch command", &format!("{error:#}")),
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        self.sync_taskbar();
        self.refresh_panel();
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
        let handle = self.browser_window;
        let open_path = self.window_mut(handle).and_then(BrowserWindow::take_open_path);
        if let Some(path) = open_path {
            if crate::runtime::files::is_image(&path) { self.open_image(Some(path)); }
            else if let Err(error) = self.open_document(path) { dialogs::error("Cannot open file", &format!("{error:#}")); }
        }
        let documents = std::mem::take(&mut self.document_windows);
        self.document_windows = documents.into_iter().filter(|handle| self.windowt(*handle).is_some()).collect();
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
        let mut changed = self.sync_taskbar();
        changed |= self.refresh_panel();
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
    fn games_launch_restore_lock_and_close_as_independent_native_windows() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let mut desktop = MyDesktop::new(vec![], Preferences::default(), dir.path().join("settings.toml"), None, Arc::new(NoSystemActions));
        let app = App::debug(80,24,"Paint.Enable(false)").app_bar().timers_count(32).build().unwrap();
        DesktopEvents::on_start(&mut desktop);
        for kind in catalog::GameKind::ALL {
            desktop.launch_application(AppId::Game(kind));
            assert_eq!(desktop.window_tasks.len(), 1);
            let info = desktop.window_tasks[0].info.clone();
            assert_eq!(info.caption, kind.name());
            assert!(info.visible && info.shortcut.is_none());
            desktop.hide_window(info.target);
            desktop.sync_taskbar();
            assert!(!desktop.window_info(info.target).unwrap().visible);
            let visibility = desktop.hide_for_terminal_lock();
            desktop.restore_after_terminal_unlock(visibility);
            assert!(!desktop.window_info(info.target).unwrap().visible);
            let command = desktop.application_commands.iter().find(|entry| entry.1 == AppId::Game(kind)).unwrap().0;
            MenuEvents::on_command(&mut desktop, Handle::None, command, Commands::LaunchApplication);
            assert_eq!(desktop.window_tasks.len(), 1);
            assert_eq!(desktop.window_tasks[0].info.target, info.target);
            assert!(desktop.window_info(info.target).unwrap().visible);
            let visibility = desktop.hide_for_terminal_lock();
            assert!(!desktop.window_info(info.target).unwrap().visible);
            desktop.restore_after_terminal_unlock(visibility);
            assert!(desktop.window_info(info.target).unwrap().visible);
            desktop.close_window(info.target);
            desktop.sync_taskbar();
            assert!(desktop.window_tasks.is_empty());
            desktop.launch_application(AppId::Game(kind));
            assert_ne!(desktop.window_tasks[0].info.target, info.target);
            let target = desktop.window_tasks[0].info.target;
            desktop.close_window(target);
            desktop.sync_taskbar();
        }
        for kind in catalog::GameKind::ALL { desktop.launch_application(AppId::Game(kind)); }
        assert_eq!(desktop.window_tasks.len(), 3);
        let targets = desktop.window_order.clone();
        MenuEvents::on_command(&mut desktop, Handle::None, Handle::None, Commands::Exit);
        assert!(targets.into_iter().all(|target| desktop.window_info(target).is_none()));
        app.run();
    }
    #[test]
    fn narrow_overflow_keeps_hide_and_close_actions_reachable() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "ui::desktop::view::tests::resizing_regroups_and_restores_apps_with_panel_enabled_or_disabled", "--nocapture"])
            .env("DESKTOP_TUI_OVERFLOW_ACTIONS", "1").output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let raw = String::from_utf8_lossy(&output.stdout);
        let mut text = String::new();
        let mut chars = raw.chars();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' { for next in chars.by_ref() { if next.is_ascii_alphabetic() { break; } } }
            else { text.push(ch); }
        }
        assert_eq!(text.matches("Hide (run in background)").count(), 2, "overflow must expose Hide for both panel states");
        assert!(text.contains("Close"));
    }
    #[test]
    fn large_window_lists_keep_registered_roots_and_all_selectors() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let mut desktop = MyDesktop::new(vec![], Preferences::default(), dir.path().join("settings.toml"), None, Arc::new(NoSystemActions));
        let app = App::debug(80,24,"Paint.Enable(false)").app_bar().build().unwrap();
        DesktopEvents::on_start(&mut desktop);
        let root = desktop.apps_menu;
        let entries: Vec<_> = (0..201).map(|index| WindowInfo { target: WindowTarget::Terminal(Handle::None), caption: format!("Document {index}"), visible: true, shortcut: None }).collect();
        desktop.rebuild_window_menu(root, &entries);
        assert!(desktop.menu_mut(root).is_some());
        assert_eq!(desktop.window_selectors.len(), 201);
        let first = desktop.window_selectors[0].0;
        desktop.window_selectors.clear();
        desktop.rebuild_window_menu(root, &entries);
        assert!(desktop.menu_mut(root).is_some());
        assert_eq!(desktop.window_selectors.len(), 201);
        assert!(desktop.window_selectors[0].0 != first);
        app.run();
    }
    #[test]
    fn taskbar_tracks_visible_native_windows_and_preserves_background_instances() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let mut desktop = MyDesktop::new(vec![], Preferences::default(), dir.path().join("settings.toml"), None, Arc::new(NoSystemActions));
        let app = App::debug(80,24,"Paint.Enable(false)").app_bar().timers_count(32).build().unwrap();
        DesktopEvents::on_start(&mut desktop);
        assert!(desktop.window_tasks.is_empty(), "unopened starters must not occupy the taskbar");
        desktop.open_browser();
        desktop.sync_taskbar();
        let browser = desktop.browser_window;
        assert_eq!(desktop.window_tasks.len(), 1);
        let hide = desktop.window_tasks[0].hide;
        let menu = desktop.window_tasks[0].menu;
        let button = desktop.window_tasks[0].button;
        MenuEvents::on_command(&mut desktop, menu, hide, Commands::HideWindow);
        assert!(!desktop.window_tasks[0].info.visible);
        assert!(!desktop.window_mut(browser).unwrap().is_closing());
        assert_eq!(desktop.window_selectors.len(), 1);
        let restore = desktop.window_selectors[0].0;
        let background = desktop.background_menu;
        MenuEvents::on_command(&mut desktop, background, restore, Commands::SelectWindow);
        assert!(desktop.window_tasks[0].info.visible);
        assert!(desktop.browser_window == browser);
        let close = desktop.window_tasks[0].close;
        MenuEvents::on_command(&mut desktop, menu, close, Commands::CloseWindow);
        assert!(desktop.window_tasks.is_empty());
        assert!(desktop.appbar().get(button).is_none());
        assert!(desktop.menu_mut(menu).is_none());
        MenuEvents::on_command(&mut desktop, Handle::None, Handle::None, Commands::Exit);
        app.run();
    }
    #[test]
    fn application_dispatch_restores_native_windows_and_mixed_lock_visibility() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _entered = runtime.enter();
        let dir = TestDir::new();
        let image = dir.path().join("image.png");
        ::image::RgbaImage::from_pixel(4, 6, ::image::Rgba([20,40,60,128])).save(&image).unwrap();
        let mut desktop = MyDesktop::new(vec![], Preferences::default(), dir.path().join("settings.toml"), None, Arc::new(NoSystemActions));
        let app = App::debug(80,24,"Paint.Enable(false)").app_bar().timers_count(32).build().unwrap();
        DesktopEvents::on_start(&mut desktop);
        assert_eq!(desktop.application_commands.len(), 9);
        let browser_command = desktop.application_commands.iter().find(|entry| entry.1 == AppId::FileBrowser).unwrap().0;
        MenuEvents::on_command(&mut desktop, Handle::None, browser_command, Commands::LaunchApplication);
        let browser = desktop.browser_window;
        desktop.window_mut(browser).unwrap().set_visible(false);
        MenuEvents::on_command(&mut desktop, Handle::None, browser_command, Commands::LaunchApplication);
        assert!(desktop.browser_window == browser);
        assert!(desktop.window_mut(browser).unwrap().is_visible());
        desktop.open_image(Some(image.clone()));
        let viewer = desktop.image_window;
        desktop.launch_application(AppId::ImageViewer);
        assert!(desktop.image_window == viewer);
        let music_command = desktop.application_commands.iter().find(|entry| entry.1 == AppId::MusicPlayer).unwrap().0;
        MenuEvents::on_command(&mut desktop, Handle::None, music_command, Commands::LaunchApplication);
        let music = desktop.music_window;
        desktop.hide_window(WindowTarget::Music(music));
        desktop.launch_application(AppId::MusicPlayer);
        assert!(desktop.music_window == music);
        assert!(desktop.window_mut(music).unwrap().is_visible());
        desktop.window_mut(viewer).unwrap().set_visible(false);
        let visibility = desktop.hide_for_terminal_lock();
        assert!(!desktop.window_mut(browser).unwrap().is_visible());
        assert!(!desktop.window_mut(viewer).unwrap().is_visible());
        assert!(!desktop.window_mut(music).unwrap().is_visible());
        let mut preferences = desktop.preferences.clone();
        preferences.desktop.surface = Rgb(1,2,3);
        desktop.apply_preferences(preferences);
        desktop.restore_after_terminal_unlock(visibility);
        assert!(desktop.window_mut(browser).unwrap().is_visible());
        assert!(!desktop.window_mut(viewer).unwrap().is_visible());
        assert!(desktop.window_mut(music).unwrap().is_visible());
        desktop.hide_window(WindowTarget::Music(music));
        let visibility = desktop.hide_for_terminal_lock();
        desktop.restore_after_terminal_unlock(visibility);
        assert!(!desktop.window_mut(music).unwrap().is_visible());
        desktop.focus_window(WindowTarget::Music(music));
        desktop.arrange_windows(desktop::ArrangeWindowsMethod::Grid);
        assert!(matches!(WindowEvents::on_cancel(desktop.window_mut(music).unwrap()), ActionRequest::Allow));
        desktop.launch_application(AppId::MusicPlayer);
        assert!(desktop.music_window != music);
        desktop.window_mut(viewer).unwrap().shutdown();
        desktop.open_image(Some(image));
        assert!(desktop.image_window != viewer);
        desktop.window_mut(browser).unwrap().shutdown();
        assert!(desktop.window_mut(browser).is_none_or(|window| window.is_closing()));
        desktop.open_browser();
        let new_browser = desktop.browser_window;
        let closing = desktop.window_mut(new_browser).map(|window| window.is_closing());
        assert!(new_browser != browser, "old: {browser:?}, new: {new_browser:?}, closing: {closing:?}");
        MenuEvents::on_command(&mut desktop, Handle::None, Handle::None, Commands::Exit);
        app.run();
        runtime.block_on(crate::runtime::processes::finish_shutdown());
    }
    #[test]
    fn document_windows_preserve_existing_editor_and_pass_opaque_os_argument() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let _ui_lock = UI_LOCK.lock().unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _entered = runtime.enter();
        let dir = TestDir::new();
        let mut name = "space 文 <FILE_PATH> ".as_bytes().to_vec();
        name.push(0xff);
        let path = dir.path().join(std::ffi::OsString::from_vec(name));
        std::fs::write(&path, b"document").unwrap();
        let capture = dir.path().join("argv");
        let mut editor: Shortcut = toml::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shortcuts/helix.toml"))).unwrap();
        editor.command = "/bin/sh".into();
        editor.args = vec!["-c".into(), "if [ $# -ge 2 ]; then printf '%s' \"$2\" > \"$1\"; fi; printf A; read answer".into(), "editor".into(), capture.to_string_lossy().into_owned()];
        let mut desktop = MyDesktop::new(vec![editor], Preferences::default(), dir.path().join("settings.toml"), None, Arc::new(NoSystemActions));
        let app = App::debug(80,24,"Paint.Enable(false)").app_bar().timers_count(32).build().unwrap();
        DesktopEvents::on_start(&mut desktop);
        desktop.launch_application(AppId::External(0));
        let original = desktop.app_windows[&0];
        pump_until(desktop.window_mut(original).unwrap(), 'A');
        desktop.window_mut(original).unwrap().set_visible(false);
        desktop.launch_application(AppId::External(0));
        assert!(desktop.app_windows[&0] == original);
        assert!(desktop.window_mut(original).unwrap().is_visible());
        desktop.open_document(path.clone()).unwrap();
        desktop.open_document(path.clone()).unwrap();
        let documents = desktop.document_windows.clone();
        assert_eq!(documents.len(), 2);
        desktop.sync_taskbar();
        assert_eq!(desktop.window_tasks.len(), 3);
        let document_task = desktop.window_tasks.iter().find(|task| task.info.target == WindowTarget::Terminal(documents[1])).unwrap();
        let (menu, hide) = (document_task.menu, document_task.hide);
        MenuEvents::on_command(&mut desktop, menu, hide, Commands::HideWindow);
        assert!(!desktop.window_mut(documents[1]).unwrap().is_visible());
        assert!(!desktop.window_mut(documents[1]).unwrap().is_closing());
        let restore = desktop.window_selectors.iter().find(|entry| entry.1 == WindowTarget::Terminal(documents[1])).unwrap().0;
        let background = desktop.background_menu;
        MenuEvents::on_command(&mut desktop, background, restore, Commands::SelectWindow);
        assert!(desktop.window_mut(documents[1]).unwrap().is_visible());
        for handle in &documents { pump_until(desktop.window_mut(*handle).unwrap(), 'A'); }
        assert_eq!(std::fs::read(capture).unwrap(), path.as_os_str().as_bytes());
        let visibility = desktop.hide_for_terminal_lock();
        for handle in &documents { assert!(!desktop.window_mut(*handle).unwrap().is_visible()); }
        desktop.restore_after_terminal_unlock(visibility);
        for handle in &documents { assert!(desktop.window_mut(*handle).unwrap().is_visible()); }
        WindowEvents::on_cancel(desktop.window_mut(documents[0]).unwrap());
        assert!(!desktop.window_mut(original).unwrap().is_closing());
        MenuEvents::on_command(&mut desktop, Handle::None, Handle::None, Commands::Exit);
        app.run();
        runtime.block_on(crate::runtime::processes::finish_shutdown());
    }
    #[test]
    fn resizing_regroups_and_restores_apps_with_panel_enabled_or_disabled() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let _entered = runtime.enter();
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
        // Launch three quiet windows and verify grouping, status space, and
        // restoration across wide/narrow/wide terminal sizes.
        for (enabled, wide, narrow) in [
            (false, 0x5831026DC2ABAA4Bu64, 0x3BC120EFAF880B16u64),
            (true, 0xF1D933FAA1821C65u64, 0x7208F3EA06BF6EB0u64),
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
                shortcut.category = Some("User".into());
                shortcut.command = "/bin/sh".into();
                shortcut.args = vec!["-c".into(), "read answer".into()];
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
            let mut launch = String::from("Paint.Enable(false)\n");
            for index in 0..3 {
                launch.push_str("Mouse.Click(15,0,left)\nKey.Pressed(Home)\nKey.Pressed(Down)\nKey.Pressed(Down)\nKey.Pressed(Right)\nKey.Pressed(Home)\n");
                for _ in 0..index { launch.push_str("Key.Pressed(Down)\n"); }
                launch.push_str("Key.Pressed(Enter)\n");
            }
            let overflow_actions = if std::env::var_os("DESKTOP_TUI_OVERFLOW_ACTIONS").is_some() {
                "Mouse.Click(35,0,left)\nKey.Pressed(Home)\nKey.Pressed(Enter)\nPaint('taskbar-actions')\nKey.Pressed(Escape)\nMouse.Click(15,0,left)\nKey.Pressed(Home)\nKey.Pressed(Down)\nKey.Pressed(Down)\nKey.Pressed(Right)\nKey.Pressed(Home)\nKey.Pressed(Down)\nKey.Pressed(Down)\nKey.Pressed(Enter)\n"
            } else { "" };
            let script = format!("{launch}Paint.Enable(true)\nPaint('initial')\nCheckHash({wide:#X})\nResize(80,24)\nPaint('narrow')\nCheckHash({narrow:#X})\n{overflow_actions}Resize(200,24)\nPaint('wide restored')\nCheckHash({wide:#X})");
            let app = App::debug(200, 24, &script)
                .desktop(desktop)
                .app_bar()
                .build()
                .unwrap();
            app.run();
            runtime.block_on(crate::runtime::processes::finish_shutdown());
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
