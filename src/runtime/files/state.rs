use anyhow::{Context, bail, ensure};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

const MAX_STATE: u64 = 128 * 1024;
pub(crate) const MAX_FILTER: usize = 512;
const MAX_FAVORITES: usize = 64;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct BrowserState {
    pub location: Option<PathBuf>, pub details: bool, pub hidden: bool, pub filter: String, pub favorites: Vec<PathBuf>,
}
#[derive(Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Stored { version: u8, location: Option<Vec<u8>>, details: bool, hidden: bool, filter: String, favorites: Vec<Vec<u8>> }
impl Default for Stored {
    fn default() -> Self { Self { version: 1, location: None, details: false, hidden: false, filter: String::new(), favorites: Vec::new() } }
}
pub(crate) fn default_state_path() -> anyhow::Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("com","Julien-cpsn","desktop-tui").context("Cannot find the user state directory")?;
    Ok(dirs.state_dir().unwrap_or(dirs.data_local_dir()).join("state/browser.toml"))
}
fn validate(state: &BrowserState) -> anyhow::Result<()> {
    if state.filter.len() > MAX_FILTER || state.favorites.len() > MAX_FAVORITES { bail!("Browser state exceeds its filter/favorite limit"); }
    for path in state.location.iter().chain(state.favorites.iter()) {
        if !path.is_absolute() || path.as_os_str().as_bytes().len() > 4096 || path.as_os_str().as_bytes().contains(&0) { bail!("Invalid saved browser path"); }
    }
    Ok(())
}
pub(crate) fn load(path: &Path) -> anyhow::Result<BrowserState> {
    let mut file = match std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BrowserState::default()),
        Err(error) => return Err(error).with_context(|| format!("Cannot open browser state {} (an owned regular file is required; symlinks are not allowed)",path.display())),
    };
    let metadata = file.metadata().context("Cannot inspect opened browser state")?;
    ensure!(metadata.is_file() && metadata.uid() == unsafe { libc::geteuid() },"Browser state must be an owned regular file: {}",path.display());
    ensure!(metadata.len() <= MAX_STATE,"Browser state exceeds 128 KiB: {}",path.display());
    let mut bytes = Vec::new(); std::io::Read::by_ref(&mut file).take(MAX_STATE+1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_STATE { bail!("Browser state exceeds 128 KiB"); }
    let stored: Stored = toml::from_str(std::str::from_utf8(&bytes)?).context("Invalid browser state")?;
    if stored.version != 1 { bail!("Unsupported browser state version {}",stored.version); }
    let state = BrowserState { location: stored.location.map(|p| PathBuf::from(std::ffi::OsString::from_vec(p))), details: stored.details, hidden: stored.hidden, filter: stored.filter,
        favorites: stored.favorites.into_iter().map(|p| PathBuf::from(std::ffi::OsString::from_vec(p))).collect() };
    validate(&state)?; Ok(state)
}
pub(crate) fn save(path: &Path, state: &BrowserState) -> anyhow::Result<()> {
    validate(state)?;
    let stored = Stored { version: 1, location: state.location.as_ref().map(|p| p.as_os_str().as_bytes().to_vec()), details: state.details, hidden: state.hidden, filter: state.filter.clone(),
        favorites: state.favorites.iter().map(|p| p.as_os_str().as_bytes().to_vec()).collect() };
    let text = toml::to_string(&stored)?; if text.len() as u64 > MAX_STATE { bail!("Browser state exceeds 128 KiB"); }
    let parent = path.parent().context("Browser state has no parent")?; std::fs::create_dir_all(parent)?;
    let (temporary, mut file) = super::operations::temporary_file(parent,"browser-state")?;
    let result = (|| -> anyhow::Result<()> { file.write_all(text.as_bytes())?; file.sync_all()?; std::fs::rename(&temporary,path)?; Ok(()) })();
    if result.is_err() { let _ = std::fs::remove_file(&temporary); }
    result.context("Cannot save browser state")
}
#[derive(Clone)]
pub(crate) enum StateJob { Load, Save(BrowserState) }
type StateResult = Result<Option<BrowserState>,String>;
struct PendingState { request: Option<StateJob>, result: Option<StateResult>, closed: bool }
type SharedState = (std::sync::Mutex<PendingState>,std::sync::Condvar);
type Workers = std::sync::Mutex<Vec<(std::sync::Weak<SharedState>,std::thread::JoinHandle<()>)>>;
static WORKERS: std::sync::OnceLock<Workers> = std::sync::OnceLock::new();
/// Coalesces saves, keeps one result, and drains the last accepted save when a window closes.
pub(crate) struct StateWorker { shared: std::sync::Arc<SharedState> }
impl StateWorker {
    pub fn new(path: PathBuf) -> std::io::Result<Self> {
        let shared = std::sync::Arc::new((std::sync::Mutex::new(PendingState { request: None,result: None,closed: false }),std::sync::Condvar::new()));
        let worker = std::sync::Arc::clone(&shared);
        let mut workers = WORKERS.get_or_init(|| std::sync::Mutex::new(Vec::new())).lock().map_err(|_| std::io::Error::other("Browser state worker registry unavailable"))?;
        let task = std::thread::Builder::new().name("desktop-browser-state".into()).spawn(move || loop {
            let request = {
                let Ok(mut state) = worker.0.lock() else { return; };
                while state.request.is_none() && !state.closed { let Ok(next) = worker.1.wait(state) else { return; }; state = next; }
                let Some(request) = state.request.take() else { return; }; request
            };
            let result = match request { StateJob::Load => load(&path).map(Some), StateJob::Save(state) => save(&path,&state).map(|_| None) }.map_err(|error| format!("{error:#}"));
            if let Ok(mut state) = worker.0.lock() {
                // Keep an error visible even when a newer save was coalesced.
                if state.result.as_ref().is_none_or(|previous| previous.is_ok()) { state.result = Some(result); }
            }
        })?;
        workers.retain(|(_,task)| !task.is_finished());
        workers.push((std::sync::Arc::downgrade(&shared),task));
        Ok(Self { shared })
    }
    pub fn submit(&mut self, job: StateJob) {
        if let Ok(mut state) = self.shared.0.lock() && !state.closed { state.request = Some(job); self.shared.1.notify_one(); }
    }
    pub fn poll(&mut self) -> Option<StateResult> { self.shared.0.lock().ok()?.result.take() }
}
fn close(shared: &SharedState) { if let Ok(mut state) = shared.0.lock() { state.closed = true; shared.1.notify_one(); } }
impl Drop for StateWorker { fn drop(&mut self) { close(&self.shared); } }
/// Called after the UI exits. Drain the last accepted save, then join every live
/// browser-state worker; callbacks only signal close and never wait for disk I/O.
pub(crate) fn finish_shutdown() {
    let workers = WORKERS.get().and_then(|workers| workers.lock().ok().map(|mut workers| std::mem::take(&mut *workers))).unwrap_or_default();
    for (shared,_) in &workers { if let Some(shared) = shared.upgrade() { close(&shared); } }
    for (_,task) in workers { let _ = task.join(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use std::os::unix::ffi::OsStringExt;
    #[test]
    fn fifo_browser_state_returns_error_and_cannot_block_desktop_shutdown() {
        use std::os::unix::fs::FileTypeExt;
        const FIXTURE: &str = "DESKTOP_TUI_BROWSER_FIFO_STATE";
        if let Some(path) = std::env::var_os(FIXTURE) {
            let path = PathBuf::from(path);
            let error = load(&path).unwrap_err(); assert!(format!("{error:#}").contains("regular file"));
            let mut worker = StateWorker::new(path.clone()).unwrap(); worker.submit(StateJob::Load);
            finish_shutdown(); assert!(matches!(worker.poll(),Some(Err(error)) if error.contains("regular file")));
            assert!(std::fs::symlink_metadata(path).unwrap().file_type().is_fifo()); return;
        }
        let dir = TestDir::new(); let path = dir.path().join("browser.toml");
        let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap(); assert_eq!(unsafe { libc::mkfifo(name.as_ptr(),0o600) },0);
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","runtime::files::state::tests::fifo_browser_state_returns_error_and_cannot_block_desktop_shutdown"])
            .env(FIXTURE,&path).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
        let deadline = std::time::Instant::now()+std::time::Duration::from_secs(3); let mut timed_out = false;
        while child.try_wait().unwrap().is_none() {
            if std::time::Instant::now() >= deadline { child.kill().unwrap(); timed_out = true; break; }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let output = child.wait_with_output().unwrap();
        assert!(!timed_out,"FIFO browser state blocked load/shutdown for three seconds; isolated child was killed safely");
        assert!(output.status.success(),"{}{}",String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr));
        assert!(std::fs::symlink_metadata(path).unwrap().file_type().is_fifo());
    }
    #[test]
    fn symlink_browser_state_is_rejected_without_reading_or_replacing_target() {
        let dir = TestDir::new(); let target = dir.path().join("actual.toml"); let link = dir.path().join("browser.toml");
        save(&target,&BrowserState::default()).unwrap(); let original = std::fs::read(&target).unwrap();
        std::os::unix::fs::symlink(&target,&link).unwrap();
        let error = load(&link).unwrap_err(); assert!(format!("{error:#}").contains("Cannot open browser state"));
        assert_eq!(std::fs::read(&target).unwrap(),original); assert_eq!(std::fs::read_link(link).unwrap(),target);
    }
    #[test]
    fn oversized_browser_state_returns_actionable_error_without_changing_the_file() {
        let dir = TestDir::new(); let path = dir.path().join("browser.toml");
        let mut file = std::fs::File::create(&path).unwrap(); file.write_all(b"preserved prefix").unwrap(); file.set_len(MAX_STATE+1).unwrap(); drop(file);
        let original = std::fs::read(&path).unwrap(); let error = load(&path).unwrap_err();
        assert!(format!("{error:#}").contains("128 KiB")); assert_eq!(std::fs::read(path).unwrap(),original);
    }
    #[test]
    fn desktop_shutdown_joins_pending_state_saves_and_closes_live_owners() {
        const FIXTURE: &str = "DESKTOP_TUI_BROWSER_SHUTDOWN_FIXTURE";
        if let Some(root) = std::env::var_os(FIXTURE) {
            let root = PathBuf::from(root);
            let mut live = StateWorker::new(root.join("live/browser.toml")).unwrap();
            live.submit(StateJob::Save(BrowserState { location: Some(root.clone()), details: true,filter: "last accepted".into(),..BrowserState::default() }));
            let mut dropped = StateWorker::new(root.join("dropped/browser.toml")).unwrap();
            dropped.submit(StateJob::Save(BrowserState { filter: "closed window".into(),..BrowserState::default() })); drop(dropped);
            finish_shutdown();
            let saved = load(&root.join("live/browser.toml")).unwrap();
            assert_eq!(saved.location,Some(root.clone())); assert!(saved.details); assert_eq!(saved.filter,"last accepted");
            assert_eq!(load(&root.join("dropped/browser.toml")).unwrap().filter,"closed window");
            live.submit(StateJob::Save(BrowserState::default())); finish_shutdown();
            assert_eq!(load(&root.join("live/browser.toml")).unwrap(),saved);
            assert!(WORKERS.get().unwrap().lock().unwrap().is_empty());
            return;
        }
        // Shutdown is process-global; isolate it from concurrently running UI tests.
        let dir = TestDir::new();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","runtime::files::state::tests::desktop_shutdown_joins_pending_state_saves_and_closes_live_owners"])
            .env(FIXTURE,dir.path()).output().unwrap();
        assert!(output.status.success(),"{}{}",String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr));
    }
    #[test]
    fn closing_state_worker_drains_last_accepted_save_without_blocking_the_owner() {
        let dir = TestDir::new(); let path = dir.path().join("state/browser.toml");
        let mut worker = StateWorker::new(path.clone()).unwrap();
        worker.submit(StateJob::Save(BrowserState { filter: "remember".into(), ..BrowserState::default() })); drop(worker);
        let deadline = std::time::Instant::now()+std::time::Duration::from_secs(3);
        while load(&path).is_ok_and(|state| state.filter != "remember") { assert!(std::time::Instant::now()<deadline); std::thread::yield_now(); }
        assert_eq!(load(&path).unwrap().filter,"remember");
    }
    #[test]
    fn browser_state_round_trips_opaque_paths_and_defaults_without_host_writes() {
        let dir = TestDir::new(); let path = dir.path().join("state/browser.toml");
        assert_eq!(load(&path).unwrap(),BrowserState::default());
        let opaque = PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/\xff".to_vec()));
        let state = BrowserState { location: Some(opaque.clone()), details: true, hidden: true, filter: "文".into(), favorites: vec![opaque] };
        save(&path,&state).unwrap(); assert_eq!(load(&path).unwrap(),state);
    }
    #[test]
    fn bounded_state_rejects_corruption_without_replacing_previous_state() {
        let dir = TestDir::new(); let path = dir.path().join("browser.toml");
        save(&path,&BrowserState::default()).unwrap(); let previous = std::fs::read(&path).unwrap();
        let state = BrowserState { filter: "x".repeat(MAX_FILTER+1), ..BrowserState::default() };
        assert!(save(&path,&state).is_err()); assert_eq!(std::fs::read(&path).unwrap(),previous);
        std::fs::write(&path,"not TOML [").unwrap(); assert!(load(&path).is_err());
    }
}
