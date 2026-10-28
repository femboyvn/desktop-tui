use crate::preferences::TerminalPalette;
use crate::shortcut::{BackgroundColor, TerminalOptions, WindowOptions, WindowSize};
use crate::terminal_emulation::TerminalParser;
use anyhow::anyhow;
use appcui::dialogs::{Location, OpenFileDialogFlags, SelectFolderDialogFlags};
use appcui::graphics::{Color, Size, Surface};
use appcui::prelude::window::Flags;
use appcui::prelude::{
    Alignment, Canvas, EventProcessStatus, Handle, LayoutBuilder, OnResize, TimerEvents, Window,
    canvas,
};
use async_channel::{Receiver, Sender};
use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;
use virtual_terminal::{Command, Input, Output};

#[CustomControl(overwrite = OnKeyPressed)]
pub struct CustomKeyboardControl {
    pub should_exit: bool,
    pub tx: Sender<Input>,
    pub rx: Receiver<Output>,
}

#[Window(events = TimerEvents)]
pub struct TuiWindow {
    pub canvas: Handle<Canvas>,
    pub terminal_parser: TerminalParser,
    pub custom_keyboard_control: Handle<CustomKeyboardControl>,
    pub horizontal_adjustment: u32,
    pub vertical_adjustment: u32,
    pending_terminal_size: Option<Size>,
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

        let mut modified_program =
            replace_file_path(program.as_ref().to_str().unwrap().to_string())?;
        modified_program = replace_folder_path(modified_program)?;
        let mut modified_args: Vec<String> = Vec::new();

        for arg in args {
            let mut modified_arg = replace_file_path(arg.as_ref().to_str().unwrap().to_string())?;
            modified_arg = replace_folder_path(modified_arg)?;
            modified_args.push(modified_arg);
        }

        let cmd = Command::new(modified_program)
            .args(modified_args)
            .terminal_size((inner_size.width as usize, inner_size.height as usize));

        let rx = cmd.out_rx();
        let tx = cmd.in_tx();

        tx.send_blocking(Input::Resize((
            inner_size.width as usize,
            inner_size.height as usize,
        )))?;

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

        tui_win.custom_keyboard_control = tui_win.add(CustomKeyboardControl {
            should_exit: false,
            base: ControlBase::new(Layout::fill(), true),
            tx,
            rx,
        });

        tokio::spawn(cmd.run());

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

    pub fn close_command(&mut self) {
        let custom_keyboard_control = self.custom_keyboard_control;
        let control = self.control_mut(custom_keyboard_control).unwrap();
        control.tx.send_blocking(Input::Terminate).ok();
        control.tx.close();
        control.rx.close();
        self.close();
    }
}

impl TimerEvents for TuiWindow {
    fn on_update(&mut self, _: u64) -> EventProcessStatus {
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
                    dialogs::error("An error occurred", &error);

                    self.close();
                    EventProcessStatus::Processed
                }
                Output::Terminated(_) => {
                    self.close();
                    EventProcessStatus::Processed
                }
            },
            Err(_) => EventProcessStatus::Ignored,
        }
    }
}

fn replace_file_path(arg: String) -> anyhow::Result<String> {
    match arg.contains("<FILE_PATH>") {
        false => Ok(arg),
        true => match dialogs::open(
            "Select file",
            "",
            Location::Path(Path::new(env!("HOME"))),
            None,
            OpenFileDialogFlags::Icons | OpenFileDialogFlags::CheckIfFileExists,
        ) {
            None => Err(anyhow!("No file selected")),
            Some(file_path) => Ok(arg.replace("<FILE_PATH>", file_path.to_str().unwrap())),
        },
    }
}

fn replace_folder_path(arg: String) -> anyhow::Result<String> {
    match arg.contains("<FOLDER_PATH>") {
        false => Ok(arg),
        true => match dialogs::select_folder(
            "Select folder",
            Location::Path(Path::new(env!("HOME"))),
            SelectFolderDialogFlags::Icons,
        ) {
            None => Err(anyhow!("No folder selected")),
            Some(file_path) => Ok(arg.replace("<FOLDER_PATH>", file_path.to_str().unwrap())),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
