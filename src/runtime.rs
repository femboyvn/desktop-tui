mod locking;
pub(crate) mod ascii_background;
pub(crate) mod ascii_paint;
mod terminal;

pub(crate) use locking::{LockCredential, MAX_PASSWORD_BYTES};
pub(crate) use terminal::{TerminalParser, TerminalMouseKind, TerminalMouseButton, MAX_TERMINAL_DIMENSION};
pub(crate) mod latest;
pub(crate) mod images;
pub(crate) mod files;
pub(crate) mod processes;
pub(crate) mod music;
pub(crate) mod pdf;

pub(crate) mod clipboard;
pub(crate) mod kanban;

pub(crate) mod ai;
pub(crate) mod activity;
