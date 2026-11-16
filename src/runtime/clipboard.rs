use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(crate) const MAX_CLIPBOARD_BYTES: usize = appcui::system::Clipboard::MAX_INTERNAL_TEXT_BYTES;
const HELPER_TIMEOUT: Duration = Duration::from_secs(2);
pub(crate) struct ClipboardResult { pub text: Option<String>, pub status: String }
#[derive(Clone, Copy)]
enum Provider { Wayland, Xclip, Xsel }
impl Provider {
    fn command(self, copy: bool) -> Command {
        let (name, args): (&str, &[&str]) = match (self, copy) {
            (Self::Wayland, true) => ("wl-copy", &["--foreground", "--type", "text/plain;charset=utf-8"]),
            (Self::Wayland, false) => ("wl-paste", &["--no-newline", "--type", "text"]),
            (Self::Xclip, true) => ("xclip", &["-quiet", "-selection", "clipboard", "-in"]),
            (Self::Xclip, false) => ("xclip", &["-selection", "clipboard", "-out"]),
            (Self::Xsel, true) => ("xsel", &["--nodetach", "--clipboard", "--input"]),
            (Self::Xsel, false) => ("xsel", &["--clipboard", "--output"]),
        };
        let mut command = Command::new(name); command.args(args); command
    }
}
fn providers() -> Vec<Provider> {
    let mut result = Vec::new();
    if std::env::var_os("WAYLAND_DISPLAY").is_some() { result.push(Provider::Wayland); }
    if std::env::var_os("DISPLAY").is_some() { result.extend([Provider::Xclip, Provider::Xsel]); }
    result
}
#[derive(Default)]
struct ClipboardState { fallback: String, owner: Option<Child>, cancelled: Option<Arc<AtomicBool>>, client_cancelled: Option<Arc<AtomicBool>> }
impl ClipboardState {
    fn perform(&mut self, operation: Operation, providers: &[Provider]) -> ClipboardResult {
        match operation {
            Operation::Copy(text) => {
                if text.len() <= MAX_CLIPBOARD_BYTES { appcui::system::Clipboard::set_internal_text(&text); }
                self.copy(text,providers)
            },
            Operation::Paste => {
                if let Some(text) = appcui::system::Clipboard::internal_text() { self.fallback = text; }
                let result = self.paste(providers);
                if let Some(text) = &result.text { appcui::system::Clipboard::set_internal_text(text); }
                result
            },
        }
    }
    fn is_cancelled(&self) -> bool {
        self.cancelled.as_ref().is_some_and(|flag| flag.load(Ordering::Relaxed))
            || self.client_cancelled.as_ref().is_some_and(|flag| flag.load(Ordering::Relaxed))
    }
    fn copy(&mut self, text: String, providers: &[Provider]) -> ClipboardResult {
        if text.len() > MAX_CLIPBOARD_BYTES { return ClipboardResult { text: None, status: "Copy exceeds 256 KiB; select less text".into() }; }
        self.fallback = text;
        if let Some(mut owner) = self.owner.take() { terminate(&mut owner); }
        let mut failure = "no X11/Wayland clipboard provider available".to_owned();
        for provider in providers {
            match run_provider(provider.command(true), Some(self.fallback.as_bytes()), true, HELPER_TIMEOUT, Some(&|| self.is_cancelled())) {
                Ok((_, owner)) => { self.owner = owner; return ClipboardResult { text: None, status: "Copied to system clipboard".into() }; }
                Err(error) => failure = error,
            }
        }
        ClipboardResult { text: None, status: format!("Copied to internal clipboard ({failure})") }
    }
    fn paste(&mut self, providers: &[Provider]) -> ClipboardResult {
        let mut failure = "no X11/Wayland clipboard provider available".to_owned();
        for provider in providers {
            match run_provider(provider.command(false), None, false, HELPER_TIMEOUT, Some(&|| self.is_cancelled())) {
                Ok((bytes, _)) => match String::from_utf8(bytes) {
                    Ok(text) => return ClipboardResult { text: Some(text), status: "Pasted from system clipboard".into() },
                    Err(_) => failure = "system clipboard is not UTF-8 text".into(),
                },
                Err(error) => failure = error,
            }
        }
        ClipboardResult { text: Some(self.fallback.clone()), status: format!("Pasted from internal clipboard ({failure})") }
    }
}
impl Drop for ClipboardState { fn drop(&mut self) { if let Some(mut owner) = self.owner.take() { terminate(&mut owner); } } }

fn nonblocking(fd: i32) -> Result<(), String> {
    // The descriptors belong exclusively to this helper worker.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}
fn terminate(child: &mut Child) {
    // Foreground providers get a dedicated process group, including descendants.
    unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL); }
    let _ = child.kill(); let _ = child.wait();
}
fn run_provider(mut command: Command, input: Option<&[u8]>, ownership: bool, timeout: Duration,
    cancelled: Option<&dyn Fn() -> bool>) -> Result<(Vec<u8>, Option<Child>), String> {
    command.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped()).stderr(Stdio::null()).process_group(0);
    let mut child = command.spawn().map_err(|error| format!("clipboard provider unavailable: {error}"))?;
    let outcome = (|| {
        let mut stdin = child.stdin.take(); let mut stdout = child.stdout.take();
        if let Some(pipe) = &stdin { nonblocking(pipe.as_raw_fd())?; }
        if let Some(pipe) = &stdout { nonblocking(pipe.as_raw_fd())?; }
        let started = Instant::now(); let mut written = 0; let mut bytes = Vec::new(); let mut eof = false;
        loop {
            if cancelled.is_some_and(|cancelled| cancelled()) { return Err("clipboard operation cancelled".into()); }
            if started.elapsed() >= timeout { return Err("clipboard provider timed out".into()); }
            if let (Some(pipe), Some(input)) = (&mut stdin, input) {
                match pipe.write(&input[written..]) {
                    Ok(count) => written += count,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {},
                    Err(error) => return Err(format!("clipboard write failed: {error}")),
                }
                if written == input.len() { stdin = None; }
            }
            if let Some(pipe) = &mut stdout {
                let mut buffer = [0; 8192];
                for _ in 0..32 {
                    match pipe.read(&mut buffer) {
                        Ok(0) => { eof = true; break; },
                        Ok(count) => {
                            if bytes.len() + count > MAX_CLIPBOARD_BYTES { return Err("system clipboard exceeds 256 KiB".into()); }
                            bytes.extend_from_slice(&buffer[..count]);
                        },
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(error) => return Err(format!("clipboard read failed: {error}")),
                    }
                }
            }
            if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
                if !status.success() { return Err(format!("clipboard provider failed ({status})")); }
                if eof { return Ok((bytes, false)); }
            }
            // Foreground providers retain clipboard ownership until replacement
            // or desktop shutdown. Startup failures are observed before success.
            if ownership && stdin.is_none() && started.elapsed() >= Duration::from_millis(100)
                && child.try_wait().map_err(|error| error.to_string())?.is_none() { return Ok((Vec::new(), true)); }
            std::thread::sleep(Duration::from_millis(5));
        }
    })();
    match outcome {
        Ok((bytes, true)) => Ok((bytes, Some(child))),
        Ok((bytes, false)) => Ok((bytes, None)),
        Err(error) => { terminate(&mut child); Err(error) },
    }
}

enum Operation { Copy(String), Paste }
struct Request { operation: Operation, reply: async_channel::Sender<ClipboardResult>, cancelled: Arc<AtomicBool> }
struct Service { sender: async_channel::Sender<Request>, cancelled: Arc<AtomicBool>, thread: Option<std::thread::JoinHandle<()>> }
static SERVICE: OnceLock<Mutex<Option<Service>>> = OnceLock::new();
/// One bounded service owns graphical clipboard providers for the desktop's lifetime.
/// Clients only enqueue requests and poll; no subprocess or pipe I/O runs on the UI.
pub(crate) struct Clipboard { sender: async_channel::Sender<Request>, reply: async_channel::Sender<ClipboardResult>, result: async_channel::Receiver<ClipboardResult>, cancelled: Arc<AtomicBool> }
impl Clipboard {
    pub(crate) fn new() -> Result<Self, String> {
        let mut service = SERVICE.get_or_init(|| Mutex::new(None)).lock().map_err(|_| "clipboard service unavailable")?;
        if service.is_none() {
            let (sender, receiver) = async_channel::bounded::<Request>(8);
            let cancelled = Arc::new(AtomicBool::new(false)); let worker_cancel = cancelled.clone();
            let thread = std::thread::Builder::new().name("desktop-clipboard".into()).spawn(move || {
                let providers = providers();
                let mut state = ClipboardState::default(); state.cancelled = Some(worker_cancel);
                while let Ok(request) = receiver.recv_blocking() {
                    if state.cancelled.as_ref().is_some_and(|flag| flag.load(Ordering::Relaxed)) { break; }
                    if request.reply.is_closed() { continue; }
                    state.client_cancelled = Some(request.cancelled);
                    let result = state.perform(request.operation, &providers);
                    let _ = request.reply.try_send(result);
                }
            }).map_err(|error| error.to_string())?;
            *service = Some(Service { sender, cancelled, thread: Some(thread) });
        }
        let sender = service.as_ref().ok_or("clipboard service unavailable")?.sender.clone();
        let (reply, result) = async_channel::bounded(8); Ok(Self { sender, reply, result, cancelled: Arc::new(AtomicBool::new(false)) })
    }
    fn submit(&self, operation: Operation) -> Result<(), String> {
        self.sender.try_send(Request { operation, reply: self.reply.clone(), cancelled: self.cancelled.clone() }).map_err(|_| "Clipboard busy; try again".into())
    }
    pub(crate) fn copy(&self, text: String) -> Result<(), String> { self.submit(Operation::Copy(text)) }
    pub(crate) fn paste(&self) -> Result<(), String> { self.submit(Operation::Paste) }
    pub(crate) fn poll(&self) -> Option<ClipboardResult> { self.result.try_recv().ok() }
    #[cfg(test)]
    pub(crate) fn with_result(result: ClipboardResult) -> Self {
        let (sender, _) = async_channel::bounded(1);
        let (reply, results) = async_channel::bounded(1);
        reply.try_send(result).unwrap();
        Self { sender, reply, result: results, cancelled: Arc::new(AtomicBool::new(false)) }
    }

}
impl Drop for Clipboard { fn drop(&mut self) { self.cancelled.store(true, Ordering::Relaxed); self.result.close(); } }
/// Call after the UI event loop. Cancels helper I/O and reaps clipboard owners.
pub(crate) fn finish_shutdown() {
    let service = SERVICE.get().and_then(|service| service.lock().ok()?.take());
    if let Some(mut service) = service {
        service.cancelled.store(true, Ordering::Relaxed); service.sender.close();
        if let Some(thread) = service.thread.take() { let _ = thread.join(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_controls_and_terminal_workers_share_the_bounded_internal_clipboard() {
        let _lock = crate::test_support::UI_LOCK.lock().unwrap();
        let mut state = ClipboardState::default();
        appcui::system::Clipboard::set_internal_text("native 中e\u{301}");
        assert_eq!(state.perform(Operation::Paste,&[]).text.as_deref(),Some("native 中e\u{301}"));
        assert!(state.perform(Operation::Copy("terminal 👩🏽‍💻".into()),&[]).status.contains("internal"));
        assert_eq!(appcui::system::Clipboard::internal_text().as_deref(),Some("terminal 👩🏽‍💻"));
        assert!(state.perform(Operation::Copy("x".repeat(MAX_CLIPBOARD_BYTES+1)),&[]).status.contains("256 KiB"));
        assert_eq!(appcui::system::Clipboard::internal_text().as_deref(),Some("terminal 👩🏽‍💻"));
        appcui::system::Clipboard::set_internal_text("");
        assert_eq!(state.perform(Operation::Paste,&[]).text.as_deref(),Some(""));
    }
    #[test]
    fn provider_failure_oversize_and_cancellation_return_errors() {
        let mut failed = Command::new("/bin/sh"); failed.args(["-c", "exit 4"]);
        assert!(run_provider(failed,None,false,Duration::from_secs(1),None).unwrap_err().contains("failed"));
        let mut large = Command::new("/bin/sh"); large.args(["-c", "printf '%262145s' x"]);
        assert!(run_provider(large,None,false,Duration::from_secs(1),None).unwrap_err().contains("256 KiB"));
        let mut cancelled = Command::new("/bin/sh"); cancelled.args(["-c", "sleep 10"]);
        assert!(run_provider(cancelled,None,false,Duration::from_secs(1),Some(&|| true)).unwrap_err().contains("cancelled"));
    }
    #[test]
    fn foreground_clipboard_ownership_is_retained_until_explicit_cleanup() {
        let mut command = Command::new("/bin/sh"); command.args(["-c", "cat >/dev/null; sleep 10"]);
        let (_,owner) = run_provider(command,Some(b"text"),true,Duration::from_secs(1),None).unwrap();
        let mut owner = owner.expect("clipboard owner must be kept alive");
        assert!(owner.try_wait().unwrap().is_none());
        terminate(&mut owner);
        assert!(owner.try_wait().unwrap().is_some());
    }
    #[test]
    fn unavailable_system_clipboard_retains_bounded_fallback_and_reports_it() {
        let mut state = ClipboardState::default();
        let result = state.copy("中e\u{301}😀".into(), &[]);
        assert!(result.status.contains("internal"));
        let result = state.paste(&[]);
        assert_eq!(result.text.as_deref(), Some("中e\u{301}😀"));
        assert!(result.status.contains("internal"));
        let result = state.copy("x".repeat(MAX_CLIPBOARD_BYTES + 1), &[]);
        assert!(result.status.contains("256 KiB"));
        assert_eq!(state.paste(&[]).text.as_deref(), Some("中e\u{301}😀"));
    }
    #[test]
    fn clipboard_helpers_are_bounded_and_reaped_on_timeout() {
        let mut command = std::process::Command::new("/bin/sh");
        command.args(["-c", "printf 'abc'"]);
        assert_eq!(run_provider(command, None, false, Duration::from_secs(1), None).unwrap().0, b"abc");
        let mut command = std::process::Command::new("/bin/sh");
        command.args(["-c", "sleep 10"]);
        let start = Instant::now();
        assert!(run_provider(command, None, false, Duration::from_millis(30), None).is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
