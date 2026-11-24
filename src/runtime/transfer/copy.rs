use super::*;
use std::collections::{HashMap, HashSet};
use std::fs::{self, Metadata};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::sync::atomic::AtomicU64;

const MAX_ENTRIES:usize = 100_000;
const MAX_DEPTH:usize = 128;

pub(super) fn checked_listing(mut items:Vec<Item>) -> Result<Vec<Item>> {
    ensure!(items.len() <= MAX_ENTRIES,"Directory exceeds 100,000 entries");
    let mut names = HashSet::new();
    for item in &items {
        validate_name(&item.name)?;
        ensure!(names.insert(&item.name),"Ambiguous duplicate name in directory");
    }
    items.sort_unstable_by(|a,b| (a.kind != Kind::Directory).cmp(&(b.kind != Kind::Directory)).then_with(|| a.name.cmp(&b.name)));
    Ok(items)
}

fn kind(meta:&Metadata) -> Kind {
    if meta.is_dir() { Kind::Directory } else if meta.is_file() { Kind::File } else { Kind::Other }
}
pub(super) fn local_list(path:&Path,ctl:&Control) -> Result<Vec<Item>> {
    let mut items = Vec::new();
    for entry in fs::read_dir(path)? {
        ctl.check()?;
        ensure!(items.len() < MAX_ENTRIES,"Directory exceeds 100,000 entries");
        let entry = entry?;
        let meta = fs::symlink_metadata(entry.path())?;
        items.push(Item {name:entry.file_name(),kind:kind(&meta),size:meta.is_file().then_some(meta.len())});
    }
    Ok(items)
}
fn open_dir(path:&Path) -> Result<File> {
    Ok(File::options().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY).open(path)?)
}
fn pinned(dir:&File) -> PathBuf { PathBuf::from(format!("/proc/self/fd/{}",dir.as_raw_fd())) }
fn metadata(path:&Path) -> Result<Option<Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta)), Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None), Err(e) => Err(e.into()),
    }
}
fn identity(meta:&Metadata) -> (u64,u64,i64,i64) { (meta.dev(),meta.ino(),meta.ctime(),meta.ctime_nsec()) }
struct Batch<'a> {
    remote:&'a mut dyn Remote, ctl:&'a Control, from:Side, visited:usize, files:u64, skipped:u64, ancestors:Vec<String>,
}
pub(super) fn batch(from:Side,names:Vec<OsString>,local:&Path,remote_path:&Path,remote:&mut dyn Remote,ctl:&Control) -> Result<String> {
    ensure!(!names.is_empty() && names.len() <= 1_000, "Select between 1 and 1,000 entries");
    let mut selected = HashSet::new();
    for name in names { validate_name(&name)?; ensure!(selected.insert(name),"Duplicate selected name"); }
    let mut job = Batch { remote,ctl,from,visited:0,files:0,skipped:0,ancestors:Vec::new() };
    if let Err(e) = job.directory(open_dir(local)?,remote_path,Some(&selected),0,None) {
        bail!("{e:#}. Completed {} file(s), skipped {}.",job.files,job.skipped);
    }
    Ok(format!("Copied {} file(s), skipped {}.",job.files,job.skipped))
}
impl Batch<'_> {
    fn directory(&mut self,dir:File,remote_dir:&Path,selected:Option<&HashSet<OsString>>,depth:usize,id:Option<&String>) -> Result<()> {
        self.ctl.check()?; ensure!(depth <= MAX_DEPTH, "Folder exceeds 128 levels");
        let here = pinned(&dir);
        let remote_items = checked_listing(self.remote.list(remote_dir,self.ctl)?)?;
        let ids = if self.from == Side::Remote { self.remote.directory_ids().cloned().unwrap_or_default() } else { HashMap::new() };
        let ancestor_count = self.ancestors.len();
        if let Some(current) = ids.get(OsStr::new(".")).or(id) {
            if self.ancestors.contains(current) { self.skipped += 1; return Ok(()); }
            self.ancestors.push(current.clone());
        }
        let (items,existing) = match self.from {
            Side::Local => (checked_listing(local_list(&here,self.ctl)?)?,remote_items),
            Side::Remote => (remote_items,Vec::new()),
        };
        let mut items = items;
        if let Some(selected) = selected {
            items.retain(|item| selected.contains(&item.name));
            ensure!(items.len() == selected.len(), "Selection changed. Refresh the source folder");
        }
        self.visited += items.len();
        ensure!(self.visited <= MAX_ENTRIES,"Selection exceeds 100,000 entries");
        let existing:HashMap<_,_> = existing.into_iter().map(|item| (item.name.clone(),item)).collect();
        for item in items {
            self.ctl.check()?;
            if item.kind == Kind::Other { self.skipped += 1; continue; }
            let local = here.join(&item.name);
            let remote = remote_dir.join(&item.name);
            let local_meta = metadata(&local)?;
            let dest = match self.from {
                Side::Remote => local_meta.as_ref().map(kind), Side::Local => existing.get(&item.name).map(|entry|entry.kind),
            };
            if let Some(dest) = dest { ensure!(dest == item.kind, "Conflicting types at {}",item.name.to_string_lossy()); }
            if item.kind == Kind::Directory {
                let child_id = ids.get(&item.name);
                if child_id.is_some_and(|id| self.ancestors.contains(id)) { self.skipped += 1; continue; }
                ensure!(depth < MAX_DEPTH,"Folder exceeds 128 levels");
                match (self.from,dest) {
                    (Side::Remote,None) => fs::create_dir(&local)?,
                    (Side::Local,None) => self.remote.mkdir(&remote,self.ctl)?,
                    _ => {}
                }
                self.directory(open_dir(&local)?,&remote,None,depth+1,child_id)?;
                continue;
            }
            if dest.is_some() && !matches!(self.ctl.ask(&format!("Replace existing file {}?",item.name.to_string_lossy()),PromptKind::Conflict)?,Answer::Replace) {
                self.skipped += 1; continue;
            }
            {
                let mut p = self.ctl.shared.progress.lock().unwrap(); p.file = item.name.to_string_lossy().into_owned(); p.dirty = true;
            }
            let expected = local_meta.as_ref().map(identity);
            match self.from {
                Side::Remote => {
                    self.download(&remote,&local,local_meta.as_ref(),item.size)?;
                }
                Side::Local => self.upload(&local,&remote,expected,existing.get(&item.name))?,
            }
            let mut p = self.ctl.shared.progress.lock().unwrap(); p.files = self.files; p.dirty = true;
        }
        self.ancestors.truncate(ancestor_count);
        Ok(())
    }
    fn download(&mut self,source:&Path,dest:&Path,original:Option<&Metadata>,size:Option<u64>) -> Result<()> {
        let parent = dest.parent().context("Missing download directory")?;
        let stage = tempfile::Builder::new().prefix(".desktop-transfer-")
            .permissions(fs::Permissions::from_mode(0o700)).tempdir_in(parent)?;
        let stage_path = stage.path().canonicalize()?;
        let result = (|| {
            let file = tempfile::NamedTempFile::new_in(stage.path())?;
            self.remote.download(source,file.path(),self.ctl)?;
            self.ctl.check()?;
            if let Some(size) = size { ensure!(file.as_file().metadata()?.len() == size,"Download length changed"); }
            file.as_file().sync_all()?;
            crate::runtime::files::publish_download(file.path(),dest,original,||self.ctl.check())?;
            self.files += 1;
            Ok::<_,anyhow::Error>(())
        })();
        if let Err(cleanup) = stage.close() {
            let outcome = result.err().map_or_else(||"Download published".into(),|e|format!("{e:#}"));
            bail!("{outcome}. Check local staging directory {}: {cleanup}",stage_path.display());
        }
        result
    }
    fn upload(&mut self,local:&Path,dest:&Path,source:Option<(u64,u64,i64,i64)>,expected:Option<&Item>) -> Result<()> {
        let parent = dest.parent().context("Missing remote folder")?;
        static NEXT:AtomicU64 = AtomicU64::new(0);
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
        let staging = parent.join(format!(".desktop-transfer-{}-{stamp:x}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
        ensure!(staging != dest, "Destination conflicts with staging name");
        self.remote.mkdir(&staging,self.ctl)?;
        let file = staging.join(dest.file_name().context("Missing destination filename")?);
        let result = (|| {
            self.remote.upload(local,&file,self.ctl)?;
            self.ctl.check()?;
            ensure!(metadata(local)?.as_ref().map(identity) == source,"Source changed during upload");
            let listing = checked_listing(self.remote.list(parent,self.ctl)?)?;
            ensure!(listing.iter().find(|item| Some(item.name.as_os_str()) == dest.file_name()) == expected,
                "Remote destination changed during upload");
            self.remote.publish(&file,dest,expected.is_some(),self.ctl)
        })();
        // Cancellation stops the payload, but cleanup gets its own bounded backend operations.
        let cleanup = self.ctl.cleanup();
        if result.is_ok() { self.files += 1; }
        if !matches!(result,Ok(Publication::Moved)) { let _ = self.remote.remove(&file,&cleanup); }
        if let Err(cleanup_error) = self.remote.rmdir(&staging,&cleanup) {
            let outcome = result.err().map_or_else(||"File published".into(),|e|format!("{e:#}"));
            bail!("{outcome}. Check staging folder {}: {cleanup_error:#}",staging.display());
        }
        result.map(|_| ())
    }
}
