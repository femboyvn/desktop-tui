use super::keyboard::CustomKeyboardControl;
use super::input_queue::InputError;
use crate::config::preferences::TerminalPalette;
use crate::config::{BackgroundColor, TerminalOptions, WindowOptions, WindowSize};
use crate::runtime::{TerminalParser, MAX_TERMINAL_DIMENSION};
use anyhow::anyhow;
use crate::runtime::processes::{self, ProcessGuard};
use appcui::dialogs::{Location, OpenFileDialogFlags, SelectFolderDialogFlags};
use appcui::graphics::{Color, Size};
use appcui::prelude::window::Flags;
use appcui::prelude::{
    Alignment, Canvas, EventProcessStatus, Handle, LayoutBuilder, TimerEvents, Window,
    canvas, WindowEvents, ActionRequest,
};
use async_channel::Receiver;
use std::cell::RefCell;
use std::rc::Rc;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use virtual_terminal::{Command, Output};

/// Actual selected bindings and effective cwd, retained for session restoration.
#[derive(Clone, Debug, Default)]
pub(crate) struct LaunchPaths {
    pub file: Option<PathBuf>,
    pub folder: Option<PathBuf>,
    pub document: Option<PathBuf>,
    pub directory: Option<PathBuf>,
}

/// File/Folder bind browser paths; Document retains picker-then-append behavior.
/// Restore reuses recorded bindings and never opens a new picker.
#[derive(Clone, Copy, Default)]
pub(crate) enum LaunchContext<'a> {
    #[default] Default, Document(&'a Path), File(&'a Path), Folder(&'a Path),
    Restore(&'a LaunchPaths),
}
impl<'a> LaunchContext<'a> {
    fn directory(self) -> Option<&'a Path> {
        match self {
            Self::File(path) => path.parent(), Self::Folder(path) => Some(path),
            Self::Restore(paths) => paths.directory.as_deref(), _ => None,
        }
    }
    fn template_directory(self) -> Option<&'a Path> {
        match self { Self::Restore(paths) => paths.folder.as_deref(), _ => self.directory() }
    }
    fn selected_file(self) -> Option<&'a Path> {
        match self { Self::File(path) => Some(path), Self::Restore(paths) => paths.file.as_deref(), _ => None }
    }
    fn explicit_document(self) -> Option<&'a Path> {
        match self { Self::Document(path) => Some(path), Self::Restore(paths) => paths.document.as_deref(), _ => None }
    }
    fn appended_document(self, has_file_template: bool) -> Option<&'a Path> {
        self.explicit_document().or_else(|| if has_file_template { None } else { self.selected_file() })
    }
}

#[Window(events = [TimerEvents, WindowEvents])]
pub struct TuiWindow {
    pub canvas: Handle<Canvas>,
    terminal_parser: Rc<RefCell<TerminalParser>>,
    pub custom_keyboard_control: Handle<CustomKeyboardControl>,
    pub horizontal_adjustment: u32,
    pub vertical_adjustment: u32,
    padding: (i32, i32),
    pending_output: Option<(Vec<u8>, usize)>,
    input_closed_at: Option<Instant>,
    process: ProcessGuard,
    closing: bool,
    launch_command: String,
    pending_error: Option<String>,
    original_title: String,
    launch_paths: LaunchPaths,
}

impl TuiWindow {
    pub fn new<S, I>(
        app_name: &str,
        program: S,
        args: I,
        window_options: WindowOptions,
        terminal_options: TerminalOptions,
        palette: &TerminalPalette,
    ) -> anyhow::Result<Self>
    where
        S: AsRef<OsStr>,
        I: IntoIterator<Item = S>,
    {
        Self::new_with_context(app_name, program, args, window_options, terminal_options, palette, LaunchContext::Default)
    }

    /// The document path is appended after template resolution and never interpreted as a template.
    pub fn new_with_document<S, I>(
        app_name: &str, program: S, args: I, window_options: WindowOptions,
        terminal_options: TerminalOptions, palette: &TerminalPalette, document: Option<&Path>,
    ) -> anyhow::Result<Self>
    where S: AsRef<OsStr>, I: IntoIterator<Item = S>,
    {
        Self::new_with_context(app_name, program, args, window_options, terminal_options, palette,
            document.map_or(LaunchContext::Default,LaunchContext::Document))
    }

    pub(crate) fn new_with_context<S, I>(
        app_name: &str, program: S, args: I, window_options: WindowOptions,
        terminal_options: TerminalOptions, palette: &TerminalPalette, context: LaunchContext<'_>,
    ) -> anyhow::Result<Self>
    where S: AsRef<OsStr>, I: IntoIterator<Item = S>,
    {
        Self::new_with_context_and_directory(app_name, program, args, window_options, terminal_options, palette, context, None)
    }

    /// Explicit file/folder contexts override a configured cwd; cwd never binds picker templates.
    pub(crate) fn new_with_context_and_directory<S, I>(
        app_name: &str, program: S, args: I, window_options: WindowOptions,
        terminal_options: TerminalOptions, palette: &TerminalPalette, context: LaunchContext<'_>, directory: Option<&Path>,
    ) -> anyhow::Result<Self>
    where S: AsRef<OsStr>, I: IntoIterator<Item = S>,
    {
        let window_size = window_options.size.unwrap_or(WindowSize {
            width: 100,
            height: 25,
        });

        let padding = terminal_options.padding.unwrap_or((0, 0));
        let (x, y, inner_size) = terminal_geometry(Size::new(window_size.width, window_size.height), padding);

        let mut window_flags = Flags::None;

        if window_options.resizable {
            window_flags |= Flags::Sizeable;
        }

        if !window_options.close_button {
            window_flags |= Flags::NoCloseButton;
        }

        if window_options.fixed_position {
            window_flags |= Flags::FixedPosition;
        }

        let win = Window::new(
            app_name,
            LayoutBuilder::new()
                .alignment(Alignment::Center)
                .width(window_size.width)
                .height(window_size.height)
                .build(),
            window_flags,
        );

        let mut configured = vec![program.as_ref().to_owned()];
        configured.extend(args.into_iter().map(|arg| arg.as_ref().to_owned()));
        let (modified_program, modified_args, launch_paths) = resolve_launch(configured,context,directory)?;
        let modified_program = crate::platform::bundled::resolve_program(&modified_program);
        let launch_command = modified_program.to_string_lossy().into_owned();
        let mut cmd = Command::new(modified_program).args(modified_args)
            .terminal_size((inner_size.width as usize, inner_size.height as usize))
            .terminal_id("xterm-256color")
            .env("COLORTERM", "truecolor");
        if let Some(directory) = &launch_paths.directory { cmd = cmd.current_dir(directory); }
        let tx = cmd.in_tx();
        let (process, rx) = processes::start(cmd);

        let default_background_color = match terminal_options.background_color {
            None => None,
            Some(BackgroundColor { r, g, b }) => Some(Color::RGB(r, g, b)),
        };

        let mut tui_win = Self {
            base: win,
            canvas: Handle::None,
            custom_keyboard_control: Handle::None,
            terminal_parser: Rc::new(RefCell::new(TerminalParser::with_palette(
                inner_size.width,
                inner_size.height,
                palette,
                default_background_color,
            ))),
            horizontal_adjustment: 2 + x as u32,
            vertical_adjustment: 2 + y as u32,
            padding, pending_output: None, input_closed_at: None,
            process, closing: false, launch_command, pending_error: None, original_title: app_name.into(), launch_paths,
        };

        tui_win.canvas = tui_win.add(Canvas::new(
            Size::new(inner_size.width, inner_size.height),
            LayoutBuilder::new()
                .width(inner_size.width)
                .height(inner_size.height)
                .x(x)
                .y(y)
                .build(),
            canvas::Flags::None,
        ));

        let surface = tui_win.terminal_parser.borrow().new_surface();
        let canvas = tui_win.canvas;
        *tui_win.control_mut(canvas).unwrap().drawing_surface_mut() = surface;

        let timer = match tui_win.timer() {
            Some(t) => t,
            None => return Err(anyhow!("Failed to get timer")),
        };
        timer.start(Duration::from_millis(25));

        let parser = tui_win.terminal_parser.clone();
        tui_win.custom_keyboard_control = tui_win.add(CustomKeyboardControl::new(tx, rx,
            parser, LayoutBuilder::new().x(x).y(y)
                .width(inner_size.width).height(inner_size.height).build()));

        Ok(tui_win)
    }

    pub(crate) fn launch_paths(&self) -> &LaunchPaths { &self.launch_paths }
    pub(crate) fn application_title(&self) -> &str { &self.original_title }

    /// Queues literal bytes through the same ordered input path as typed keys.
    pub(crate) fn send_input(&mut self, bytes: Vec<u8>) {
        let handle = self.custom_keyboard_control;
        if let Some(control) = self.control_mut(handle) { control.enqueue(bytes, false); }
    }
    pub(crate) fn terminal_action(&mut self, action: super::TerminalAction) {
        let handle = self.custom_keyboard_control;
        if let Some(control) = self.control_mut(handle) { control.terminal_action(action); }
    }

    pub fn apply_palette(&mut self, palette: &TerminalPalette) {
        if let Some(canvas) = self.base.control_mut(self.canvas) {
            self.terminal_parser
                .borrow_mut()
                .apply_palette(palette, canvas.drawing_surface_mut());
        }
    }

    fn sync_terminal_size(&mut self) -> Result<(), InputError> {
        let (x, y, size) = terminal_geometry(self.size(), self.padding);
        let Size { width, height } = size;
        self.horizontal_adjustment = 2 + x as u32;
        self.vertical_adjustment = 2 + y as u32;
        let canvas = self.base.control_mut(self.canvas).unwrap();
        if canvas.position() != appcui::graphics::Point::new(x, y) { canvas.set_position(x, y); }
        if canvas.drawing_surface_mut().size() != size {
            self.terminal_parser
                .borrow_mut()
                .resize_surface(width, height, canvas.drawing_surface_mut());
            canvas.set_size(width as u16, height as u16);
            let control = self.base.control_mut(self.custom_keyboard_control).unwrap();
            control.set_size(width as u16, height as u16);
            control.input.resize(width as usize, height as usize)?;
        }
        let control = self.base.control_mut(self.custom_keyboard_control).unwrap();
        if control.position() != appcui::graphics::Point::new(x, y) { control.set_position(x, y); }
        control.input.flush()
    }

    pub fn is_closing(&self) -> bool { self.pending_error.is_none() && self.closing }

    fn cleanup(&mut self) {
        if self.closing { return; }
        self.closing = true;
        self.process.close();
        let handle = self.custom_keyboard_control;
        if let Some(control) = self.control_mut(handle) {
            control.close_clipboard();
            control.tx.close();
            control.rx.close();
        }
    }
    fn show_launch_error(&self, error: &str) {
        dialogs::error("Unable to run application", &format!("{}\n{error}\nInstall this program separately and ensure its executable is on PATH, or update the shortcut command.", self.launch_command));
    }
    pub fn close_command(&mut self) { self.pending_error = None; self.cleanup(); self.close(); }
}
impl WindowEvents for TuiWindow {
    fn on_cancel(&mut self) -> ActionRequest { self.cleanup(); ActionRequest::Allow }
}

impl TimerEvents for TuiWindow {
    fn on_update(&mut self, _: u64) -> EventProcessStatus {
        if self.is_visible() && let Some(error) = self.pending_error.take() {
            self.show_launch_error(&error);
            self.close_command();
            return EventProcessStatus::Processed;
        }
        if self.closing { return EventProcessStatus::Ignored; }
        // The runner drops its input receiver at exit, possibly before the UI
        // reads its terminal output. Preserve the child's diagnostic/clean exit.
        let resize_error = self.sync_terminal_size().err();
        let control_handle = self.custom_keyboard_control;
        let (should_close, input_error, rx) = {
            let control = self.control_mut(control_handle).unwrap();
            (control.should_exit, control.input_error.take(), control.rx.clone())
        };
        if should_close { self.close_command(); return EventProcessStatus::Processed; }

        let (view_changed, status) = {
            let control = self.control_mut(control_handle).unwrap();
            (control.tick(), control.status())
        };
        let status: String = status.chars().take(160).collect();
        let title = if status.is_empty() { self.original_title.clone() } else { format!("{} [{status}]", self.original_title) };
        self.set_title(&title);
        let parser = self.terminal_parser.clone();
        let mut parser = parser.borrow_mut();
        let batch = drain_output(&rx, &mut self.pending_output, |bytes| parser.advance_bytes(bytes));
        if batch.changed || view_changed {
            let canvas = self.base.control_mut(self.canvas).unwrap();
            parser.project(canvas.drawing_surface_mut());
        }
        let replies = parser.take_replies();
        let reply_overflow = parser.take_reply_error();
        drop(parser);
        let input_result = self.control_mut(control_handle).unwrap().input.data(replies);
        if let Some(error) = batch.error {
            self.fail_child(error);
        } else if batch.terminated {
            self.close_command();
        } else if reply_overflow {
            self.fail_child("The child terminal-query reply buffer exceeded 64 KiB. This application will close.".into());
        } else {
            let errors = [input_error, input_result.err(), resize_error];
            // Queue exhaustion is a failure even if a closed-channel observation
            // occurred in the same tick. Closure alone can be an exit transition.
            let error = if errors.contains(&Some(InputError::Full)) { Some(InputError::Full) }
                else { errors.into_iter().flatten().next() };
            if let Some(error) = error {
                if let Some(message) = self.input_failure(error, &rx) { self.fail_child(message); }
            } else { self.input_closed_at = None; }
        }
        if batch.changed || view_changed || batch.terminated || self.closing { EventProcessStatus::Processed }
        else { EventProcessStatus::Ignored }
    }
}

impl TuiWindow {
    fn input_failure(&mut self, error: InputError, rx: &Receiver<Output>) -> Option<String> {
        if error == InputError::Full { return Some(error.to_string()); }
        if self.process.finished() || !rx.is_empty() || self.pending_output.is_some() {
            self.input_closed_at = None;
            return None;
        }
        // Input can close before the runner publishes its exit/error. Give that
        // asynchronous result time to arrive without blocking a UI callback.
        // A broken writer with a live child still closes after a bounded grace.
        let since = self.input_closed_at.get_or_insert_with(Instant::now);
        (since.elapsed() >= Duration::from_secs(1)).then(|| error.to_string())
    }
    fn fail_child(&mut self, error: String) {
        if self.is_visible() {
            self.show_launch_error(&error);
            self.close_command();
        } else {
            self.pending_error = Some(error);
            self.cleanup();
        }
    }
}

/// Consume a bounded amount of output and leave the rest for the next UI tick.
#[derive(Default)]
struct OutputBatch { changed: bool, error: Option<String>, terminated: bool }
fn drain_output(rx: &Receiver<Output>, pending: &mut Option<(Vec<u8>, usize)>, mut consume: impl FnMut(&[u8])) -> OutputBatch {
    let mut batch = OutputBatch::default();
    let mut remaining = 256 * 1024;
    for _ in 0..64 {
        if remaining == 0 { break; }
        let (bytes, offset) = if let Some(data) = pending.take() { data } else {
            match rx.try_recv() {
                Ok(Output::Stdout(bytes)) => (bytes, 0),
                Ok(Output::Pid(_)) => continue,
                Ok(Output::Error(error)) => { batch.error = Some(error); break; }
                Ok(Output::Terminated(_)) => { batch.terminated = true; break; }
                Err(_) => break,
            }
        };
        let end = bytes.len().min(offset + remaining);
        if end > offset { consume(&bytes[offset..end]); batch.changed = true; }
        remaining -= end - offset;
        if end < bytes.len() { *pending = Some((bytes, end)); break; }
    }
    batch
}

/// Padding is an inset from the client origin; retain at least one inner cell.
fn terminal_geometry(size: Size, padding: (i32, i32)) -> (i32, i32, Size) {
    let client_width = size.width.saturating_sub(2).clamp(1, MAX_TERMINAL_DIMENSION);
    let client_height = size.height.saturating_sub(2).clamp(1, MAX_TERMINAL_DIMENSION);
    let x = padding.0.max(0).min(client_width.saturating_sub(1) as i32);
    let y = padding.1.max(0).min(client_height.saturating_sub(1) as i32);
    (x, y, Size::new(client_width - x as u32, client_height - y as u32))
}

#[cfg(test)]
fn resolve_templates(configured: Vec<OsString>, context: LaunchContext<'_>) -> anyhow::Result<(OsString, Vec<OsString>)> {
    let (program,args,_) = resolve_launch(configured,context,None)?;
    Ok((program,args))
}

fn resolve_launch(mut configured: Vec<OsString>, context: LaunchContext<'_>, configured_directory: Option<&Path>)
    -> anyhow::Result<(OsString, Vec<OsString>, LaunchPaths)> {
    let contains = |token: &[u8]| configured.iter().any(|arg| arg.as_bytes().windows(token.len()).any(|window| window == token));
    let has_file_template = contains(b"<FILE_PATH>");
    let has_folder_template = contains(b"<FOLDER_PATH>");
    let restoring = matches!(context,LaunchContext::Restore(_));
    if restoring {
        for (required,bound,token) in [(has_file_template,context.selected_file().is_some(),"<FILE_PATH>"),
            (has_folder_template,context.template_directory().is_some(),"<FOLDER_PATH>")] {
            if required && !bound {
                return Err(anyhow!("Cannot restore terminal: saved {token} binding is missing. Relaunch this application and select its path again."));
            }
        }
    }
    let home = directories::UserDirs::new().map(|dirs| dirs.home_dir().to_owned()).unwrap_or_else(|| "/".into());
    let file = if let Some(path) = context.selected_file() { Some(path.to_owned()) } else if has_file_template {
        Some(dialogs::open("Select file", "", Location::Path(&home), None,
            OpenFileDialogFlags::Icons | OpenFileDialogFlags::CheckIfFileExists).ok_or_else(|| anyhow!("No file selected"))?)
    } else { None };
    let folder = if let Some(path) = context.template_directory() { Some(path.to_owned()) } else if has_folder_template {
        Some(dialogs::select_folder("Select folder", Location::Path(&home), SelectFolderDialogFlags::Icons)
            .ok_or_else(|| anyhow!("No folder selected"))?)
    } else { None };
    configured = configured.iter().map(|arg| expand_argument(arg, file.as_deref(), folder.as_deref())).collect();
    let program = configured.remove(0);
    append_document(&mut configured,context.appended_document(has_file_template));
    // Record an absolute effective cwd so a later restore does not reinterpret
    // relative launcher paths against whichever directory starts the desktop.
    let requested_directory = context.directory().or(configured_directory);
    let directory = match requested_directory {
        Some(path) if path.is_absolute() => path.to_owned(),
        path => {
            let current = std::env::current_dir().map_err(|error| anyhow!("Unable to determine the terminal working directory: {error}"))?;
            match path { Some(path) => current.join(path),None => current }
        },
    };
    let paths = LaunchPaths { file,folder,document: context.explicit_document().map(Path::to_owned),directory: Some(directory) };
    Ok((program,configured,paths))
}
fn expand_argument(arg: &OsStr, file: Option<&Path>, folder: Option<&Path>) -> OsString {
    let mut bytes = arg.as_bytes();
    let mut result = Vec::new();
    while !bytes.is_empty() {
        let replacement = [(b"<FILE_PATH>".as_slice(), file), (b"<FOLDER_PATH>".as_slice(), folder)]
            .into_iter().find(|(token, path)| path.is_some() && bytes.starts_with(token));
        if let Some((token, Some(path))) = replacement {
            result.extend_from_slice(path.as_os_str().as_bytes());
            bytes = &bytes[token.len()..];
        } else {
            result.push(bytes[0]);
            bytes = &bytes[1..];
        }
    }
    OsString::from_vec(result)
}
fn append_document(args: &mut Vec<OsString>, document: Option<&Path>) {
    if let Some(path) = document { args.push(path.as_os_str().to_owned()); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use appcui::prelude::{Desktop,DesktopEvents};

    #[Desktop(events=DesktopEvents)]
    struct LaunchMetadataProbe {
        document: PathBuf, directory: PathBuf,
        result: std::sync::Arc<std::sync::Mutex<Option<Result<LaunchPaths,String>>>>,
    }
    impl DesktopEvents for LaunchMetadataProbe {
        fn on_start(&mut self) {
            let options = WindowOptions { resizable: true,close_button: true,fixed_position: false,size: None };
            let terminal = TerminalOptions { padding: None,background_color: None };
            let result = TuiWindow::new_with_context_and_directory("metadata","/bin/sh",["-c","exit 0","<FILE_PATH>","<FOLDER_PATH>"],
                options,terminal,&crate::config::preferences::Preferences::default().terminal,LaunchContext::Document(&self.document),Some(&self.directory))
                .map(|mut window| { let paths = window.launch_paths().clone(); window.cleanup(); paths }).map_err(|error|error.to_string());
            *self.result.lock().unwrap() = Some(result);
        }
    }
    #[test]
    fn review_restore_launch_metadata_preserves_all_opaque_paths_and_rejects_missing_bindings() {
        let paths = LaunchPaths {
            file: Some(PathBuf::from(OsString::from_vec(b"/tmp/file \xff <FOLDER_PATH>".to_vec()))),
            folder: Some(PathBuf::from(OsString::from_vec(b"/tmp/folder \xfe".to_vec()))),
            document: Some(PathBuf::from(OsString::from_vec(b"/tmp/document \xfd".to_vec()))),
            directory: Some(PathBuf::from(OsString::from_vec(b"/tmp/cwd \xfc".to_vec()))),
        };
        let configured: Vec<OsString> = vec!["editor".into(),"--file=<FILE_PATH>".into(),"--folder=<FOLDER_PATH>".into()];
        let (program,args,metadata) = resolve_launch(configured.clone(),LaunchContext::Restore(&paths),Some(Path::new("/ignored"))).unwrap();
        assert_eq!(program,"editor"); assert_eq!(args.len(),3);
        assert_eq!(args[0].as_bytes(),[b"--file=".as_slice(),paths.file.as_ref().unwrap().as_os_str().as_bytes()].concat());
        assert_eq!(args[1].as_bytes(),[b"--folder=".as_slice(),paths.folder.as_ref().unwrap().as_os_str().as_bytes()].concat());
        assert_eq!(args[2],paths.document.as_ref().unwrap().as_os_str());
        assert_eq!(metadata.file,paths.file); assert_eq!(metadata.folder,paths.folder);
        assert_eq!(metadata.document,paths.document); assert_eq!(metadata.directory,paths.directory);
        let (_,restored,_) = resolve_launch(configured,LaunchContext::Restore(&metadata),None).unwrap();
        assert_eq!(restored,args);
        for token in ["<FILE_PATH>","<FOLDER_PATH>"] {
            let error = resolve_launch(vec!["editor".into(),token.into()],LaunchContext::Restore(&LaunchPaths::default()),None).unwrap_err();
            assert!(error.to_string().contains(token)); assert!(error.to_string().contains("restore"));
        }
        let file_only = LaunchPaths { file: paths.file.clone(),..LaunchPaths::default() };
        let (_,args,_) = resolve_launch(vec!["editor".into()],LaunchContext::Restore(&file_only),None).unwrap();
        assert_eq!(args,[paths.file.unwrap().into_os_string()]);
        let current = std::env::current_dir().unwrap();
        let (_,_,inherited) = resolve_launch(vec!["editor".into()],LaunchContext::Default,None).unwrap();
        assert_eq!(inherited.directory,Some(current.clone()));
        let (_,args,relative) = resolve_launch(vec!["editor".into()],LaunchContext::Document(Path::new("explicit document")),Some(Path::new("relative cwd"))).unwrap();
        assert_eq!(relative.directory,Some(current.join("relative cwd")));
        assert_eq!(relative.document,Some(PathBuf::from("explicit document")));
        assert_eq!(args,[OsString::from("explicit document")]);
    }
    #[test]
    fn review_launch_metadata_retains_debug_picker_bindings_document_and_configured_cwd() {
        let _lock = crate::test_support::UI_LOCK.lock().unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap(); let _entered = runtime.enter();
        let dir = crate::test_support::TestDir::new();
        let file = dir.path().join("pick me.txt"); std::fs::write(&file,b"file").unwrap();
        let folder = dir.path().join("picked folder"); std::fs::create_dir(&folder).unwrap();
        let document = dir.path().join("explicit document.txt"); std::fs::write(&document,b"doc").unwrap();
        let directory = dir.path().join("configured cwd"); std::fs::create_dir(&directory).unwrap();
        // The native picker treats Name as relative to its initial folder.
        let home = directories::UserDirs::new().map(|dirs|dirs.home_dir().to_owned()).unwrap_or_else(||"/".into());
        let mut picker_file = PathBuf::new();
        for component in home.components() { if matches!(component,std::path::Component::Normal(_)) { picker_file.push(".."); } }
        picker_file.push(file.strip_prefix("/").unwrap());
        let script = format!("Paint.Enable(false)\nKey.TypeText('{}')\nKey.Pressed(Enter)\nKey.Pressed(Alt+P)\nKey.Pressed(Ctrl+A)\nKey.TypeText('{}')\nKey.Pressed(Enter)\nMouse.Click(68,22,left)",picker_file.display(),folder.display());
        let result = std::sync::Arc::new(std::sync::Mutex::new(None));
        let probe = LaunchMetadataProbe { base: appcui::prelude::Desktop::new(),document: document.clone(),directory: directory.clone(),result: result.clone() };
        let app = appcui::prelude::App::debug(80,30,&script).desktop(probe).build().unwrap();
        app.run(); runtime.block_on(processes::finish_shutdown());
        let paths = result.lock().unwrap().take().unwrap().unwrap();
        assert_eq!(paths.file.as_deref(),Some(file.as_path()));
        assert_eq!(paths.folder.as_deref(),Some(folder.as_path()));
        assert_eq!(paths.document.as_deref(),Some(document.as_path()));
        assert_eq!(paths.directory.as_deref(),Some(directory.as_path()));
        let (_,args,restored) = resolve_launch(vec!["/bin/sh".into(),"-c".into(),"exit 0".into(),"<FILE_PATH>".into(),"<FOLDER_PATH>".into()],LaunchContext::Restore(&paths),None).unwrap();
        assert_eq!(args,[OsString::from("-c"),OsString::from("exit 0"),paths.file.as_ref().unwrap().as_os_str().to_owned(),
            paths.folder.as_ref().unwrap().as_os_str().to_owned(),document.into_os_string()]);
        assert_eq!(restored.directory,paths.directory);
    }
    #[test]
    fn configured_working_directory_does_not_bind_folder_templates() {
        let directory = Path::new("/tmp/configured cwd");
        let context = LaunchContext::Default;
        assert_eq!(context.directory(),None);
        assert_eq!(context.template_directory(),None);
        let (program,args,paths) = resolve_launch(vec!["sh".into(),"literal argument".into()],context,Some(directory)).unwrap();
        assert_eq!(program,"sh"); assert_eq!(args,[OsString::from("literal argument")]);
        assert_eq!(paths.directory.as_deref(),Some(directory)); assert!(paths.folder.is_none());
        assert_eq!(LaunchContext::Folder(directory).template_directory(),Some(directory));
    }
    #[test]
    fn completed_child_drains_queries_and_output_before_clean_close() {
        let _lock = crate::test_support::UI_LOCK.lock().unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap(); let _entered = runtime.enter();
        let app = appcui::prelude::App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let options = WindowOptions { resizable: true, close_button: true, fixed_position: false, size: None };
        let terminal = TerminalOptions { padding: None, background_color: None };
        let mut window = TuiWindow::new("finite child", "/bin/sh", ["-c", "exit 0"], options, terminal,
            &crate::config::preferences::Preferences::default().terminal).unwrap();
        window.set_visible(false);
        let deadline = std::time::Instant::now() + Duration::from_secs(4);
        while !window.process.finished() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        let closed_before_polling = window.is_closing();
        let (tx, rx) = async_channel::bounded(100);
        tx.try_send(Output::Stdout(b"\x1b[6n".to_vec())).unwrap();
        for _ in 0..70 { tx.try_send(Output::Stdout(b"x".to_vec())).unwrap(); }
        tx.try_send(Output::Terminated(Some(0))).unwrap();
        let keyboard = window.custom_keyboard_control;
        window.control_mut(keyboard).unwrap().rx = rx;
        TimerEvents::on_update(&mut window, 0);
        let after_first_tick = (window.closing, window.pending_error.clone());
        TimerEvents::on_update(&mut window, 0);
        let after_second_tick = (window.closing, window.pending_error.clone());
        app.run();
        runtime.block_on(processes::finish_shutdown());
        assert!(!closed_before_polling, "the timer must observe final output/error before removing the window");
        assert_eq!(after_first_tick, (false, None));
        assert_eq!(after_second_tick, (true, None));
    }
    #[test]
    fn closed_input_waits_for_exit_result_but_a_broken_live_writer_is_closed() {
        let _lock = crate::test_support::UI_LOCK.lock().unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap(); let _entered = runtime.enter();
        let app = appcui::prelude::App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let options = WindowOptions { resizable: true, close_button: true, fixed_position: false, size: None };
        let terminal = TerminalOptions { padding: None, background_color: None };
        let mut window = TuiWindow::new("broken writer", "/bin/sh", ["-c", "sleep 60"], options, terminal,
            &crate::config::preferences::Preferences::default().terminal).unwrap();
        window.set_visible(false);
        let keyboard = window.custom_keyboard_control;
        let output = window.control(keyboard).unwrap().rx.clone();
        let deadline = Instant::now() + Duration::from_secs(4);
        while output.is_empty() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        window.control(keyboard).unwrap().tx.close();
        TimerEvents::on_update(&mut window, 0);
        let first = (window.closing, window.pending_error.clone());
        window.input_closed_at = Some(Instant::now() - Duration::from_secs(2));
        TimerEvents::on_update(&mut window, 0);
        let second = (window.closing, window.pending_error.clone());
        app.run();
        runtime.block_on(processes::finish_shutdown());
        assert_eq!(first, (false, None));
        assert!(second.0);
        assert!(second.1.unwrap().contains("input channel has closed"));
    }
    #[test]
    fn output_drain_limits_floods_and_preserves_partial_messages() {
        let (tx, rx) = async_channel::bounded(100);
        for _ in 0..70 { tx.try_send(Output::Stdout(vec![b'x'])).unwrap(); }
        let mut pending = None;
        let mut text = Vec::new();
        let batch = drain_output(&rx, &mut pending, |bytes| text.extend_from_slice(bytes));
        assert!(batch.changed);
        assert_eq!(text.len(), 64);
        assert_eq!(rx.len(), 6);
        while rx.try_recv().is_ok() {}
        tx.try_send(Output::Stdout(vec![b'y'; 300 * 1024])).unwrap();
        text.clear();
        drain_output(&rx, &mut pending, |bytes| text.extend_from_slice(bytes));
        assert_eq!(text.len(), 256 * 1024);
        assert!(text.iter().all(|byte| *byte == b'y'));
        text.clear();
        drain_output(&rx, &mut pending, |bytes| text.extend_from_slice(bytes));
        assert_eq!(text.len(), 44 * 1024);
        assert!(pending.is_none());
    }

    #[test]
    fn terminal_geometry_bounds_padding_and_dimension_casts() {
        for (width, height, padding, expected) in [
            (80, 24, (0, 0), (0, 0, 78, 22)),
            (80, 24, (2, 1), (2, 1, 76, 21)),
            (0, 0, (-10, -20), (0, 0, 1, 1)),
            (3, 3, (i32::MAX, i32::MAX), (0, 0, 1, 1)),
            (u32::MAX, u32::MAX, (0, 0), (0, 0, 10000, 10000)),
        ] {
            let (x, y, size) = terminal_geometry(Size::new(width, height), padding);
            assert_eq!((x, y, size.width, size.height), expected);
        }
    }
    #[test]
    fn contextual_launch_failure_is_deferred_when_hidden_and_releases_its_child() {
        let _lock = crate::test_support::UI_LOCK.lock().unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap(); let _entered = runtime.enter();
        let dir = crate::test_support::TestDir::new();
        for (program,folder) in [(dir.path().join("missing executable"),dir.path().to_owned()),(std::path::PathBuf::from("/bin/sh"),dir.path().join("missing folder"))] {
            let app = appcui::prelude::App::debug(80,24,"Key.Pressed(Enter)").build().unwrap();
            let options = WindowOptions { resizable: true, close_button: true, fixed_position: false, size: None };
            let terminal = TerminalOptions { padding: None, background_color: None };
            let mut window = TuiWindow::new_with_context("test",program.as_os_str(),Vec::<&OsStr>::new(),options,terminal,&crate::config::preferences::Preferences::default().terminal,LaunchContext::Folder(&folder)).unwrap();
            window.set_visible(false);
            let deadline = std::time::Instant::now()+Duration::from_secs(4);
            while window.pending_error.is_none() {
                TimerEvents::on_update(&mut window,0);
                assert!(std::time::Instant::now()<deadline,"launch error was not observed");
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(window.pending_error.as_deref().unwrap().contains("No such file"),
                "the child's launch diagnostic must survive input-channel closure: {:?}", window.pending_error);
            assert!(!window.is_closing(),"hidden error must remain available on restore");
            window.set_visible(true); TimerEvents::on_update(&mut window,0);
            assert!(window.is_closing()); assert!(window.pending_error.is_none());
            app.run();
        }
        runtime.block_on(processes::finish_shutdown());
    }
    #[test]
    fn selected_file_binds_templates_without_reinterpreting_or_duplicating_path() {
        let file = std::path::PathBuf::from(OsString::from_vec(["/tmp/folder 文/opaque ".as_bytes(), b"\xff <FOLDER_PATH>"].concat()));
        let context = LaunchContext::File(&file);
        let configured = vec![OsString::from("editor"),OsString::from("--file=<FILE_PATH>"),OsString::from("<FOLDER_PATH>")];
        let (program,args) = resolve_templates(configured,context).unwrap();
        assert_eq!(program,"editor");
        let mut expected = b"--file=".to_vec(); expected.extend_from_slice(file.as_os_str().as_bytes());
        assert_eq!(args[0].as_bytes(),expected);
        assert_eq!(args[1],OsStr::new("/tmp/folder 文"));
        assert_eq!(args.len(),2);
        let (_,args) = resolve_templates(vec!["editor".into(),"--flag".into()],context).unwrap();
        assert_eq!(args,vec![OsString::from("--flag"),file.as_os_str().to_owned()]);
        assert_eq!(context.directory(),Some(std::path::Path::new("/tmp/folder 文")));
        let (_,args) = resolve_templates(vec!["sh".into(),"<FOLDER_PATH>".into()],LaunchContext::Folder(std::path::Path::new("/tmp/文"))).unwrap();
        assert_eq!(args,vec![OsString::from("/tmp/文")]);
    }
    #[test]
    fn template_paths_are_opaque_and_document_argument_is_appended_once() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let file = std::path::PathBuf::from(std::ffi::OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xff, b' ', b'<', b'F', b'O', b'L', b'D', b'E', b'R', b'_', b'P', b'A', b'T', b'H', b'>']));
        let folder = std::path::PathBuf::from("/tmp/folder");
        let expanded = expand_argument(OsStr::new("--file=<FILE_PATH>:<FOLDER_PATH>"), Some(&file), Some(&folder));
        let mut expected = b"--file=".to_vec();
        expected.extend_from_slice(file.as_os_str().as_bytes());
        expected.extend_from_slice(b":/tmp/folder");
        assert_eq!(expanded.as_bytes(), expected);
        let mut args = vec![expand_argument(OsStr::new("--option"), None, None)];
        append_document(&mut args, Some(&file));
        assert_eq!(args.len(), 2);
        assert_eq!(args[1], file.as_os_str());
    }
}
