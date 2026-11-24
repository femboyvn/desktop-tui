use super::{Control, Fingerprint, Outcome, PinnedEntry, directory_path, fingerprint, open_directory, rename_new};
use crate::runtime::files::{Entry, entry_details, path_caption};
use anyhow::{Context, ensure};
use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

const INFO_LIMIT: u64 = 32 * 1024;
const LIST_LIMIT: usize = 5_000;

pub(super) fn trash_root(root: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    if let Some(root) = root { return Ok(root); }
    Ok(directories::BaseDirs::new().context("Cannot find user trash directory")?.data_local_dir().join("Trash"))
}

pub(super) struct TrashDirectories {
    pub(super) files: File,
    pub(super) info: File,
}

fn private_directory(path: &Path, create: bool) -> anyhow::Result<File> {
    if create {
        match std::fs::DirBuilder::new().recursive(true).mode(0o700).create(path) {
            Ok(()) => {},
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {},
            Err(error) => return Err(error).context("Cannot create user Trash directory"),
        }
    }
    let directory = open_directory(path)?;
    let metadata = directory.metadata()?;
    ensure!(metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o077 == 0,
        "User Trash must be a private directory owned by this user");
    Ok(directory)
}

impl TrashDirectories {
    pub(super) fn open(root: &Path, create: bool) -> anyhow::Result<Self> {
        let root = private_directory(root,create)?;
        let path = directory_path(&root);
        Ok(Self { files: private_directory(&path.join("files"),create)?, info: private_directory(&path.join("info"),create)? })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TrashItem {
    root: PathBuf,
    info_name: OsString,
    original: PathBuf,
    info_identity: Fingerprint,
    payload_identity: Fingerprint,
}
impl TrashItem {
    pub fn original_path(&self) -> &Path { &self.original }
}

fn original_path(encoded: &str) -> anyhow::Result<PathBuf> {
    let mut bytes = Vec::new();
    let mut input = encoded.as_bytes().iter().copied();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let digit = |value: Option<u8>| value.and_then(|byte| char::from(byte).to_digit(16)).map(|digit| digit as u8).context("Invalid path escape");
            bytes.push(digit(input.next())? * 16 + digit(input.next())?);
        } else { bytes.push(byte); }
    }
    ensure!(!bytes.contains(&0),"Trash metadata contains a NUL byte");
    ensure!(bytes.len() <= 4_096,"Original path exceeds the Trash path limit");
    let path = PathBuf::from(OsString::from_vec(bytes));
    ensure!(path.is_absolute() && path.file_name().is_some() &&
        path.components().all(|part| matches!(part,Component::RootDir | Component::Normal(_))),
        "Trash metadata must give an absolute original path");
    Ok(path.components().collect())
}

fn read_item(root: &Path, directories: &TrashDirectories, info_name: &OsStr) -> anyhow::Result<(Entry,TrashItem)> {
    let info_name_path = Path::new(info_name);
    ensure!(info_name_path.components().count() == 1 && info_name_path.extension() == Some(OsStr::new("trashinfo")),"Invalid Trash metadata name");
    let name = info_name_path.file_stem().context("Missing Trash item name")?;
    ensure!(matches!(Path::new(name).components().next(),Some(Component::Normal(_))),"Invalid Trash item name");
    let info = directory_path(&directories.info).join(info_name);
    let file = OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(&info)?;
    let metadata = file.metadata()?;
    ensure!(metadata.is_file() && metadata.uid() == unsafe { libc::geteuid() } && metadata.len() <= INFO_LIMIT,"Invalid Trash metadata file");
    let info_identity = Fingerprint::of(&metadata);
    let mut text = String::new();
    file.take(INFO_LIMIT+1).read_to_string(&mut text)?;
    ensure!(text.len() as u64 <= INFO_LIMIT && fingerprint(&info)? == info_identity,"Trash metadata changed");
    let mut lines = text.lines();
    ensure!(lines.next() == Some("[Trash Info]"),"Invalid Trash metadata header");
    let mut paths = lines.clone().filter_map(|line| line.strip_prefix("Path="));
    let original = original_path(paths.next().context("Missing original path")?)?;
    ensure!(paths.next().is_none(),"Duplicate original path");
    let deleted = lines.find_map(|line| line.strip_prefix("DeletionDate="))
        .filter(|date| date.len() == 19 && chrono::NaiveDateTime::parse_from_str(date,"%Y-%m-%dT%H:%M:%S").is_ok())
        .unwrap_or("—");
    let payload = directory_path(&directories.files).join(name);
    let payload_metadata = std::fs::symlink_metadata(payload)?;
    let payload_identity = Fingerprint::of(&payload_metadata);
    let mut entry = entry_details(root.join("files").join(name),path_caption(&original),Some(&payload_metadata));
    entry.modified = deleted.into();
    Ok((entry,TrashItem { root: root.to_owned(),info_name: info_name.to_owned(),original,info_identity,payload_identity }))
}

/// List valid pairs in the per-user Trash. Invalid metadata and missing payloads
/// stay on disk and are counted in the listing status.
pub(crate) fn list_trash(root: Option<PathBuf>, canceled: &AtomicBool) -> anyhow::Result<(Vec<(Entry,TrashItem)>,String)> {
    ensure!(!canceled.load(Ordering::Relaxed),"Trash listing canceled");
    let root = trash_root(root)?;
    match std::fs::symlink_metadata(&root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(),"Trash is empty".into())),
        result => { result?; }
    }
    let directories = TrashDirectories::open(&root,false)?;
    let mut skipped = 0;
    let mut limited = false;
    let mut items = Vec::new();
    for (index, entry) in std::fs::read_dir(directory_path(&directories.info))?.enumerate() {
        ensure!(!canceled.load(Ordering::Relaxed),"Trash listing canceled");
        if index == LIST_LIMIT { limited = true; break; }
        let Ok(entry) = entry else { skipped += 1; continue; };
        if entry.path().extension() != Some(OsStr::new("trashinfo")) { continue; }
        match read_item(&root,&directories,&entry.file_name()) {
            Ok(item) => items.push(item),
            Err(_) => skipped += 1,
        }
    }
    items.sort_by(|(a,_),(b,_)| b.modified.cmp(&a.modified).then_with(|| a.name.cmp(&b.name)).then_with(|| a.path.cmp(&b.path)));
    let mut status = format!("{} items in Trash",items.len());
    if limited { status.push_str(" | Listing limit reached"); }
    if skipped > 0 { status.push_str(&format!(" | {skipped} invalid or unavailable")); }
    Ok((items,status))
}

pub(super) fn restore(item: TrashItem, control: &Control) -> anyhow::Result<Outcome> {
    let directories = TrashDirectories::open(&item.root,false)?;
    let (entry,current) = read_item(&item.root,&directories,&item.info_name).context("Trash item is no longer available")?;
    ensure!(current.info_identity == item.info_identity && current.payload_identity == item.payload_identity && current.original == item.original,
        "Trash item changed; refresh Trash and select it again");
    let destination = PinnedEntry::new(&item.original)
        .with_context(|| format!("Cannot restore to {}. Check that the original folder exists",item.original.display()))?;
    ensure!(destination.original == item.original && !destination.original.starts_with(item.root.canonicalize()?),
        "The original location has changed or points inside Trash");
    let name = entry.path.file_name().context("Invalid Trash item name")?;
    let source = directory_path(&directories.files).join(name);
    let info = directory_path(&directories.info).join(&item.info_name);
    control.check()?;
    destination.unchanged()?;
    ensure!(fingerprint(&source)? == item.payload_identity && fingerprint(&info)? == item.info_identity,"Trash item changed before restore");
    rename_new(&source,destination.path()).with_context(|| format!("Cannot restore to {}. The destination must be vacant and on the same filesystem",item.original.display()))?;
    if !fingerprint(destination.path()).is_ok_and(|actual| item.payload_identity.after_rename_matches(&actual)) {
        rename_new(destination.path(),&source).with_context(|| format!("Trash payload changed during restore; inspect {}",item.original.display()))?;
        anyhow::bail!("Trash payload changed during restore; refresh Trash and select it again");
    }
    let message = format!("Restored to {}",item.original.display());
    if fingerprint(&info).is_ok_and(|actual| actual == item.info_identity) {
        if let Err(error) = std::fs::remove_file(&info) {
            return Ok(Outcome::Warning(format!("{message}. Could not remove Trash metadata: {error}")));
        }
    } else {
        return Ok(Outcome::Warning(format!("{message}. Trash metadata changed and was left in place")));
    }
    Ok(Outcome::Complete(message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use super::super::{Operation, execute};

    #[test]
    fn lists_and_restores_exact_non_utf8_paths_and_preserves_conflicting_files() {
        let dir = TestDir::new();
        let root = dir.path().join("Trash");
        let source = dir.path().join(OsString::from_vec(b"work %\xff".to_vec()));
        std::fs::write(&source,b"trashed").unwrap();
        assert!(matches!(execute(Operation::Trash { source: source.clone(),root: Some(root.clone()),expected_source: None },&Control::default()),Outcome::Complete(_)));
        let canceled = AtomicBool::new(false);
        let (mut items,_) = list_trash(Some(root.clone()),&canceled).unwrap();
        let (_,item) = items.pop().unwrap();
        assert_eq!(item.original,source);
        std::fs::write(&source,b"existing").unwrap();
        assert!(restore(item.clone(),&Control::default()).is_err());
        assert_eq!(std::fs::read(&source).unwrap(),b"existing");
        std::fs::remove_file(&source).unwrap();
        assert!(matches!(restore(item,&Control::default()).unwrap(),Outcome::Complete(_)));
        assert_eq!(std::fs::read(&source).unwrap(),b"trashed");
        assert!(list_trash(Some(root),&canceled).unwrap().0.is_empty());
    }

    #[test]
    fn refuses_stale_items_and_skips_malformed_or_unpaired_metadata() {
        let dir = TestDir::new();
        let root = dir.path().join("Trash");
        let source = dir.path().join("original");
        std::fs::create_dir(&source).unwrap();
        assert!(matches!(execute(Operation::Trash { source: source.clone(),root: Some(root.clone()),expected_source: None },&Control::default()),Outcome::Complete(_)));
        let canceled = AtomicBool::new(false);
        let (mut items,_) = list_trash(Some(root.clone()),&canceled).unwrap();
        let (entry,item) = items.pop().unwrap();
        std::fs::write(entry.path.join("changed"),b"new file").unwrap();
        assert!(restore(item,&Control::default()).is_err());
        assert!(!source.exists());
        std::fs::write(root.join("info/bad.trashinfo"),"[Trash Info]\nPath=/tmp/%2E%2E/unsafe\n").unwrap();
        std::fs::write(root.join("info/missing.trashinfo"),"[Trash Info]\nPath=/some/path\n").unwrap();
        let (items,status) = list_trash(Some(root),&canceled).unwrap();
        assert_eq!(items.len(),1);
        assert!(status.contains("2 invalid"));
    }
}
