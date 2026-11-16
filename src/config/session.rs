use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_SESSION_BYTES: u64 = 1024 * 1024;
pub(crate) const WORKSPACE_COUNT: u8 = 4;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SavedPath(Vec<u8>);
impl SavedPath {
    pub fn new(path: &Path) -> Self { Self(path.as_os_str().as_bytes().to_vec()) }
    pub fn path(&self) -> PathBuf { std::ffi::OsString::from_vec(self.0.clone()).into() }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SavedContext { Default, File, Folder, Document }
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SavedBindings {
    pub file: Option<SavedPath>,
    pub folder: Option<SavedPath>,
    pub document: Option<SavedPath>,
    pub directory: Option<SavedPath>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum SavedLaunch {
    Native { app: String, path: Option<SavedPath> },
    External { launcher: String, #[serde(default)] primary: bool, #[serde(default)] additional: Option<String>, #[serde(default)] bindings: Option<SavedBindings>, context: SavedContext, path: Option<SavedPath> },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SavedWindow {
    pub launch: SavedLaunch,
    pub workspace: u8,
    pub hidden: bool,
    pub x: i32,
    pub y: i32,
    pub width: u16,
    pub height: u16,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Session {
    pub version: u8,
    pub active_workspace: u8,
    pub windows: Vec<SavedWindow>,
}
impl Default for Session {
    fn default() -> Self { Self { version: 1, active_workspace: 0, windows: Vec::new() } }
}
pub(crate) fn session_path() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("com","Julien-cpsn","desktop-tui")?;
    Some(dirs.state_dir().unwrap_or(dirs.data_local_dir()).join("session.toml"))
}
fn validate(session: &Session) -> Result<()> {
    ensure!(session.version == 1,"Unsupported saved workspace version");
    ensure!(session.active_workspace < WORKSPACE_COUNT,"Invalid active workspace");
    ensure!(session.windows.len() <= 256,"Saved workspace exceeds 256 windows");
    for window in &session.windows {
        ensure!(window.workspace < WORKSPACE_COUNT && window.width > 0 && window.height > 0,"Invalid saved window geometry or workspace");
        let path = match &window.launch {
            SavedLaunch::Native { app,path } => { ensure!(app.len() <= 256,"Invalid saved application"); path },
            SavedLaunch::External { launcher,path,.. } => { ensure!(launcher.len() <= 4096,"Invalid saved launcher identity"); path },
        };
        if let Some(path) = path { ensure!(path.0.len() <= 16384 && !path.0.contains(&0),"Invalid saved document path"); }
        if let SavedLaunch::External { bindings: Some(bindings),.. } = &window.launch {
            for path in [&bindings.file,&bindings.folder,&bindings.document,&bindings.directory].into_iter().flatten() {
                ensure!(path.0.len() <= 16384 && !path.0.contains(&0),"Invalid saved launch binding");
            }
        }
    }
    Ok(())
}
pub(crate) fn load(path: &Path) -> Result<Option<Session>> {
    let file = match OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("Cannot read saved workspace"),
    };
    let metadata = file.metadata()?;
    ensure!(metadata.is_file() && metadata.uid() == unsafe { libc::geteuid() },"Saved workspace must be an owned regular file");
    ensure!(metadata.len() <= MAX_SESSION_BYTES,"Saved workspace is too large");
    let mut text = String::new(); file.take(MAX_SESSION_BYTES+1).read_to_string(&mut text)?;
    ensure!(text.len() as u64 <= MAX_SESSION_BYTES,"Saved workspace is too large");
    let session: Session = toml::from_str(&text).context("Cannot parse saved workspace")?;
    validate(&session)?;
    Ok(Some(session))
}
pub(crate) fn save(path: &Path, session: &Session) -> Result<()> {
    validate(session)?;
    let text = toml::to_string_pretty(session)?;
    ensure!(text.len() as u64 <= MAX_SESSION_BYTES,"Saved workspace is too large");
    let parent = path.parent().context("Saved workspace has no parent directory")?;
    fs::DirBuilder::new().recursive(true).mode(0o700).create(parent)?;
    let directory = OpenOptions::new().read(true).custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW).open(parent)?;
    ensure!(directory.metadata()?.uid() == unsafe { libc::geteuid() },"Workspace directory must belong to the current user");
    match fs::symlink_metadata(path) {
        Ok(metadata) => ensure!(metadata.is_file() && metadata.uid() == unsafe { libc::geteuid() },"Saved workspace must be an owned regular file"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
        Err(error) => return Err(error.into()),
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let temporary = parent.join(format!(".session-{}-{}.tmp",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temporary)?;
        file.write_all(text.as_bytes())?; file.sync_all()?;
        fs::rename(&temporary,path)?; directory.sync_all()?;
        Ok(())
    })();
    if result.is_err() { let _ = fs::remove_file(&temporary); }
    result.context("Cannot save workspace")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    #[test]
    fn sessions_round_trip_opaque_paths_and_explicit_visibility() {
        let dir = TestDir::new(); let path = dir.path().join("state/session.toml");
        assert!(load(&path).unwrap().is_none());
        let document = PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/file \xff.txt".to_vec()));
        let session = Session { windows: vec![SavedWindow { launch: SavedLaunch::External { launcher: "editor".into(),primary: false,additional: None,bindings: Some(SavedBindings { document: Some(SavedPath::new(&document)),..SavedBindings::default() }),context: SavedContext::Document,path: Some(SavedPath::new(&document)) },workspace: 2,hidden: true,x: 4,y: 2,width: 60,height: 18 }], ..Session::default() };
        save(&path,&session).unwrap();
        let restored = load(&path).unwrap().unwrap();
        assert_eq!(restored.windows[0].launch,session.windows[0].launch);
        assert!(restored.windows[0].hidden); assert_eq!(restored.windows[0].workspace,2);
        let before = fs::read(&path).unwrap();
        let invalid = Session { version: 2,..session };
        assert!(save(&path,&invalid).is_err()); assert_eq!(fs::read(&path).unwrap(),before);
    }
    #[test]
    fn session_storage_rejects_symlinks_and_invalid_workspaces() {
        let dir = TestDir::new(); let source = dir.path().join("source");
        fs::write(&source,"keep").unwrap(); let link = dir.path().join("link");
        std::os::unix::fs::symlink(&source,&link).unwrap();
        assert!(load(&link).is_err()); assert!(save(&link,&Session::default()).is_err());
        assert_eq!(fs::read_to_string(source).unwrap(),"keep");
        let invalid = Session { active_workspace: 4,..Session::default() };
        assert!(save(&dir.path().join("invalid"),&invalid).is_err());
    }
}
