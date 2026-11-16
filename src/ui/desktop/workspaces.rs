use super::*;
use crate::config::session::{SavedBindings, SavedContext, SavedPath, SavedWindow, WORKSPACE_COUNT};
use crate::ui::terminal::LaunchPaths;
use std::collections::VecDeque;

pub(super) struct WorkspaceMember { pub target: WindowTarget, pub workspace: u8, pub hidden: bool }
pub(super) enum SessionJob { Load(PathBuf), Save(PathBuf,Session) }
pub(super) enum SessionResult { Loaded(Result<Option<Session>,String>), Saved(Result<(),String>) }

#[ModalWindow(response = bool, events = ButtonEvents)]
struct RestoreDialog { restore: Handle<Button> }
impl RestoreDialog {
    fn new(count: usize) -> Self {
        let mut dialog = Self { base: ModalWindow::new("Restore previous workspace?",layout!("a:c,w:64,h:10"),window::Flags::None),restore: Handle::None };
        dialog.add(Label::new(&format!("Restore {count} saved windows?\nSaved external applications will be launched again.\nMissing applications or documents will be reported."),layout!("l:1,r:1,t:0,h:4")));
        let cancel = dialog.add(Button::new("&Cancel",layout!("l:10,b:0,w:14")));
        dialog.restore = dialog.add(Button::new("&Restore",layout!("r:10,b:0,w:14")));
        dialog.base.request_focus_for_control(cancel); dialog
    }
}
impl ButtonEvents for RestoreDialog {
    fn on_pressed(&mut self, handle: Handle<Button>) -> EventProcessStatus { self.exit_with(handle == self.restore); EventProcessStatus::Processed }
}

impl MyDesktop {
    pub(super) fn fit_workspace_windows(&mut self) {
        let area = self.desktop_rect();
        for target in self.window_order.clone() {
            if let Some(window) = self.target_base_mut(target) {
                let position = window.position(); let size = window.size();
                let (next_position,next_size) = fit_geometry(area,position,size);
                if next_size != size { window.set_size(next_size.width as u16,next_size.height as u16); }
                if next_position != position { window.set_position(next_position.x,next_position.y); }
            }
        }
    }
    pub(super) fn reveal_workspace(&mut self, target: WindowTarget) {
        let workspace = self.members.iter().find(|member| member.target == target).map(|member| member.workspace);
        if let Some(workspace) = workspace {
            if workspace != self.active_workspace { self.switch_workspace(workspace); }
            if let Some(member) = self.members.iter_mut().find(|member| member.target == target) { member.hidden = false; }
        }
    }
    pub(super) fn switch_workspace(&mut self, workspace: u8) {
        if workspace >= WORKSPACE_COUNT || self.terminal_locked { return; }
        self.active_workspace = workspace;
        let visibility = self.members.iter().map(|member| (member.target,member.workspace == workspace && !member.hidden)).collect::<Vec<_>>();
        for (target,visible) in visibility {
            if !visible { self.suspend_target(target); }
            if let Some(window) = self.target_base_mut(target) { window.set_visible(visible); }
        }
        if let Some(method) = self.arrange_method { self.arrange_workspace(method); }
        if let Some(target) = self.members.iter().rev().find(|member| member.workspace == workspace && !member.hidden).map(|member| member.target)
            && let Some(window) = self.target_base_mut(target) { window.request_focus(); }
        self.sync_taskbar(); self.refresh_panel();
    }
    fn suspend_target(&mut self, target: WindowTarget) {
        match target {
            WindowTarget::Paint(handle) => if let Some(window) = self.window_mut(handle) { window.suspend(); },
            WindowTarget::Game(handle) => if let Some(window) = self.window_mut(handle) { window.suspend(); },
            _ => {},
        }
    }
    pub(super) fn move_to_workspace(&mut self, target: WindowTarget, workspace: u8) {
        if workspace >= WORKSPACE_COUNT || self.terminal_locked { return; }
        if let Some(member) = self.members.iter_mut().find(|member| member.target == target) { member.workspace = workspace; }
        let visible = self.members.iter().find(|member| member.target == target).is_some_and(|member| member.workspace == self.active_workspace && !member.hidden);
        if !visible { self.suspend_target(target); }
        if let Some(window) = self.target_base_mut(target) { window.set_visible(visible); }
        self.sync_taskbar(); self.refresh_panel();
    }
    pub(super) fn arrange_workspace(&mut self, method: desktop::ArrangeWindowsMethod) {
        let targets = self.window_order.iter().copied().filter(|target| self.target_base(*target).is_some_and(|window| window.is_visible())).collect::<Vec<_>>();
        let count = targets.len(); if count == 0 { return; }
        let area = self.desktop_rect(); let width = area.width().max(1); let height = area.height().max(1);
        let mut columns = 1usize; while columns * columns < count { columns += 1; }
        for (index,target) in targets.into_iter().enumerate() {
            let (x,y,w,h) = match method {
                desktop::ArrangeWindowsMethod::Vertical => {
                    let left = width * index as u32 / count as u32; let right = width * (index+1) as u32 / count as u32;
                    (left,0,right.saturating_sub(left).max(1),height)
                }
                desktop::ArrangeWindowsMethod::Horizontal => {
                    let top = height * index as u32 / count as u32; let bottom = height * (index+1) as u32 / count as u32;
                    (0,top,width,bottom.saturating_sub(top).max(1))
                }
                desktop::ArrangeWindowsMethod::Grid => {
                    let rows = count.div_ceil(columns); let column = index % columns; let row = index / columns;
                    let x = width * column as u32 / columns as u32; let y = height * row as u32 / rows as u32;
                    (x,y,(width * (column+1) as u32 / columns as u32).saturating_sub(x).max(1),(height * (row+1) as u32 / rows as u32).saturating_sub(y).max(1))
                }
                desktop::ArrangeWindowsMethod::Cascade => ((index as u32 * 2) % width.max(1),index as u32 % height.max(1),(width*3/4).max(1),(height*3/4).max(1)),
            };
            if let Some(window) = self.target_base_mut(target) {
                window.set_position(area.left()+x as i32,area.top()+y as i32);
                window.set_size(w.min(u16::MAX as u32) as u16,h.min(u16::MAX as u32) as u16);
            }
        }
    }
    pub(super) fn remember_terminal(&mut self, handle: Handle<TuiWindow>, index: usize, _context: LaunchContext<'_>, additional: Option<String>) {
        let shortcut = &self.shortcuts[index];
        let Some(paths) = self.windowt(handle).map(|window| window.launch_paths().clone()) else { return; };
        let (context,path) = if paths.document.is_some() { (SavedContext::Document,paths.document.as_deref()) }
            else if paths.file.is_some() { (SavedContext::File,paths.file.as_deref()) }
            else if paths.folder.is_some() { (SavedContext::Folder,paths.folder.as_deref()) }
            else { (SavedContext::Default,paths.directory.as_deref()) };
        let bindings = SavedBindings { file: paths.file.as_deref().map(SavedPath::new),folder: paths.folder.as_deref().map(SavedPath::new),
            document: paths.document.as_deref().map(SavedPath::new),directory: paths.directory.as_deref().map(SavedPath::new) };
        self.terminal_launches.push((handle,SavedLaunch::External { launcher: shortcut.stable_id(),primary: self.app_windows.get(&index) == Some(&handle),additional,bindings: Some(bindings),context,path: path.map(SavedPath::new) }));
    }
    pub(super) fn session_snapshot(&self) -> Session {
        let windows = self.members.iter().filter_map(|member| {
            self.window_info(member.target)?;
            let base = self.target_base(member.target)?; let size = base.size(); let position = base.position();
            let launch = match member.target {
                WindowTarget::Terminal(handle) => self.terminal_launches.iter().find(|entry| entry.0 == handle)?.1.clone(),
                WindowTarget::Browser(handle) => SavedLaunch::Native { app: "files".into(),path: self.windowt(handle)?.current_path().as_deref().map(SavedPath::new) },
                WindowTarget::Image(handle) => SavedLaunch::Native { app: "image".into(),path: self.windowt(handle)?.document_path().as_deref().map(SavedPath::new) },
                WindowTarget::Pdf(handle) => SavedLaunch::Native { app: "pdf".into(),path: self.windowt(handle)?.document_path().as_deref().map(SavedPath::new) },
                WindowTarget::Music(handle) => SavedLaunch::Native { app: "music".into(),path: self.windowt(handle)?.document_path().as_deref().map(SavedPath::new) },
                WindowTarget::Paint(handle) => SavedLaunch::Native { app: "paint".into(),path: self.windowt(handle)?.document_path().as_deref().map(SavedPath::new) },
                WindowTarget::Help(_) => SavedLaunch::Native { app: "help".into(),path: None },
                WindowTarget::Game(handle) => SavedLaunch::Native { app: self.game_windows.iter().find(|(_,value)| **value == handle)?.0.name().to_owned(),path: None },
            };
            Some(SavedWindow { launch,workspace: member.workspace,hidden: member.hidden,x: position.x,y: position.y,width: size.width.clamp(1,u16::MAX as u32) as u16,height: size.height.clamp(1,u16::MAX as u32) as u16 })
        }).collect();
        Session { active_workspace: self.active_workspace,windows,..Session::default() }
    }
    pub(super) fn start_session_io(&mut self) {
        let Some(path) = self.session_path.clone() else { return; };
        match LatestWorker::new(|job| match job {
            SessionJob::Load(path) => SessionResult::Loaded(session::load(&path).map_err(|error| format!("{error:#}"))),
            SessionJob::Save(path,session) => SessionResult::Saved(session::save(&path,&session).map_err(|error| format!("{error:#}"))),
        }) {
            Ok(mut worker) => { worker.submit(SessionJob::Load(path)); self.session_worker = Some(worker); self.session_loading = true; },
            Err(error) => dialogs::error("Workspace storage unavailable",&error.to_string()),
        }
    }
    pub(super) fn save_before_exit(&mut self) -> bool {
        if self.exit_saved { return false; }
        if !self.session_writable { return false; }
        if self.session_loading { self.exit_waiting = true; self.freeze_for_exit(); return true; }
        if self.saving_exit { return true; }
        let Some(path) = self.session_path.clone() else { return false; };
        let snapshot = self.session_snapshot();
        // Declining restoration and closing an empty desktop keeps the previous snapshot available.
        if self.session_declined && snapshot.windows.is_empty() && self.saved_session.as_ref().is_some_and(|session| !session.windows.is_empty()) { return false; }
        let Some(worker) = self.session_worker.as_mut() else { return false; };
        worker.submit(SessionJob::Save(path,snapshot)); self.saving_exit = true;
        self.restore_queue.clear(); self.freeze_for_exit(); true
    }
    fn freeze_for_exit(&mut self) {
        for target in self.window_order.clone() {
            self.suspend_target(target);
            // AppCUI window controls are always enabled. Hiding them prevents
            // edits after the unsaved-work guard without changing logical visibility.
            if let Some(window) = self.target_base_mut(target) { window.set_visible(false); }
        }
        let menu = self.desktop_menu;
        if let Some(button) = self.appbar().get_mut(menu) { button.set_caption("Saving"); }
    }
    pub(super) fn poll_session_io(&mut self) {
        let result = self.session_worker.as_mut().and_then(LatestWorker::poll);
        match result {
            Some(SessionResult::Loaded(Ok(Some(session)))) => {
                self.session_loading = false;
                let offer = !session.windows.is_empty(); self.session_declined = offer; self.saved_session = Some(session);
                if offer && !self.exit_waiting { self.offer_restore(); }
            }
            Some(SessionResult::Loaded(Ok(None))) => self.session_loading = false,
            Some(SessionResult::Loaded(Err(error))) => { self.session_loading = false; self.session_writable = false; dialogs::error("Saved workspace unavailable",&error); },
            Some(SessionResult::Saved(result)) => {
                self.saving_exit = false; self.exit_saved = true;
                if let Err(error) = result { dialogs::error("Workspace was not saved",&error); }
                self.complete_desktop(DesktopIntent::Exit);
            }
            _ => {},
        }
        if self.exit_waiting && !self.session_loading { self.exit_waiting = false; self.complete_desktop(DesktopIntent::Exit); }
        if self.saving_exit { return; }
        if let Some(window) = self.restore_queue.pop_front() {
            let workspace = window.workspace;
            match self.restore_window(window) {
                Ok(target) => {
                    self.session_declined = false;
                    if let Some(member) = self.members.iter_mut().find(|member| member.target == target) { member.workspace = workspace; }
                }
                Err(error) => self.restore_errors.push(format!("{error:#}")),
            }
            if self.restore_queue.is_empty() {
                self.switch_workspace(self.restore_workspace);
                if !self.restore_errors.is_empty() { dialogs::error("Some windows could not be restored",&std::mem::take(&mut self.restore_errors).join("\n")); }
            }
        }
    }
    pub(super) fn offer_restore(&mut self) {
        if !self.restore_queue.is_empty() { return; }
        let Some(session) = self.saved_session.clone() else { return; };
        if session.windows.is_empty() { return; }
        if RestoreDialog::new(session.windows.len()).show() == Some(true) {
            self.restore_workspace = session.active_workspace; self.restore_queue = VecDeque::from(session.windows); self.restore_errors.clear();
        }
    }
    pub(super) fn restore_window(&mut self, saved: SavedWindow) -> anyhow::Result<WindowTarget> {
        let path = match &saved.launch { SavedLaunch::Native {path,..}|SavedLaunch::External {path,..} => path.as_ref().map(SavedPath::path) };
        if let Some(path) = &path { anyhow::ensure!(path.exists(),"Saved file or working directory is unavailable: {}",path.display()); }
        let target = match saved.launch {
            SavedLaunch::External { launcher, primary,additional,bindings,context,.. } => {
                let index = self.shortcuts.iter().enumerate().find(|(index,shortcut)| self.launcher_enabled[*index] && shortcut.stable_id() == launcher).map(|(index,_)| index).ok_or_else(|| anyhow::anyhow!("Saved launcher is unavailable: {launcher}"))?;
                let bindings = if let Some(bindings) = bindings {
                    LaunchPaths { file: bindings.file.as_ref().map(SavedPath::path),folder: bindings.folder.as_ref().map(SavedPath::path),document: bindings.document.as_ref().map(SavedPath::path),directory: bindings.directory.as_ref().map(SavedPath::path) }
                } else { match (context,path.as_deref()) {
                    (SavedContext::Default,Some(path)) => LaunchPaths { directory: Some(path.to_owned()),..LaunchPaths::default() },
                    (SavedContext::File,Some(path)) => LaunchPaths { file: Some(path.to_owned()),folder: path.parent().map(std::path::Path::to_owned),directory: path.parent().map(std::path::Path::to_owned),..LaunchPaths::default() },
                    (SavedContext::Folder,Some(path)) => LaunchPaths { folder: Some(path.to_owned()),directory: Some(path.to_owned()),..LaunchPaths::default() },
                    _ => return Err(anyhow::anyhow!("Saved launcher {launcher} lacks its original launch paths. Relaunch it before saving another workspace.")),
                } };
                for path in [&bindings.file,&bindings.folder,&bindings.document,&bindings.directory].into_iter().flatten() {
                    anyhow::ensure!(path.exists(),"Saved launch path is unavailable: {}",path.display());
                }
                let context = LaunchContext::Restore(&bindings);
                if let Some(name) = additional {
                    let shortcut = self.shortcuts[index].clone();
                    let command = shortcut.taskbar.additional_commands.iter().find(|command| command.name == name).ok_or_else(|| anyhow::anyhow!("Saved extra command is unavailable: {name}"))?;
                    let window = TuiWindow::new_with_context_and_directory(&shortcut.name,command.command.clone(),command.args.clone(),shortcut.window.clone(),shortcut.terminal.clone(),&self.preferences.terminal,context,shortcut.working_directory.as_deref())?;
                    let handle = self.add_window(window); self.document_windows.push(handle); self.remember_terminal(handle,index,context,Some(name)); self.track_window(WindowTarget::Terminal(handle));
                    WindowTarget::Terminal(handle)
                } else {
                    self.launch_independent(index,context,&self.shortcuts[index].name.clone())?;
                    let handle = *self.document_windows.last().ok_or_else(|| anyhow::anyhow!("Restored terminal was not created"))?;
                    if primary && !self.app_windows.get(&index).is_some_and(|handle| self.windowt(*handle).is_some()) {
                        self.app_windows.insert(index,handle); self.document_windows.retain(|value| *value != handle);
                        if let Some((_,SavedLaunch::External { primary,.. })) = self.terminal_launches.iter_mut().find(|entry| entry.0 == handle) { *primary = true; }
                    }
                    WindowTarget::Terminal(handle)
                }
            }
            SavedLaunch::Native { app,.. } => match app.as_str() {
                "files" => {
                    let window = if let Some(path) = path { BrowserWindow::new_at(path)? } else { BrowserWindow::new()? };
                    if self.windowt(self.browser_window).is_some() { return Err(anyhow::anyhow!("File Browser is already open")); }
                    self.browser_window = self.add_window(window); let target = WindowTarget::Browser(self.browser_window); self.track_window(target); target
                }
                "image" => { anyhow::ensure!(self.windowt(self.image_window).is_none(),"Image Viewer is already open; existing window kept"); anyhow::ensure!(path.is_some(),"Saved image has no path"); self.open_image(path); WindowTarget::Image(self.image_window) },
                "pdf" => { anyhow::ensure!(self.windowt(self.pdf_window).is_none(),"PDF Reader is already open; existing window kept"); self.open_pdf(path); WindowTarget::Pdf(self.pdf_window) },
                "music" => { anyhow::ensure!(self.windowt(self.music_window).is_none(),"Music Player is already open; existing window kept"); self.open_music(path); WindowTarget::Music(self.music_window) },
                "paint" => { anyhow::ensure!(self.windowt(self.paint_window).is_none(),"ASCII Paint is already open; existing window kept"); self.open_paint(path); WindowTarget::Paint(self.paint_window) },
                "help" => { let handle = self.add_window(help_window()); self.help_windows.push(handle); let target = WindowTarget::Help(handle); self.track_window(target); target },
                name => {
                    let kind = GameKind::ALL.into_iter().find(|kind| kind.name() == name).ok_or_else(|| anyhow::anyhow!("Saved application is unavailable: {name}"))?;
                    anyhow::ensure!(!self.game_windows.get(&kind).is_some_and(|handle| self.windowt(*handle).is_some()),"{name} is already open; existing window kept");
                    self.open_game(kind); WindowTarget::Game(*self.game_windows.get(&kind).ok_or_else(|| anyhow::anyhow!("Could not restore {name}"))?)
                }
            },
        };
        if let Some(member) = self.members.iter_mut().find(|member| member.target == target) { member.workspace = saved.workspace; member.hidden = saved.hidden; }
        let size = self.desktop_rect().size();
        if let Some(window) = self.target_base_mut(target) {
            window.set_size(saved.width.min(size.width.max(1).min(u16::MAX as u32) as u16),saved.height.min(size.height.max(1).min(u16::MAX as u32) as u16));
            let actual = window.size();
            window.set_position(saved.x.clamp(0,size.width.saturating_sub(actual.width) as i32),saved.y.clamp(0,size.height.saturating_sub(actual.height) as i32));
            window.set_visible(false);
        }
        Ok(target)
    }
}

fn fit_geometry(area: Rect, position: Point, size: Size) -> (Point,Size) {
    let width = size.width.clamp(1,area.width().max(1).min(u16::MAX as u32));
    let height = size.height.clamp(1,area.height().max(1).min(u16::MAX as u32));
    let x = position.x.clamp(area.left(),area.left()+area.width().saturating_sub(width) as i32);
    let y = position.y.clamp(area.top(),area.top()+area.height().saturating_sub(height) as i32);
    (Point::new(x,y),Size::new(width,height))
}

#[cfg(test)]
mod resize_tests {
    use super::*;
    #[test]
    fn fitting_ssh_windows_preserves_valid_geometry_and_recovers_offscreen_windows() {
        let area = Rect::with_size(0,1,80,23);
        let valid = (Point::new(5,3),Size::new(60,18));
        assert_eq!(fit_geometry(area,valid.0,valid.1),valid);
        let (position,size) = fit_geometry(area,Point::new(100,40),Size::new(120,36));
        assert_eq!(position,Point::new(0,1)); assert_eq!(size,Size::new(80,23));
        let small = Rect::with_size(0,1,40,11);
        let (position,size) = fit_geometry(small,Point::new(-5,-8),Size::new(68,18));
        assert_eq!(position,Point::new(0,1)); assert_eq!(size,Size::new(40,11));
    }
}
