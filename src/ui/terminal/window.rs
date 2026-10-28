use super::keyboard::CustomKeyboardControl;
use crate::config::preferences::TerminalPalette;
use crate::config::{BackgroundColor, TerminalOptions, WindowOptions, WindowSize};
use crate::runtime::TerminalParser;
use anyhow::anyhow;
use crate::runtime::processes::{self, ProcessGuard};
use appcui::dialogs::{Location, OpenFileDialogFlags, SelectFolderDialogFlags};
use appcui::graphics::{Color, Size, Surface};
use appcui::prelude::window::Flags;
use appcui::prelude::{
    Alignment, Canvas, EventProcessStatus, Handle, LayoutBuilder, OnResize, TimerEvents, Window,
    canvas, WindowEvents, ActionRequest,
};
use async_channel::Sender;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::Path;
use std::time::Duration;
use virtual_terminal::{Command, Input, Output};

#[Window(events = [TimerEvents, WindowEvents])]
pub struct TuiWindow {
    pub canvas: Handle<Canvas>,
    pub terminal_parser: TerminalParser,
    pub custom_keyboard_control: Handle<CustomKeyboardControl>,
    pub horizontal_adjustment: u32,
    pub vertical_adjustment: u32,
    pending_terminal_size: Option<Size>,
    process: ProcessGuard,
    closing: bool,
    launch_command: String,
    pending_error: Option<String>,
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
        Self::new_with_document(app_name, program, args, window_options, terminal_options, palette, None)
    }

    /// The document path is appended after template resolution and never interpreted as a template.
    pub fn new_with_document<S, I>(
        app_name: &str, program: S, args: I, window_options: WindowOptions,
        terminal_options: TerminalOptions, palette: &TerminalPalette, document: Option<&Path>,
    ) -> anyhow::Result<Self>
    where S: AsRef<OsStr>, I: IntoIterator<Item = S>,
    {
        let window_size = window_options.size.unwrap_or(WindowSize {
            width: 100,
            height: 25,
        });

        let mut x = 0;
        let mut y = 0;
        let mut horizontal_adjustment: i32 = 2;
        let mut vertical_adjustment: i32 = 2;

        if let Some(padding) = terminal_options.padding {
            x = padding.0;
            y = padding.1;

            horizontal_adjustment += padding.0;
            vertical_adjustment += padding.1;
        }

        let inner_size = Size {
            width: window_size
                .width
                .saturating_sub(horizontal_adjustment as u32)
                .max(1),
            height: window_size
                .height
                .saturating_sub(vertical_adjustment as u32)
                .max(1),
        };

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
        let (modified_program, mut modified_args) = resolve_templates(configured)?;
        append_document(&mut modified_args, document);
        let launch_command = modified_program.to_string_lossy().into_owned();
        let cmd = Command::new(modified_program).args(modified_args)
            .terminal_size((inner_size.width as usize, inner_size.height as usize));
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
            terminal_parser: TerminalParser::with_palette(
                inner_size.width,
                inner_size.height,
                palette,
                default_background_color,
            ),
            horizontal_adjustment: horizontal_adjustment as u32,
            vertical_adjustment: vertical_adjustment as u32,
            pending_terminal_size: None,
            process, closing: false, launch_command, pending_error: None,
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

        let surface = tui_win.terminal_parser.new_surface();
        let canvas = tui_win.canvas;
        *tui_win.control_mut(canvas).unwrap().drawing_surface_mut() = surface;

        let timer = match tui_win.timer() {
            Some(t) => t,
            None => return Err(anyhow!("Failed to get timer")),
        };
        timer.start(Duration::from_millis(25));

        tui_win.custom_keyboard_control = tui_win.add(CustomKeyboardControl::new(tx, rx));

        Ok(tui_win)
    }

    pub fn apply_palette(&mut self, palette: &TerminalPalette) {
        if let Some(canvas) = self.base.control_mut(self.canvas) {
            self.terminal_parser
                .apply_palette(palette, canvas.drawing_surface_mut());
        }
    }

    fn sync_terminal_size(&mut self) {
        let size = self.size();
        let width = size.width.saturating_sub(self.horizontal_adjustment).max(1);
        let height = size.height.saturating_sub(self.vertical_adjustment).max(1);
        let canvas = self.base.control_mut(self.canvas).unwrap();
        if canvas.drawing_surface_mut().size() != Size::new(width, height) {
            self.terminal_parser
                .resize_surface(width, height, canvas.drawing_surface_mut());
            canvas.set_size(width as u16, height as u16);
            self.pending_terminal_size = Some(Size::new(width, height));
        }
        let control = self.base.control(self.custom_keyboard_control).unwrap();
        send_pending_resize(&mut self.pending_terminal_size, &control.tx);
    }

    pub fn is_closing(&self) -> bool { self.pending_error.is_none() && (self.closing || self.process.finished()) }

    fn cleanup(&mut self) {
        if self.closing { return; }
        self.closing = true;
        self.process.close();
        let handle = self.custom_keyboard_control;
        if let Some(control) = self.control_mut(handle) {
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
        self.sync_terminal_size();
        let (should_close, rx_clone) = {
            let ckc = self.control(self.custom_keyboard_control).unwrap();

            (ckc.should_exit, ckc.rx.clone())
        };

        if should_close {
            self.close_command();
            return EventProcessStatus::Processed;
        }

        match rx_clone.try_recv() {
            Ok(msg) => match msg {
                Output::Pid(_) => EventProcessStatus::Ignored,
                Output::Stdout(command_output) => {
                    let canvas = self.base.control_mut(self.canvas).unwrap();
                    let old_surface =
                        std::mem::replace(canvas.drawing_surface_mut(), Surface::new(1, 1));
                    *canvas.drawing_surface_mut() = self
                        .terminal_parser
                        .parse_to_surface(&command_output, old_surface);

                    EventProcessStatus::Processed
                }
                Output::Error(error) => {
                    if self.is_visible() {
                        self.show_launch_error(&error);
                        self.close_command();
                    } else {
                        self.pending_error = Some(error);
                        self.cleanup();
                    }
                    EventProcessStatus::Processed
                }
                Output::Terminated(_) => {
                    self.close_command();
                    EventProcessStatus::Processed
                }
            },
            Err(_) => EventProcessStatus::Ignored,
        }
    }
}

fn resolve_templates(mut configured: Vec<OsString>) -> anyhow::Result<(OsString, Vec<OsString>)> {
    let contains = |token: &[u8]| configured.iter().any(|arg| arg.as_bytes().windows(token.len()).any(|window| window == token));
    let home = directories::UserDirs::new().map(|dirs| dirs.home_dir().to_owned()).unwrap_or_else(|| "/".into());
    let file = if contains(b"<FILE_PATH>") {
        Some(dialogs::open("Select file", "", Location::Path(&home), None,
            OpenFileDialogFlags::Icons | OpenFileDialogFlags::CheckIfFileExists).ok_or_else(|| anyhow!("No file selected"))?)
    } else { None };
    let folder = if contains(b"<FOLDER_PATH>") {
        Some(dialogs::select_folder("Select folder", Location::Path(&home), SelectFolderDialogFlags::Icons)
            .ok_or_else(|| anyhow!("No folder selected"))?)
    } else { None };
    configured = configured.iter().map(|arg| expand_argument(arg, file.as_deref(), folder.as_deref())).collect();
    let program = configured.remove(0);
    Ok((program, configured))
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
    #[test]
    fn terminal_resize_is_retried_after_input_queue_drains() {
        let (tx, rx) = async_channel::bounded(1);
        tx.try_send(Input::Data(vec![b'a'])).unwrap();
        let mut pending = Some(Size::new(60, 12));
        send_pending_resize(&mut pending, &tx);
        assert_eq!(pending, Some(Size::new(60, 12)));
        assert_eq!(rx.try_recv().unwrap(), Input::Data(vec![b'a']));
        send_pending_resize(&mut pending, &tx);
        assert_eq!(pending, None);
        assert_eq!(rx.try_recv().unwrap(), Input::Resize((60, 12)));
        send_pending_resize(&mut pending, &tx);
        assert!(rx.try_recv().is_err());
    }
}

fn send_pending_resize(pending: &mut Option<Size>, tx: &Sender<Input>) {
    if let Some(size) = *pending {
        match tx.try_send(Input::Resize((size.width as usize, size.height as usize))) {
            Ok(()) | Err(async_channel::TrySendError::Closed(_)) => *pending = None,
            Err(async_channel::TrySendError::Full(_)) => {}
        }
    }
}
