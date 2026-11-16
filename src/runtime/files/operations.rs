use anyhow::{Context, bail, ensure};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Fingerprint { dev: u64, ino: u64, mode: u32, size: u64, modified: (i64,i64), changed: (i64,i64) }
impl Fingerprint {
    fn after_rename_matches(&self, other: &Self) -> bool { self.dev == other.dev && self.ino == other.ino && self.mode == other.mode && self.size == other.size && self.modified == other.modified }
    fn of(metadata: &std::fs::Metadata) -> Self { Self { dev: metadata.dev(), ino: metadata.ino(), mode: metadata.mode(), size: metadata.len(), modified: (metadata.mtime(),metadata.mtime_nsec()), changed: (metadata.ctime(),metadata.ctime_nsec()) } }
}
#[derive(Clone, Debug)]
pub(crate) enum Operation {
    Create { path: PathBuf, directory: bool },
    Transfer { source: PathBuf, destination: PathBuf, moving: bool, approval: Option<(Fingerprint,Fingerprint)> },
    Trash { source: PathBuf, root: Option<PathBuf> },
}
#[derive(Debug)]
pub(crate) enum Outcome { Complete(String), Confirm(Operation), Failed(String) }
#[derive(Default)]
struct Control { cancel: AtomicBool, closed: AtomicBool, bytes: AtomicU64, entries: AtomicU64 }
type Workers = Mutex<Vec<(std::sync::Weak<Control>,std::thread::JoinHandle<()>)>>;
static WORKERS: std::sync::OnceLock<Workers> = std::sync::OnceLock::new();
impl Control {
    fn check(&self) -> anyhow::Result<()> { ensure!(!self.cancel.load(Ordering::Acquire),"Operation canceled; source preserved"); Ok(()) }
}
/// A single accepted mutation and one bounded result; no replaceable mutation queue.
/// Closing cancels before publication and never waits for filesystem I/O on the UI thread.
pub(crate) struct OperationWorker {
    sender: std::sync::mpsc::SyncSender<Operation>, result: Arc<Mutex<Option<Outcome>>>, control: Arc<Control>, busy: bool,
}
impl OperationWorker {
    pub fn new() -> std::io::Result<Self> {
        let (sender,receiver) = std::sync::mpsc::sync_channel(1);
        let result = Arc::new(Mutex::new(None)); let control = Arc::new(Control::default());
        let shared = Arc::clone(&result); let state = Arc::clone(&control);
        let mut workers = WORKERS.get_or_init(|| Mutex::new(Vec::new())).lock().map_err(|_| std::io::Error::other("File operation worker registry unavailable"))?;
        let task = std::thread::Builder::new().name("desktop-file-operation".into()).spawn(move || {
            while !state.closed.load(Ordering::Acquire) {
                // A live UI owner may still hold the sender after the event loop.
                // Bound idle receive so global shutdown can close and join it too.
                let operation = match receiver.recv_timeout(std::time::Duration::from_millis(40)) {
                    Ok(operation) => operation,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                };
                if state.closed.load(Ordering::Acquire) { break; }
                let outcome = execute(operation,&state);
                if let Ok(mut slot) = shared.lock() { *slot = Some(outcome); }
            }
        })?;
        workers.retain(|(_,task)| !task.is_finished()); workers.push((Arc::downgrade(&control),task));
        Ok(Self { sender, result, control, busy: false })
    }
    pub fn submit(&mut self, operation: Operation) -> anyhow::Result<()> {
        ensure!(!self.control.closed.load(Ordering::Acquire),"File operation worker is closed");
        ensure!(!self.busy,"Another file operation is running");
        self.control.cancel.store(false,Ordering::Release); self.control.bytes.store(0,Ordering::Relaxed); self.control.entries.store(0,Ordering::Relaxed);
        self.sender.try_send(operation).context("Cannot start file operation")?; self.busy = true; Ok(())
    }
    pub fn poll(&mut self) -> Option<Outcome> {
        let result = self.result.lock().ok()?.take(); if result.is_some() { self.busy = false; } result
    }
    pub fn busy(&self) -> bool { self.busy }
    pub fn progress(&self) -> (u64,u64) { (self.control.entries.load(Ordering::Relaxed),self.control.bytes.load(Ordering::Relaxed)) }
    pub fn cancel(&self) { self.control.cancel.store(true,Ordering::Release); }
}
fn close(control: &Control) {
    control.closed.store(true,Ordering::Release); control.cancel.store(true,Ordering::Release);
}
impl Drop for OperationWorker { fn drop(&mut self) { close(&self.control); } }
/// Cancel and join outside the UI loop, including workers whose window owners
/// remain alive. Pending staging cleanup and replacement recovery finish first.
pub(super) fn finish_shutdown() {
    let workers = WORKERS.get().and_then(|workers| workers.lock().ok().map(|mut workers| std::mem::take(&mut *workers))).unwrap_or_default();
    for (control,_) in &workers { if let Some(control) = control.upgrade() { close(&control); } }
    for (_,task) in workers { let _ = task.join(); }
}

fn fingerprint(path: &Path) -> anyhow::Result<Fingerprint> { Ok(Fingerprint::of(&std::fs::symlink_metadata(path).with_context(|| format!("Cannot inspect {}",path.display()))?)) }
fn maybe_fingerprint(path: &Path) -> anyhow::Result<Option<Fingerprint>> {
    match std::fs::symlink_metadata(path) { Ok(metadata) => Ok(Some(Fingerprint::of(&metadata))), Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None), Err(error) => Err(error).context("Cannot inspect destination") }
}
static NEXT: AtomicU64 = AtomicU64::new(0);
fn unique(parent: &Path, prefix: &str) -> PathBuf { parent.join(format!(".desktop-tui-{prefix}-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed))) }
pub(super) fn temporary_file(parent: &Path, prefix: &str) -> anyhow::Result<(PathBuf,std::fs::File)> {
    for _ in 0..100 {
        let path = unique(parent,prefix);
        match std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path) {
            Ok(file) => return Ok((path,file)), Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue, Err(error) => return Err(error).context("Cannot create temporary file"),
        }
    }
    bail!("Cannot allocate a unique temporary file")
}
struct Stage { path: PathBuf }
impl Stage {
    fn new(parent: &Path) -> anyhow::Result<Self> {
        use std::os::unix::fs::DirBuilderExt;
        for _ in 0..100 {
            let path = unique(parent,"stage");
            match std::fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Ok(Self { path }), Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue, Err(error) => return Err(error).context("Cannot stage destination"),
            }
        }
        bail!("Cannot allocate a staging directory")
    }
    fn cleanup(&self) -> anyhow::Result<()> {
        if !self.path.try_exists()? { return Ok(()); }
        prepare_staging_cleanup(&self.path)?;
        std::fs::remove_dir_all(&self.path).with_context(|| format!("Cannot remove staging directory {}",recovery_path(&self.path).display()))
    }
    fn cleanup_error(&self, error: anyhow::Error) -> anyhow::Error {
        match self.cleanup() {
            Ok(()) => error,
            Err(cleanup) => error.context(format!("Unpublished staging retained at {}: {cleanup:#}",recovery_path(&self.path).display())),
        }
    }
}
fn prepare_staging_cleanup(path: &Path) -> anyhow::Result<()> {
    // O_PATH opens even mode-000 directories; O_NOFOLLOW makes a replaced entry
    // inspectable without ever chmodding the target of a symlink.
    let entry = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_PATH | libc::O_NOFOLLOW).open(path)?;
    let metadata = entry.metadata()?;
    if !metadata.is_dir() { return Ok(()); }
    ensure!(metadata.uid() == unsafe { libc::geteuid() },"Staging directory ownership changed");
    let anchored = directory_path(&entry);
    std::fs::set_permissions(&anchored,std::fs::Permissions::from_mode(metadata.mode() | 0o700))?;
    for child in std::fs::read_dir(&anchored)? { prepare_staging_cleanup(&child?.path())?; }
    Ok(())
}
impl Drop for Stage {
    fn drop(&mut self) {
        // Stage owns only unpublished copies inside its private directory.
        let _ = self.cleanup();
    }
}
// NOREPLACE prevents a concurrent new destination from being silently overwritten.
fn rename_new(from: &Path, to: &Path) -> std::io::Result<()> {
    let from = std::ffi::CString::new(from.as_os_str().as_bytes())?; let to = std::ffi::CString::new(to.as_os_str().as_bytes())?;
    let status = unsafe { libc::renameat2(libc::AT_FDCWD,from.as_ptr(),libc::AT_FDCWD,to.as_ptr(),libc::RENAME_NOREPLACE) };
    if status == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
}
fn absolute_entry(path: &Path) -> anyhow::Result<PathBuf> {
    let name = path.file_name().context("Select a named entry, not a filesystem root")?;
    ensure!(name != "." && name != "..","Select a named entry");
    Ok(path.parent().unwrap_or(Path::new(".")).canonicalize()?.join(name))
}
fn open_directory(path: &Path) -> anyhow::Result<std::fs::File> {
    std::fs::OpenOptions::new().read(true).custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW).open(path).with_context(|| format!("Cannot anchor directory {}",path.display()))
}
fn directory_path(file: &std::fs::File) -> PathBuf {
    use std::os::fd::AsRawFd;
    PathBuf::from(format!("/proc/self/fd/{}",file.as_raw_fd()))
}
// Hold directory descriptors across work. Replacing an ancestor cannot redirect
// publication, cleanup, or source reads into a different directory tree.
struct PinnedEntry { parent: std::fs::File, original: PathBuf, path: PathBuf }
impl PinnedEntry {
    fn new(path: &Path) -> anyhow::Result<Self> {
        let original = absolute_entry(path)?; let mut parent = open_directory(Path::new("/"))?;
        for component in original.parent().context("No entry parent")?.components() {
            if let std::path::Component::Normal(name) = component { parent = open_directory(&directory_path(&parent).join(name))?; }
        }
        let path = directory_path(&parent).join(original.file_name().context("No entry name")?);
        Ok(Self { parent, original, path })
    }
    fn path(&self) -> &Path { &self.path }
    fn unchanged(&self) -> anyhow::Result<()> {
        let expected = self.parent.metadata()?; let actual = std::fs::metadata(self.original.parent().context("No parent")?)?;
        ensure!(expected.dev() == actual.dev() && expected.ino() == actual.ino(),"Source or destination parent changed; operation stopped"); Ok(())
    }
}
fn anchored_child(base: &Path, relative: &Path) -> anyhow::Result<(Option<std::fs::File>,PathBuf)> {
    if relative.as_os_str().is_empty() { return Ok((None,base.to_owned())); }
    let mut parent = open_directory(base)?;
    let components = relative.components().collect::<Vec<_>>();
    for component in &components[..components.len()-1] { parent = open_directory(&directory_path(&parent).join(component.as_os_str()))?; }
    let path = directory_path(&parent).join(components.last().context("No child name")?.as_os_str());
    Ok((Some(parent),path))
}
fn recovery_path(path: &Path) -> PathBuf {
    let components = path.components().collect::<Vec<_>>();
    if components.len() >= 5 && path.starts_with("/proc/self/fd") {
        let descriptor = components[..5].iter().fold(PathBuf::new(),|mut path,part| { path.push(part.as_os_str()); path });
        if let Ok(mut actual) = std::fs::read_link(descriptor) { for component in &components[5..] { actual.push(component.as_os_str()); } return actual; }
    }
    path.to_owned()
}
fn child_path(base: &Path, relative: &Path) -> PathBuf { if relative.as_os_str().is_empty() { base.to_owned() } else { base.join(relative) } }
type Manifest = BTreeMap<PathBuf,Fingerprint>;
fn manifest(path: &Path, control: &Control) -> anyhow::Result<Manifest> {
    fn walk(path: &Path, relative: &Path, depth: usize, entries: &mut Manifest, control: &Control) -> anyhow::Result<()> {
        control.check()?; ensure!(depth <= 128 && entries.len() < 100_000,"Folder exceeds the 100000-entry/128-level operation limit");
        let metadata = std::fs::symlink_metadata(path)?;
        ensure!(metadata.is_file() || metadata.is_dir() || metadata.file_type().is_symlink(),"Special files cannot be transferred");
        entries.insert(relative.to_owned(),Fingerprint::of(&metadata));
        if metadata.is_dir() {
            let directory = open_directory(path)?;
            ensure!(Fingerprint::of(&directory.metadata()?) == Fingerprint::of(&metadata),"Source directory changed during traversal");
            for entry in std::fs::read_dir(directory_path(&directory))? { let entry = entry?; walk(&entry.path(),&relative.join(entry.file_name()),depth+1,entries,control)?; }
        }
        Ok(())
    }
    let mut entries = BTreeMap::new(); walk(path,Path::new(""),0,&mut entries,control)?; Ok(entries)
}
fn copy_tree(source: &Path, target: &Path, expected: &Manifest, control: &Control) -> anyhow::Result<()> {
    for (relative,identity) in expected {
        control.check()?; let (_parent,from) = anchored_child(source,relative)?; let to = child_path(target,relative);
        ensure!(fingerprint(&from)? == *identity,"Source changed during transfer; source preserved");
        let metadata = std::fs::symlink_metadata(&from)?;
        if metadata.is_dir() { std::fs::create_dir(&to)?; }
        else if metadata.file_type().is_symlink() { std::os::unix::fs::symlink(std::fs::read_link(&from)?,&to)?; }
        else {
            let mut input = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(&from)?;
            ensure!(Fingerprint::of(&input.metadata()?) == *identity,"Source changed before reading");
            let mut output = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&to)?;
            let mut buffer = [0u8; 128*1024];
            loop { control.check()?; let read = input.read(&mut buffer)?; if read == 0 { break; } output.write_all(&buffer[..read])?; control.bytes.fetch_add(read as u64,Ordering::Relaxed); }
            ensure!(Fingerprint::of(&input.metadata()?) == *identity,"Source changed while copying; source preserved");
            output.set_permissions(std::fs::Permissions::from_mode(metadata.mode() & 0o777))?; output.sync_all()?;
        }
        control.entries.fetch_add(1,Ordering::Relaxed);
    }
    // Apply directory permissions after children so read-only source directories can be copied.
    for (relative,identity) in expected.iter().rev() {
        if identity.mode & libc::S_IFMT == libc::S_IFDIR { std::fs::set_permissions(child_path(target,relative),std::fs::Permissions::from_mode((identity.mode & 0o777) | if relative.as_os_str().is_empty() { 0o200 } else { 0 }))?; }
    }
    ensure!(*expected == manifest(source,control)?,"Source tree changed while copying; source preserved"); Ok(())
}
fn restore_backup(backup: &Path, destination: &Path) -> anyhow::Result<()> {
    rename_new(backup,destination).with_context(|| format!("Previous destination is preserved at {} because its original path changed",recovery_path(backup).display()))?; Ok(())
}
fn publish(staged: &Path, destination: &Path, expected: Option<&Fingerprint>, control: &Control) -> anyhow::Result<Option<PathBuf>> {
    control.check()?;
    ensure!(maybe_fingerprint(destination)?.as_ref() == expected,"Destination changed; transfer stopped");
    let backup = if let Some(expected) = expected {
        let backup = unique(destination.parent().context("No destination parent")?,"replaced");
        rename_new(destination,&backup).context("Cannot retain previous destination")?;
        if !expected.after_rename_matches(&fingerprint(&backup)?) {
            restore_backup(&backup,destination)?; bail!("Destination changed before replacement; transfer stopped");
        }
        Some(backup)
    } else { None };
    if let Err(error) = control.check().and_then(|_| rename_new(staged,destination).context("Cannot publish destination; source preserved")) {
        if let Some(backup) = &backup { restore_backup(backup,destination)?; }
        return Err(error);
    }
    Ok(backup)
}
fn transfer(source: PathBuf, destination: PathBuf, moving: bool, approval: Option<(Fingerprint,Fingerprint)>, control: &Control) -> anyhow::Result<Outcome> {
    let source = absolute_entry(&source)?; let destination = absolute_entry(&destination)?;
    ensure!(source != destination && !destination.starts_with(&source),"Destination must be outside the source");
    let original_source = source.clone(); let original_destination = destination.clone();
    let source_pin = PinnedEntry::new(&source)?; let destination_pin = PinnedEntry::new(&destination)?;
    let source = source_pin.path().to_owned(); let destination = destination_pin.path().to_owned();
    let source_identity = fingerprint(&source)?; let destination_identity = maybe_fingerprint(&destination)?;
    if let Some(identity) = &destination_identity {
        ensure!(identity.dev != source_identity.dev || identity.ino != source_identity.ino,"Source and destination refer to the same entry");
        ensure!(identity.mode & libc::S_IFMT != libc::S_IFDIR,"Choose a new destination: existing folders are never merged or replaced");
        if approval.is_none() {
            return Ok(Outcome::Confirm(Operation::Transfer { source: original_source, destination: original_destination, moving, approval: Some((source_identity,identity.clone())) }));
        }
    }
    if let Some((expected_source,expected_destination)) = &approval {
        ensure!(*expected_source == source_identity && destination_identity.as_ref() == Some(expected_destination),"Source or destination changed after confirmation; transfer stopped");
    }
    source_pin.unchanged()?; destination_pin.unchanged()?;
    if moving && std::fs::metadata(destination.parent().context("No destination parent")?)?.dev() == source_identity.dev {
        ensure!(fingerprint(&source)? == source_identity,"Source changed before move; source preserved");
        let backup = publish(&source,&destination,destination_identity.as_ref(),control)?;
        if !source_identity.after_rename_matches(&fingerprint(&destination)?) {
            rename_new(&destination,&source).context("Source changed during move; moved entry retained at destination")?;
            if let Some(backup) = &backup { restore_backup(backup,&destination)?; }
            bail!("Source changed before move; source preserved");
        }
        let mut message = format!("Moved to {}",original_destination.display());
        if let Some(backup) = backup && let Err(error) = std::fs::remove_file(&backup) { message.push_str(&format!(". Previous destination retained at {}: {error}",recovery_path(&backup).display())); }
        control.entries.store(1,Ordering::Relaxed);
        return Ok(Outcome::Complete(message));
    }
    let expected = manifest(&source,control)?;
    let stage = Stage::new(destination.parent().context("No destination parent")?)?; let staged = stage.path.join("entry");
    let backup = (|| -> anyhow::Result<Option<PathBuf>> {
        copy_tree(&source,&staged,&expected,control)?;
        source_pin.unchanged()?; destination_pin.unchanged()?;
        publish(&staged,&destination,destination_identity.as_ref(),control)
    })().map_err(|error| stage.cleanup_error(error))?;
    let mut message = format!("Copied to {}",original_destination.display());
    if source_identity.mode & libc::S_IFMT == libc::S_IFDIR && let Err(error) = std::fs::set_permissions(&destination,std::fs::Permissions::from_mode(source_identity.mode & 0o777)) { message.push_str(&format!(". Could not restore directory permissions: {error}")); }
    if moving {
        // Rename first. If the tree changed after publication, retain it intact rather than deleting changed content.
        let held = unique(source.parent().context("No source parent")?,"move-source");
        match control.check().and_then(|_| {
            ensure!(manifest(&source,control)? == expected,"Source changed; published copy retained and source preserved");
            rename_new(&source,&held).context("Destination published; cannot remove source")?;
            let actual = manifest(&held,&Control::default());
            // A rename updates only the root ctime; descendants must still match exactly.
            let unchanged = actual.as_ref().is_ok_and(|actual| actual.len() == expected.len() && actual.iter().all(|(path,value)| expected.get(path).is_some_and(|old| if path.as_os_str().is_empty() { old.dev == value.dev && old.ino == value.ino && old.mode == value.mode && old.size == value.size && old.modified == value.modified } else { old == value })));
            if !unchanged { rename_new(&held,&source).with_context(|| format!("Changed source retained at {}",recovery_path(&held).display()))?; bail!("Source changed; published copy retained and source preserved"); }
            let removal = if source_identity.mode & libc::S_IFMT == libc::S_IFDIR { std::fs::remove_dir_all(&held) } else { std::fs::remove_file(&held) };
            if let Err(error) = removal { bail!("Destination published; source remainder retained at {}: {error}",recovery_path(&held).display()); }
            Ok(())
        }) {
            Ok(()) => message = format!("Moved to {}",original_destination.display()),
            Err(error) => message = format!("{message}. Move incomplete: {error:#}"),
        }
    }
    if let Some(backup) = backup {
        // Retained backups are files/symlinks only, never recursively removed.
        if let Err(error) = std::fs::remove_file(&backup) { message.push_str(&format!(". Previous destination retained at {}: {error}",recovery_path(&backup).display())); }
    }
    if let Err(error) = stage.cleanup() { message.push_str(&format!(". Staging retained at {}: {error:#}",recovery_path(&stage.path).display())); }
    Ok(Outcome::Complete(message))
}
fn trash(source: PathBuf, root: Option<PathBuf>, control: &Control) -> anyhow::Result<Outcome> {
    use std::os::unix::fs::DirBuilderExt;
    let original_source = absolute_entry(&source)?; let source_pin = PinnedEntry::new(&original_source)?;
    let source = source_pin.path().to_owned(); let identity = fingerprint(&source)?;
    let root = match root { Some(root) => root, None => directories::BaseDirs::new().context("Cannot find user trash directory")?.data_local_dir().join("Trash") };
    ensure!(!root.starts_with(&original_source),"Cannot trash a folder containing the user trash");
    for path in [root.clone(),root.join("files"),root.join("info")] {
        match std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&path) { Ok(()) => {}, Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}, Err(error) => return Err(error).context("User trash is unavailable; source preserved") }
        let metadata = std::fs::symlink_metadata(&path)?;
        ensure!(metadata.is_dir() && metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o077 == 0,"User trash must be a private directory owned by this user");
    }
    ensure!(std::fs::metadata(root.join("files"))?.dev() == identity.dev,"Trash is on another filesystem; source preserved. No permanent-delete fallback is used");
    let name = format!("desktop-tui-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed));
    let files_directory = open_directory(&root.join("files"))?; let info_directory = open_directory(&root.join("info"))?;
    let info = directory_path(&info_directory).join(format!("{name}.trashinfo")); let destination = directory_path(&files_directory).join(name);
    let encoded = original_source.as_os_str().as_bytes().iter().map(|b| if b.is_ascii_alphanumeric() || b"/-_.~".contains(b) { (*b as char).to_string() } else { format!("%{b:02X}") }).collect::<String>();
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&info)?;
    let result = (|| -> anyhow::Result<()> {
        writeln!(file,"[Trash Info]\nPath={encoded}\nDeletionDate={}",chrono::Local::now().format("%Y-%m-%dT%H:%M:%S"))?; file.sync_all()?; control.check()?;
        source_pin.unchanged()?;
        ensure!(fingerprint(&source)? == identity,"Source changed before trash; source preserved");
        rename_new(&source,&destination).context("Cannot move to user trash; source preserved")?; Ok(())
    })();
    if result.is_err() { let _ = std::fs::remove_file(&info); }
    result?; Ok(Outcome::Complete(format!("Moved to user trash; restoration metadata: {}",recovery_path(&info).display())))
}
fn execute(operation: Operation, control: &Control) -> Outcome {
    let result = (|| -> anyhow::Result<Outcome> {
        control.check()?;
        match operation {
            Operation::Create { path, directory } => {
                if directory { std::fs::create_dir(&path).with_context(|| format!("Cannot create folder {}",path.display()))?; }
                else { std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path).with_context(|| format!("Cannot create file {}",path.display()))?; }
                Ok(Outcome::Complete(format!("Created {}",path.display())))
            }
            Operation::Transfer { source, destination, moving, approval } => transfer(source,destination,moving,approval,control),
            Operation::Trash { source, root } => trash(source,root,control),
        }
    })();
    result.unwrap_or_else(|error| Outcome::Failed(format!("{error:#}")))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use std::os::unix::ffi::OsStringExt;
    fn transfer(source: PathBuf, destination: PathBuf, moving: bool) -> Operation {
        Operation::Transfer { source, destination, moving, approval: None }
    }
    fn run(operation: Operation) -> Outcome { execute(operation, &Control::default()) }
    fn complete(outcome: Outcome) { assert!(matches!(outcome, Outcome::Complete(_)), "{outcome:?}"); }
    #[test]
    fn normal_desktop_exit_waits_for_live_and_dropped_operation_cleanup_and_replacement_preservation() {
        const FIXTURE: &str = "DESKTOP_TUI_OPERATION_SHUTDOWN_FIXTURE";
        if let Some(root) = std::env::var_os(FIXTURE) {
            let root = PathBuf::from(root);
            for dropped in [false,true] {
                let folder = root.join(if dropped { "dropped" } else { "live" }); std::fs::create_dir(&folder).unwrap();
                let source = folder.join("source"); let destination = folder.join("destination");
                std::fs::File::create(&source).unwrap().set_len(64*1024*1024).unwrap(); std::fs::write(&destination,b"original destination").unwrap();
                let Outcome::Confirm(operation) = run(transfer(source.clone(),destination.clone(),false)) else { panic!("replacement must be confirmed"); };
                let mut worker = OperationWorker::new().unwrap(); worker.submit(operation).unwrap();
                let deadline = std::time::Instant::now()+std::time::Duration::from_secs(3);
                while worker.progress().1 < 128*1024 { assert!(std::time::Instant::now()<deadline); std::thread::yield_now(); }
                worker.cancel(); let mut live = if dropped { drop(worker); None } else { Some(worker) };
                crate::runtime::files::finish_shutdown();
                if let Some(worker) = &mut live {
                    assert!(matches!(worker.poll(),Some(Outcome::Failed(error)) if error.contains("canceled")),"shutdown must join the canceled live owner's mutation");
                    assert!(worker.submit(Operation::Create { path: folder.join("after-shutdown"),directory: false }).is_err(),"closed worker must reject new mutations");
                }
                assert_eq!(std::fs::metadata(&source).unwrap().len(),64*1024*1024);
                assert_eq!(std::fs::read(&destination).unwrap(),b"original destination");
                let mut names = std::fs::read_dir(&folder).unwrap().map(|entry| entry.unwrap().file_name()).collect::<Vec<_>>(); names.sort();
                assert_eq!(names,vec![std::ffi::OsString::from("destination"),std::ffi::OsString::from("source")],"normal exit must finish staging cleanup/replacement recovery");
            }
            // Force publication to fail after retaining the previous destination.
            // Copying keeps its output fd open; removing write on the private
            // stage prevents rename-out but does not interrupt those writes.
            let folder = root.join("rollback"); std::fs::create_dir(&folder).unwrap();
            let source = folder.join("source"); let destination = folder.join("destination");
            std::fs::File::create(&source).unwrap().set_len(64*1024*1024).unwrap(); std::fs::write(&destination,b"rollback destination").unwrap();
            let Outcome::Confirm(operation) = run(transfer(source.clone(),destination.clone(),false)) else { panic!("replacement must be confirmed"); };
            let mut worker = OperationWorker::new().unwrap(); worker.submit(operation).unwrap();
            let deadline = std::time::Instant::now()+std::time::Duration::from_secs(5);
            while worker.progress().1 < 128*1024 { assert!(std::time::Instant::now()<deadline); std::thread::yield_now(); }
            let stage = std::fs::read_dir(&folder).unwrap().map(|entry| entry.unwrap().path()).find(|path| path.file_name().unwrap().as_bytes().starts_with(b".desktop-tui-stage-")).unwrap();
            std::fs::set_permissions(&stage,std::fs::Permissions::from_mode(0o500)).unwrap();
            loop {
                if let Some(outcome) = worker.poll() { assert!(matches!(outcome,Outcome::Failed(error) if error.contains("Cannot publish destination"))); break; }
                assert!(std::time::Instant::now()<deadline); std::thread::yield_now();
            }
            crate::runtime::files::finish_shutdown();
            assert!(!stage.exists()); assert_eq!(std::fs::read(destination).unwrap(),b"rollback destination");
            assert_eq!(std::fs::metadata(source).unwrap().len(),64*1024*1024);
            assert_eq!(std::fs::read_dir(folder).unwrap().count(),2,"rollback must leave no replacement backup or unpublished stage");
            return;
        }
        // Global shutdown and the deliberate in-flight copy run in a bounded child.
        let dir = TestDir::new(); std::fs::set_permissions(dir.path(),std::fs::Permissions::from_mode(0o777)).unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args(["--exact","runtime::files::operations::tests::normal_desktop_exit_waits_for_live_and_dropped_operation_cleanup_and_replacement_preservation"])
            .env(FIXTURE,dir.path()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        if unsafe { libc::geteuid() } == 0 { use std::os::unix::process::CommandExt; command.uid(65534).gid(65534); }
        let mut child = command.spawn().unwrap();
        let deadline = std::time::Instant::now()+std::time::Duration::from_secs(8); let mut timed_out = false;
        while child.try_wait().unwrap().is_none() {
            if std::time::Instant::now() >= deadline { child.kill().unwrap(); timed_out = true; break; }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let output = child.wait_with_output().unwrap(); assert!(!timed_out,"Operation shutdown failed to finish; isolated child was killed safely");
        assert!(output.status.success(),"{}{}",String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr));
    }
    #[test]
    fn canceled_unpublished_copy_removes_nested_read_only_staging_without_changing_symlink_targets() {
        use std::os::unix::process::CommandExt;
        const FIXTURE: &str = "DESKTOP_TUI_READONLY_STAGE_FIXTURE";
        if let Some(root) = std::env::var_os(FIXTURE) {
            let root = PathBuf::from(root); let source = root.join("source");
            for cancel in [true,false] {
                let control = Control::default(); let expected = manifest(&source,&control).unwrap();
                let stage = Stage::new(&root).unwrap(); let stage_path = stage.path.clone(); let staged = stage.path.join("entry");
                copy_tree(&source,&staged,&expected,&control).unwrap();
                if cancel { control.cancel.store(true,Ordering::Release); }
                else { std::fs::write(root.join("destination"),b"concurrent destination").unwrap(); }
                let error = publish(&staged,&root.join("destination"),None,&control).unwrap_err();
                let _ = stage.cleanup_error(error); drop(stage);
                assert!(!stage_path.exists(),"cancellation/publication failure must remove the private staging tree");
                assert_eq!(std::fs::read(source.join("nested/deeper/file")).unwrap(),b"keep");
                assert_eq!(std::fs::metadata(root.join("outside")).unwrap().mode() & 0o777,0o555,"cleanup must not chmod through a symlink");
                if cancel { assert!(!root.join("destination").exists()); }
                else { assert_eq!(std::fs::read(root.join("destination")).unwrap(),b"concurrent destination"); std::fs::remove_file(root.join("destination")).unwrap(); }
            }
            return;
        }
        let dir = TestDir::new(); std::fs::set_permissions(dir.path(),std::fs::Permissions::from_mode(0o777)).unwrap();
        std::fs::create_dir_all(dir.path().join("source/nested/deeper")).unwrap(); std::fs::create_dir(dir.path().join("outside")).unwrap();
        std::fs::write(dir.path().join("source/nested/deeper/file"),b"keep").unwrap();
        std::os::unix::fs::symlink(dir.path().join("outside"),dir.path().join("source/link")).unwrap();
        for path in ["source/nested/deeper","source/nested","outside"] { std::fs::set_permissions(dir.path().join(path),std::fs::Permissions::from_mode(0o555)).unwrap(); }
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child.args(["--exact","runtime::files::operations::tests::canceled_unpublished_copy_removes_nested_read_only_staging_without_changing_symlink_targets"]).env(FIXTURE,dir.path());
        if unsafe { libc::geteuid() } == 0 { child.uid(65534).gid(65534); }
        let output = child.output().unwrap();
        for path in ["source/nested/deeper","source/nested","outside"] { std::fs::set_permissions(dir.path().join(path),std::fs::Permissions::from_mode(0o755)).unwrap(); }
        assert!(output.status.success(),"{}{}",String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr));
    }
    #[test]
    fn cross_filesystem_move_stages_complete_content_and_unavailable_cross_device_trash_preserves_source() {
        let dir = TestDir::new(); let target_parent = std::env::current_dir().unwrap().join("target"); std::fs::create_dir_all(&target_parent).unwrap();
        let other = Stage::new(&target_parent).unwrap();
        if std::fs::metadata(dir.path()).unwrap().dev() == std::fs::metadata(&other.path).unwrap().dev() { return; }
        let source = dir.path().join("from"); let destination = other.path.join("to");
        std::fs::create_dir(&source).unwrap(); std::fs::write(source.join("文 file"),b"cross-device content").unwrap();
        std::os::unix::fs::symlink("/unavailable",source.join("symlink")).unwrap();
        complete(run(transfer(source.clone(),destination.clone(),true)));
        assert!(!source.exists()); assert_eq!(std::fs::read(destination.join("文 file")).unwrap(),b"cross-device content");
        assert_eq!(std::fs::read_link(destination.join("symlink")).unwrap(),PathBuf::from("/unavailable"));
        let trash_source = dir.path().join("keep"); std::fs::write(&trash_source,b"never delete").unwrap();
        assert!(matches!(run(Operation::Trash { source: trash_source.clone(), root: Some(other.path.join("Trash")) }),Outcome::Failed(_)));
        assert_eq!(std::fs::read(trash_source).unwrap(),b"never delete");
    }
    #[test]
    fn copy_to_read_only_destination_and_fifo_fail_without_source_loss() {
        use std::os::unix::process::CommandExt;
        const CHILD: &str = "DESKTOP_TUI_FILE_MUTATION_PERMISSIONS";
        if let Some(root) = std::env::var_os(CHILD) {
            let root = PathBuf::from(root); let source = root.join("source");
            assert!(matches!(run(transfer(source.clone(),root.join("blocked/destination"),false)),Outcome::Failed(_)));
            assert_eq!(std::fs::read(source).unwrap(),b"preserve"); return;
        }
        let dir = TestDir::new(); std::fs::write(dir.path().join("source"),b"preserve").unwrap();
        let blocked = dir.path().join("blocked"); std::fs::create_dir(&blocked).unwrap(); std::fs::set_permissions(&blocked,std::fs::Permissions::from_mode(0o500)).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child.args(["--exact","runtime::files::operations::tests::copy_to_read_only_destination_and_fifo_fail_without_source_loss"]).env(CHILD,dir.path());
        if unsafe { libc::geteuid() } == 0 { child.uid(65534).gid(65534); }
        let output = child.output().unwrap(); std::fs::set_permissions(&blocked,std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(output.status.success(),"{}{}",String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr));
        let fifo = dir.path().join("fifo"); let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap(); assert_eq!(unsafe { libc::mkfifo(name.as_ptr(),0o600) },0);
        assert!(matches!(run(transfer(fifo.clone(),dir.path().join("fifo-copy"),false)),Outcome::Failed(_))); assert!(fifo.exists());
    }
    #[test]
    fn pinned_parent_and_recursive_reads_do_not_follow_replaced_directory_symlinks() {
        let dir = TestDir::new(); let parent = dir.path().join("parent"); let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir(&parent).unwrap(); std::fs::create_dir(&elsewhere).unwrap(); std::fs::write(parent.join("file"),b"original").unwrap(); std::fs::write(elsewhere.join("file"),b"different").unwrap();
        let pinned = PinnedEntry::new(&parent.join("file")).unwrap();
        std::fs::rename(&parent,dir.path().join("old-parent")).unwrap(); std::os::unix::fs::symlink(&elsewhere,&parent).unwrap();
        assert_eq!(std::fs::read(pinned.path()).unwrap(),b"original");
        assert!(pinned.unchanged().is_err());
        let source = dir.path().join("tree"); std::fs::create_dir(&source).unwrap(); std::fs::create_dir(source.join("child")).unwrap(); std::fs::write(source.join("child/file"),b"expected").unwrap();
        let expected = manifest(&source,&Control::default()).unwrap();
        std::fs::rename(source.join("child"),source.join("original-child")).unwrap(); std::os::unix::fs::symlink(&elsewhere,source.join("child")).unwrap();
        assert!(copy_tree(&source,&dir.path().join("copy"),&expected,&Control::default()).is_err());
        assert_eq!(std::fs::read(elsewhere.join("file")).unwrap(),b"different");
    }
    #[test]
    fn mutation_worker_rejects_queueing_and_returns_progress_without_blocking() {
        let dir = TestDir::new(); let mut worker = OperationWorker::new().unwrap();
        worker.submit(Operation::Create { path: dir.path().join("created"), directory: false }).unwrap();
        assert!(worker.busy()); assert!(worker.submit(Operation::Create { path: dir.path().join("rejected"), directory: false }).is_err());
        let deadline = std::time::Instant::now()+std::time::Duration::from_secs(3);
        loop { if let Some(outcome) = worker.poll() { complete(outcome); break; } assert!(std::time::Instant::now()<deadline); std::thread::yield_now(); }
        assert!(!worker.busy()); assert!(dir.path().join("created").exists()); assert!(!dir.path().join("rejected").exists());
    }
    #[test]
    fn copies_a_root_symlink_as_a_symlink_and_detects_changed_source_snapshots() {
        let dir = TestDir::new(); let source = dir.path().join("source"); let destination = dir.path().join("copy");
        std::os::unix::fs::symlink("/does/not/exist",&source).unwrap();
        complete(run(transfer(source.clone(),destination.clone(),false)));
        assert_eq!(std::fs::read_link(destination).unwrap(),PathBuf::from("/does/not/exist"));
        std::fs::remove_file(&source).unwrap(); std::fs::write(&source,b"first").unwrap();
        let expected = manifest(&source,&Control::default()).unwrap(); std::fs::write(&source,b"changed").unwrap();
        assert!(copy_tree(&source,&dir.path().join("changed-copy"),&expected,&Control::default()).is_err());
        assert_eq!(std::fs::read(source).unwrap(),b"changed");
    }
    #[test]
    fn same_filesystem_move_and_rename_preserve_inode_and_read_only_directory_contents() {
        let dir = TestDir::new(); let source = dir.path().join("from"); let destination = dir.path().join("to");
        std::fs::create_dir(&source).unwrap(); std::fs::write(source.join("file"),b"keep").unwrap();
        let inode = std::fs::symlink_metadata(&source).unwrap().ino();
        std::fs::set_permissions(&source,std::fs::Permissions::from_mode(0o500)).unwrap();
        complete(run(transfer(source.clone(),destination.clone(),true)));
        assert_eq!(std::fs::symlink_metadata(&destination).unwrap().ino(),inode);
        assert_eq!(std::fs::read(destination.join("file")).unwrap(),b"keep");
        std::fs::set_permissions(&destination,std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[test]
    fn publication_collision_and_cancel_leave_unpublished_stage_and_source_untouched() {
        let dir = TestDir::new(); let staged = dir.path().join("stage"); let destination = dir.path().join("to");
        std::fs::write(&staged,b"new").unwrap(); std::fs::write(&destination,b"raced").unwrap();
        assert!(publish(&staged,&destination,None,&Control::default()).is_err());
        assert_eq!(std::fs::read(&staged).unwrap(),b"new"); assert_eq!(std::fs::read(&destination).unwrap(),b"raced");
        std::fs::remove_file(&destination).unwrap(); let control = Control::default(); control.cancel.store(true,Ordering::Release);
        assert!(publish(&staged,&destination,None,&control).is_err()); assert!(staged.exists()); assert!(!destination.exists());
    }
    #[test]
    fn existing_destination_folder_is_rejected_without_merging_or_removing_entries() {
        let dir = TestDir::new(); let source = dir.path().join("from"); let destination = dir.path().join("to");
        std::fs::create_dir(&source).unwrap(); std::fs::create_dir(&destination).unwrap(); std::fs::write(destination.join("keep"),b"old").unwrap();
        assert!(matches!(run(transfer(source.clone(),destination.clone(),true)),Outcome::Failed(_)));
        assert!(source.exists()); assert_eq!(std::fs::read(destination.join("keep")).unwrap(),b"old");
    }
    #[test]
    fn creates_files_and_directories_without_replacing_existing_entries() {
        let dir = TestDir::new(); let path = dir.path().join("文 file");
        complete(run(Operation::Create { path: path.clone(), directory: false }));
        std::fs::write(&path,b"keep").unwrap();
        assert!(matches!(run(Operation::Create { path: path.clone(), directory: false }),Outcome::Failed(_)));
        assert_eq!(std::fs::read(path).unwrap(),b"keep");
        complete(run(Operation::Create { path: dir.path().join("folder"), directory: true }));
    }
    #[test]
    fn recursive_copy_preserves_opaque_names_and_symlinks_without_traversing_targets() {
        let dir = TestDir::new(); let source = dir.path().join("source"); std::fs::create_dir(&source).unwrap();
        let name = std::ffi::OsString::from_vec(b"opaque \xff".to_vec());
        std::fs::write(source.join(&name),b"content").unwrap();
        std::os::unix::fs::symlink("/does/not/exist",source.join("link")).unwrap();
        let destination = dir.path().join("copy 文"); complete(run(transfer(source.clone(),destination.clone(),false)));
        assert_eq!(std::fs::read(destination.join(name)).unwrap(),b"content");
        assert_eq!(std::fs::read_link(destination.join("link")).unwrap(),PathBuf::from("/does/not/exist"));
        assert!(source.exists());
    }
    #[test]
    fn collision_requires_confirmation_and_changes_after_confirmation_preserve_both_files() {
        let dir = TestDir::new(); let source = dir.path().join("from"); let destination = dir.path().join("to");
        std::fs::write(&source,b"new").unwrap(); std::fs::write(&destination,b"old").unwrap();
        let Outcome::Confirm(operation) = run(transfer(source.clone(),destination.clone(),true)) else { panic!("must ask"); };
        assert_eq!(std::fs::read(&destination).unwrap(),b"old");
        std::fs::write(&destination,b"changed").unwrap();
        assert!(matches!(run(operation),Outcome::Failed(_)));
        assert_eq!(std::fs::read(source).unwrap(),b"new"); assert_eq!(std::fs::read(destination).unwrap(),b"changed");
    }
    #[test]
    fn confirmed_replacement_and_move_publish_complete_content() {
        let dir = TestDir::new(); let source = dir.path().join("from"); let destination = dir.path().join("to");
        std::fs::write(&source,b"new").unwrap(); std::fs::write(&destination,b"old").unwrap();
        let Outcome::Confirm(operation) = run(transfer(source.clone(),destination.clone(),true)) else { panic!("must ask"); };
        complete(run(operation)); assert!(!source.exists()); assert_eq!(std::fs::read(destination).unwrap(),b"new");
    }
    #[test]
    fn canceled_transfer_leaves_source_and_existing_destination_untouched() {
        let dir = TestDir::new(); let source = dir.path().join("from"); let destination = dir.path().join("to");
        std::fs::write(&source,b"new").unwrap(); std::fs::write(&destination,b"old").unwrap();
        let control = Control::default(); control.cancel.store(true,Ordering::Release);
        assert!(matches!(execute(transfer(source.clone(),destination.clone(),true),&control),Outcome::Failed(_)));
        assert_eq!(std::fs::read(source).unwrap(),b"new"); assert_eq!(std::fs::read(destination).unwrap(),b"old");
    }
    #[test]
    fn rejects_self_descendant_and_symlinked_descendant_destinations() {
        let dir = TestDir::new(); let source = dir.path().join("source"); std::fs::create_dir(&source).unwrap();
        std::os::unix::fs::symlink(&source,dir.path().join("alias")).unwrap();
        for destination in [source.clone(),source.join("child"),dir.path().join("alias/child")] {
            assert!(matches!(run(transfer(source.clone(),destination,true)),Outcome::Failed(_)));
        }
        assert!(source.exists());
    }
    #[test]
    fn trash_preserves_bytes_and_restoration_metadata() {
        let dir = TestDir::new(); let source = dir.path().join("space 文"); let root = dir.path().join("Trash");
        std::fs::write(&source,b"restore me").unwrap();
        complete(run(Operation::Trash { source: source.clone(), root: Some(root.clone()) }));
        assert!(!source.exists());
        let files = std::fs::read_dir(root.join("files")).unwrap().map(|e| e.unwrap().path()).collect::<Vec<_>>();
        assert_eq!(files.len(),1); assert_eq!(std::fs::read(&files[0]).unwrap(),b"restore me");
        let info = std::fs::read_dir(root.join("info")).unwrap().next().unwrap().unwrap().path();
        let text = std::fs::read_to_string(info).unwrap(); assert!(text.contains("[Trash Info]")); assert!(text.contains("%20")); assert!(text.contains("DeletionDate="));
    }
    #[test]
    fn unavailable_trash_and_special_file_copy_leave_source_intact() {
        let dir = TestDir::new(); let source = dir.path().join("from"); let root = dir.path().join("not-directory");
        std::fs::write(&source,b"keep").unwrap(); std::fs::write(&root,b"blocked").unwrap();
        assert!(matches!(run(Operation::Trash { source: source.clone(), root: Some(root) }),Outcome::Failed(_)));
        assert_eq!(std::fs::read(source).unwrap(),b"keep");
    }
}
