pub(crate) mod preferences;
mod shortcuts;

pub(crate) use shortcuts::{
    BackgroundColor, Shortcut, TerminalOptions, WindowOptions, WindowSize, parse_shortcut_dir,
};
