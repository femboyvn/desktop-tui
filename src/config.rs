pub(crate) mod preferences;
mod shortcuts;

pub(crate) use shortcuts::{
    BackgroundColor, Shortcut, TerminalOptions, WindowOptions, WindowSize, parse_shortcut_dir,
};

pub(crate) mod catalog;
pub(crate) mod workflow;
pub(crate) mod session;
pub(crate) mod ai;
