use super::{Entry, Listing, entry_details, operations::{directory_path, open_directory}, path_caption};
use anyhow::{Context, ensure};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

const ENTRY_LIMIT: usize = 100_000;
const RESULT_LIMIT: usize = 5_000;
const DEPTH_LIMIT: usize = 128;

struct Search<'a> {
    root: &'a Path,
    query: String,
    hidden: bool,
    canceled: &'a AtomicBool,
    entries: Vec<Entry>,
    visited: usize,
    skipped: usize,
    limited: bool,
}

impl Search<'_> {
    fn walk(&mut self, directory: &std::fs::File, relative: &Path, depth: usize) -> anyhow::Result<()> {
        let items = std::fs::read_dir(directory_path(directory))
            .with_context(|| format!("Cannot read {}", self.root.join(relative).display()))?;
        for item in items {
            if self.canceled.load(Ordering::Relaxed) || self.limited { break; }
            if self.visited == ENTRY_LIMIT || self.entries.len() == RESULT_LIMIT {
                self.limited = true;
                break;
            }
            self.visited += 1;
            let Ok(item) = item else { self.skipped += 1; continue; };
            let name = item.file_name();
            if !self.hidden && name.as_bytes().starts_with(b".") { continue; }
            let child = relative.join(&name);
            let Ok(metadata) = std::fs::symlink_metadata(item.path()) else { self.skipped += 1; continue; };
            if name.to_string_lossy().to_lowercase().contains(&self.query) {
                self.entries.push(entry_details(self.root.join(&child),path_caption(&child),Some(&metadata)));
            }
            if metadata.is_dir() {
                if depth == DEPTH_LIMIT { self.skipped += 1; continue; }
                if open_directory(&item.path()).and_then(|directory| self.walk(&directory,&child,depth+1)).is_err() {
                    self.skipped += 1;
                }
            }
        }
        Ok(())
    }
}

/// Find basename matches below this folder. Nested symlinks are listed as links
/// when they match but are never traversed. Warnings describe partial results.
pub(crate) fn search_filenames(path: &Path, query: &str, hidden: bool, canceled: &AtomicBool) -> anyhow::Result<(Listing,String)> {
    ensure!(!query.trim().is_empty() && query.len() <= 512,"Enter a filename query of 1–512 bytes");
    let path = path.canonicalize().with_context(|| format!("Cannot access {}",path.display()))?;
    let directory = open_directory(&path)?;
    let mut search = Search { root: &path,query: query.to_lowercase(),hidden,canceled,
        entries: Vec::new(),visited: 0,skipped: 0,limited: false };
    search.walk(&directory,Path::new(""),0)?;
    ensure!(!canceled.load(Ordering::Relaxed),"Search canceled");
    search.entries.sort_by(|a,b| a.path.cmp(&b.path));
    let mut status = format!("{} filename matches",search.entries.len());
    if search.limited { status.push_str(" | Search limit reached"); }
    if search.skipped > 0 { status.push_str(&format!(" | {} unreadable or too deep",search.skipped)); }
    let entries = search.entries;
    Ok((Listing { path,entries },status))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use std::os::unix::ffi::OsStringExt;

    #[test]
    fn searches_nested_basenames_with_exact_paths_without_following_directory_links() {
        let dir = TestDir::new();
        let outside = TestDir::new();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::create_dir(dir.path().join(".hidden")).unwrap();
        let name = std::ffi::OsString::from_vec(b"READme\xff".to_vec());
        let target = dir.path().join("src").join(name);
        std::fs::write(&target,b"nested").unwrap();
        std::fs::write(dir.path().join(".hidden/readme"),b"hidden").unwrap();
        std::fs::write(outside.path().join("readme"),b"outside").unwrap();
        std::os::unix::fs::symlink(outside.path(),dir.path().join("linked")).unwrap();
        let canceled = AtomicBool::new(false);
        let (listing,_) = search_filenames(dir.path(),"readME",false,&canceled).unwrap();
        assert_eq!(listing.entries.iter().map(|entry| &entry.path).collect::<Vec<_>>(),vec![&target]);
        assert_eq!(search_filenames(dir.path(),"readme",true,&canceled).unwrap().0.entries.len(),2);
        canceled.store(true,Ordering::Relaxed);
        assert!(search_filenames(dir.path(),"readme",true,&canceled).is_err());
    }
}
