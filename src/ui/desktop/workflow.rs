use super::*;
use crate::ui::desktop::palette::{CommandPalette, PaletteResult, ShortcutDialog};
use crate::ui::desktop::launchers::{LauncherManager, DiagnosticsDialog};

enum PaletteAction {
    Launch(AppId), Focus(WindowTarget), Hide(WindowTarget), Close(WindowTarget),
    NewTerminal, Preference, Help, Shortcut, Launchers, Diagnostics, Activity, Terminal(TerminalAction),
    Workspace(u8), Move(WindowTarget,u8), Restore,
}

impl MyDesktop {
    pub(super) fn association_key(path: &std::path::Path) -> Option<String> {
        path.extension().and_then(|extension| extension.to_str()).filter(|extension| !extension.is_empty()).map(str::to_lowercase)
    }
    pub(super) fn file_association(&self, path: &std::path::Path) -> Option<AppId> {
        let key = Self::association_key(path)?;
        let identity = self.preferences.workflow.file_associations.get(&key)?;
        match identity.as_str() {
            "native:kanban" => Some(AppId::Kanban), "native:image" => Some(AppId::ImageViewer), "native:pdf" => Some(AppId::PdfReader),
            "native:music" => Some(AppId::MusicPlayer), "native:paint" => Some(AppId::AsciiPaint),
            value => value.strip_prefix("external:").and_then(|id| self.shortcuts.iter().enumerate().find(|(index,shortcut)| self.launcher_enabled[*index] && shortcut.stable_id() == id).map(|(index,_)| AppId::External(index))),
        }
    }
    pub(super) fn remember_association(&mut self, path: &std::path::Path, id: AppId) {
        let Some(key) = Self::association_key(path) else { return; };
        let identity = match id {
            AppId::Kanban => "native:kanban".to_owned(), AppId::ImageViewer => "native:image".to_owned(), AppId::PdfReader => "native:pdf".to_owned(),
            AppId::MusicPlayer => "native:music".to_owned(), AppId::AsciiPaint => "native:paint".to_owned(),
            AppId::External(index) => format!("external:{}",self.shortcuts[index].stable_id()), _ => return,
        };
        let mut preferences = self.preferences.clone(); preferences.workflow.file_associations.insert(key,identity);
        match preferences.save(&self.settings_path,Some(&self.preferences.terminal_lock)) {
            Ok(()) => self.preferences = preferences,
            Err(error) => dialogs::error("Default application was not saved",&format!("{error:#}")),
        }
    }
    pub(super) fn target_base(&self, target: WindowTarget) -> Option<&Window> {
        match target {
            WindowTarget::Terminal(h) => self.windowt(h).map(|w| &**w),
            WindowTarget::Transfer(h) => self.windowt(h).map(|w| &**w),
            WindowTarget::Browser(h) => self.windowt(h).map(|w| &**w),
            WindowTarget::Image(h) => self.windowt(h).map(|w| &**w),
            WindowTarget::Music(h) => self.windowt(h).map(|w| &**w),
            WindowTarget::AiChat(h) => self.windowt(h).map(|w| &**w),
            WindowTarget::Kanban(h) => self.windowt(h).map(|w| &**w),
            WindowTarget::Paint(h) => self.windowt(h).map(|w| &**w),
            WindowTarget::Pdf(h) => self.windowt(h).map(|w| &**w),
            WindowTarget::Game(h) => self.windowt(h).map(|w| &**w),
            WindowTarget::Help(h) => self.windowt(h).map(|w| &**w),
        }
    }
    pub(super) fn target_base_mut(&mut self, target: WindowTarget) -> Option<&mut Window> {
        match target {
            WindowTarget::Terminal(h) => self.window_mut(h).map(|w| &mut **w),
            WindowTarget::Transfer(h) => self.window_mut(h).map(|w| &mut **w),
            WindowTarget::Browser(h) => self.window_mut(h).map(|w| &mut **w),
            WindowTarget::Image(h) => self.window_mut(h).map(|w| &mut **w),
            WindowTarget::Music(h) => self.window_mut(h).map(|w| &mut **w),
            WindowTarget::AiChat(h) => self.window_mut(h).map(|w| &mut **w),
            WindowTarget::Kanban(h) => self.window_mut(h).map(|w| &mut **w),
            WindowTarget::Paint(h) => self.window_mut(h).map(|w| &mut **w),
            WindowTarget::Pdf(h) => self.window_mut(h).map(|w| &mut **w),
            WindowTarget::Game(h) => self.window_mut(h).map(|w| &mut **w),
            WindowTarget::Help(h) => self.window_mut(h).map(|w| &mut **w),
        }
    }
    pub(super) fn active_target(&self) -> Option<WindowTarget> {
        let active = self.active_window_handle()?;
        self.window_order.iter().copied().find(|target| self.target_base(*target).is_some_and(|window| window.handle() == active))
    }
    pub(super) fn show_palette(&mut self) {
        if self.terminal_locked { return; }
        let origin = self.active_target();
        let mut labels = Vec::new(); let mut actions = Vec::new();
        let mut push = |label: String, action| { labels.push(label); actions.push(action); };
        push("New terminal".into(),PaletteAction::NewTerminal);
        for workspace in 0..crate::config::session::WORKSPACE_COUNT {
            push(format!("Switch to workspace {}{}",workspace+1,if workspace == self.active_workspace { " (current)" } else { "" }),PaletteAction::Workspace(workspace));
            if let Some(target) = origin { push(format!("Move current window to workspace {}",workspace+1),PaletteAction::Move(target,workspace)); }
        }
        push("Restore saved workspace".into(),PaletteAction::Restore);
        for app in catalog::applications(&self.shortcuts) {
            if let AppId::External(index) = app.id && !self.launcher_enabled[index] { continue; }
            push(format!("Launch: {} ({})",app.name,app.category),PaletteAction::Launch(app.id));
        }
        for target in self.window_order.iter().copied() {
            if let Some(info) = self.window_info(target) {
                push(format!("Focus: {}{}",info.caption,if info.visible { "" } else { " (hidden)" }),PaletteAction::Focus(target));
                push(format!("Hide: {}",info.caption),PaletteAction::Hide(target));
                push(format!("Close: {}",info.caption),PaletteAction::Close(target));
            }
        }
        if matches!(origin,Some(WindowTarget::Terminal(_))) {
            for (label,action) in [("Terminal: Copy mode",TerminalAction::CopyMode),("Terminal: Search history",TerminalAction::Search),("Terminal: Paste",TerminalAction::Paste),("Terminal: Return to live",TerminalAction::Live)] {
                push(label.into(),PaletteAction::Terminal(action));
            }
        }
        push("Preferences".into(),PaletteAction::Preference); push("Help".into(),PaletteAction::Help);
        push(format!("Configure desktop shortcut ({})",self.preferences.workflow.palette_shortcut.label()),PaletteAction::Shortcut); push("Manage launchers".into(),PaletteAction::Launchers);
        push("Diagnostics".into(),PaletteAction::Diagnostics);
        let unread = self.activity.unread();
        let activity = if unread > 0 { format!("Activity ({unread} unread)") } else { "Activity".into() };
        push(activity,PaletteAction::Activity);
        match CommandPalette::new(labels,self.preferences.workflow.palette_shortcut.key()).show() {
            Some(PaletteResult::ForwardPrefix) => if let Some(WindowTarget::Terminal(handle)) = origin {
                let key = self.preferences.workflow.palette_shortcut.key();
                if let Some(bytes) = crate::ui::terminal::encode_desktop_key(key)
                    && let Some(window) = self.window_mut(handle) { window.send_input(bytes); }
            },
            Some(PaletteResult::Selected(index)) => if let Some(action) = actions.into_iter().nth(index) {
                match action {
                    PaletteAction::NewTerminal => self.new_terminal(),
                    PaletteAction::Workspace(workspace) => self.switch_workspace(workspace),
                    PaletteAction::Move(target,workspace) => self.move_to_workspace(target,workspace),
                    PaletteAction::Restore => self.offer_restore(),
                    PaletteAction::Launch(id) => self.launch_application(id),
                    PaletteAction::Focus(target) => self.focus_window(target),
                    PaletteAction::Hide(target) => self.hide_window(target),
                    PaletteAction::Close(target) => self.close_window(target),
                    PaletteAction::Shortcut => self.configure_shortcut(),
                    PaletteAction::Launchers => self.manage_launchers(),
                    PaletteAction::Diagnostics => self.show_diagnostics(),
                    PaletteAction::Activity => self.show_activity(),
                    PaletteAction::Preference => MenuEvents::on_command(self,Handle::None,Handle::None,Commands::Preference),
                    PaletteAction::Help => MenuEvents::on_command(self,Handle::None,Handle::None,Commands::Help),
                    PaletteAction::Terminal(action) => if let Some(WindowTarget::Terminal(handle)) = origin && let Some(window) = self.window_mut(handle) { window.terminal_action(action); },
                }
            },
            None => {},
        }
        self.sync_taskbar(); self.refresh_panel();
    }
    pub(super) fn new_terminal(&mut self) {
        let result = self.shortcuts.iter().enumerate().find(|(index,shortcut)| self.launcher_enabled[*index] && shortcut.name == "Terminal")
            .map(|(index,_)| index).ok_or_else(|| anyhow::anyhow!("Configure a Terminal launcher."))
            .and_then(|index| self.launch_independent(index,LaunchContext::Default,"Terminal"));
        if let Err(error) = result { dialogs::error("Cannot open terminal",&format!("{error:#}")); }
    }
    pub(super) fn configure_shortcut(&mut self) {
        let Some(binding) = ShortcutDialog::new(self.preferences.workflow.palette_shortcut.key()).show() else { return; };
        let mut preferences = self.preferences.clone(); preferences.workflow.palette_shortcut = binding;
        match preferences.save(&self.settings_path,Some(&self.preferences.terminal_lock)) {
            Ok(()) => self.preferences = preferences,
            Err(error) => dialogs::error("Shortcut was not saved",&format!("{error:#}")),
        }
    }
    pub(super) fn manage_launchers(&mut self) {
        let path = self.shortcut_dir.clone().unwrap_or_else(|| self.settings_path.parent().unwrap_or(std::path::Path::new(".")).to_owned());
        if let Some(shortcuts) = LauncherManager::new(path,self.settings_path.clone()).show() { self.reload_launchers(shortcuts); }
    }
    pub(super) fn show_diagnostics(&self) {
        let path = self.shortcut_dir.clone().unwrap_or_else(|| self.settings_path.parent().unwrap_or(std::path::Path::new(".")).to_owned());
        DiagnosticsDialog::new(path,self.settings_path.clone()).show();
    }
    pub(super) fn show_activity(&mut self) {
        if self.terminal_locked || self.pending_desktop.is_some() { return; }
        ActivityDialog::new(self.activity.clone()).show();
        self.refresh_panel();
    }
    pub(super) fn reload_launchers(&mut self, shortcuts: Vec<Shortcut>) {
        let incoming = catalog::with_starters(shortcuts);
        self.launcher_enabled.fill(false);
        for shortcut in incoming {
            if let Some(index) = self.shortcuts.iter().position(|old| old.stable_id() == shortcut.stable_id()) {
                self.shortcuts[index] = shortcut; self.launcher_enabled[index] = true;
            } else { self.shortcuts.push(shortcut); self.launcher_enabled.push(true); }
        }
        self.rebuild_launch_menus();
    }
    pub(super) fn rebuild_launch_menus(&mut self) {
        if self.application_root.is_none() { return; }
        for handle in std::mem::take(&mut self.app_menues) { if !handle.is_none() { self.unregister_menu(handle); } }
        self.additional_commands.clear(); self.application_commands.clear();
        self.visibility_items = vec![Handle::None;self.shortcuts.len()];
        self.app_menues = vec![Handle::None;self.shortcuts.len()];
        for index in 0..self.shortcuts.len() {
            if !self.launcher_enabled[index] { continue; }
            let shortcut = self.shortcuts[index].clone(); let mut menu = Menu::new();
            self.visibility_items[index] = menu.add(Command::new("Hide",Key::None,Commands::AppVisibilityToggle));
            menu.add(Command::new("Start",Key::None,Commands::OpenApp)); menu.add(Command::new("Close",Key::None,Commands::CloseApp));
            for command in &shortcut.taskbar.additional_commands {
                let item = menu.add(Command::new(&command.name.replace('&',"&&"),Key::None,Commands::AppCommand));
                self.additional_commands.push((item,index,command.command.clone(),command.args.clone()));
            }
            self.app_menues[index] = self.register_menu(menu);
        }
        let root = self.application_root;
        if let Some(menu) = self.menu_mut(root) { menu.clear(); }
        let apps = catalog::applications(&self.shortcuts).into_iter().filter(|app| match app.id { AppId::External(index) => self.launcher_enabled[index], _ => true }).collect::<Vec<_>>();
        for category in catalog::categories(&apps) {
            let mut menu = Menu::new();
            for app in apps.iter().filter(|app| app.category == category) {
                let item = menu.add(Command::new(&app.name.replace('&',"&&"),Key::None,Commands::LaunchApplication));
                self.application_commands.push((item,app.id));
            }
            if !apps.iter().any(|app| app.category == category) { let mut item = Command::new("No apps yet",Key::None,Commands::None); item.set_enabled(false); menu.add(item); }
            let submenu = menu::SubMenu::new(&category.replace('&',"&&"),menu);
            if let Some(menu) = self.menu_mut(root) { menu.add(submenu); }
        }
        if let Some(menu) = self.menu_mut(root) { self.background_entry = menu.add(Command::new("Background windows",Key::None,Commands::BackgroundWindows)); }
        for task in std::mem::take(&mut self.window_tasks) { self.unregister_menu(task.menu); self.appbar().remove(task.button); }
        self.sync_taskbar(); self.refresh_panel();
    }
}
