use std::marker::PhantomData;
use std::sync::Mutex;

use crate::prelude::RuntimeManager;

use super::App;

/// Clipboard access for native controls. Unix CrossTerm uses process-local text;
/// other backends may use a platform clipboard. The ordinary methods require
/// the application/UI thread. The explicit internal-cache methods are bounded,
/// thread-safe and perform no graphical or terminal I/O.
pub struct Clipboard {
    _phantom: PhantomData<*mut ()>,
}

static INTERNAL_TEXT: Mutex<Option<String>> = Mutex::new(None);

impl Clipboard {
    /// Maximum UTF-8 bytes retained in the shared process-local clipboard.
    pub const MAX_INTERNAL_TEXT_BYTES: usize = 256 * 1024;

    /// Shared process-local text, usable from workers without accessing the UI
    /// singleton or a graphical session. CrossTerm native controls use this cache.
    pub fn internal_text() -> Option<String> {
        INTERNAL_TEXT.lock().ok()?.clone()
    }

    /// Rejects oversized text without replacing the previous internal clipboard.
    /// This method performs no host clipboard I/O and is safe on worker threads.
    pub fn set_internal_text(text: &str) -> bool {
        if text.len() > Self::MAX_INTERNAL_TEXT_BYTES { return false; }
        let Ok(mut clipboard) = INTERNAL_TEXT.lock() else { return false; };
        *clipboard = Some(text.to_owned());
        true
    }
    /// Clears the clipboard content
    pub fn clear() {
        if !App::is_created() {
            return;
        }
        RuntimeManager::get().backend_mut().set_clipboard_text("");
    }
    /// Checks if the clipboard has text
    pub fn has_text() -> bool {
        if !App::is_created() {
            return false;
        }
        RuntimeManager::get().backend().has_clipboard_text()
    }
    /// Sets the clipboard text
    pub fn set_text(text: &str) {
        if !App::is_created() {
            return;
        }
        RuntimeManager::get().backend_mut().set_clipboard_text(text);
    }
    /// Gets the clipboard text
    pub fn text() -> Option<String> {
        if !App::is_created() {
            return None;
        }
        RuntimeManager::get().backend_mut().clipboard_text()
    }
}
