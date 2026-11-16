mod keyboard;
mod input_queue;
mod window;

pub(crate) use window::{TuiWindow, LaunchContext, LaunchPaths};

/// Desktop commands apply only to the terminal window that originated them.
#[derive(Clone, Copy, Debug)]
pub(crate) enum TerminalAction { CopyMode, Search, Paste, Live }

/// Encode a rebound desktop prefix exactly as terminal keyboard input.
pub(crate) fn encode_desktop_key(key: appcui::input::Key) -> Option<Vec<u8>> {
    keyboard::to_escape_sequence_vec(key, '\0')
}
