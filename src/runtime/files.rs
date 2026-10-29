use anyhow::Context;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileKind { Directory, File, Other }
#[derive(Clone)]
pub(crate) struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub kind: FileKind,
    pub size: String,
    pub modified: String,
}
pub(crate) struct Listing { pub path: PathBuf, pub entries: Vec<Entry> }

pub(crate) fn list_directory(path: &Path, hidden: bool) -> anyhow::Result<Listing> {
    let path = path.canonicalize().with_context(|| format!("Cannot access {}", path.display()))?;
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&path).with_context(|| format!("Cannot list {}", path.display()))? {
        let entry = entry.with_context(|| format!("Cannot read an entry in {}", path.display()))?;
        use std::os::unix::ffi::OsStrExt;
        if !hidden && entry.file_name().as_bytes().starts_with(b".") { continue; }
        let metadata = std::fs::metadata(entry.path());
        let kind = match &metadata {
            Ok(metadata) if metadata.is_dir() => FileKind::Directory,
            Ok(metadata) if metadata.is_file() => FileKind::File,
            _ => FileKind::Other,
        };
        let size = metadata.as_ref().ok().filter(|m| m.is_file()).map(|m| m.len().to_string()).unwrap_or_else(|| "—".into());
        let modified = metadata.ok().and_then(|m| m.modified().ok()).map(|time| {
            chrono::DateTime::<chrono::Local>::from(time).format("%Y-%m-%d %H:%M").to_string()
        }).unwrap_or_else(|| "—".into());
        entries.push(Entry { path: entry.path(), name: entry.file_name().to_string_lossy().into_owned(), kind, size, modified });
    }
    entries.sort_by(|a, b| (a.kind != FileKind::Directory).cmp(&(b.kind != FileKind::Directory)).then_with(|| a.path.file_name().cmp(&b.path.file_name())));
    Ok(Listing { path, entries })
}
pub(crate) fn is_image(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ["png", "jpg", "jpeg", "webp", "gif", "bmp"].iter().any(|candidate| ext.eq_ignore_ascii_case(candidate)))
}
pub(crate) fn is_pdf(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use std::os::unix::ffi::OsStringExt;
    #[test]
    fn inaccessible_directory_returns_an_actionable_error() {
        use std::os::unix::fs::PermissionsExt;
        use std::os::unix::process::CommandExt;
        const FIXTURE: &str = "DESKTOP_TUI_INACCESSIBLE_DIRECTORY";
        if let Some(path) = std::env::var_os(FIXTURE) {
            let error = list_directory(Path::new(&path), false).err().expect("unreadable folder must fail");
            assert!(format!("{error:#}").contains("Permission denied"));
            return;
        }
        let dir = TestDir::new();
        let blocked = dir.path().join("blocked");
        std::fs::create_dir(&blocked).unwrap();
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o0)).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child.args(["--exact", "runtime::files::tests::inaccessible_directory_returns_an_actionable_error"]).env(FIXTURE, &blocked);
        if unsafe { libc::geteuid() } == 0 { child.uid(65534).gid(65534); }
        let output = child.output().unwrap();
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
    }
    #[test]
    fn lists_directories_first_and_preserves_opaque_paths_and_hidden_files() {
        let dir = TestDir::new();
        std::fs::create_dir(dir.path().join("z directory")).unwrap();
        for name in [std::ffi::OsString::from("a file 文.txt"), std::ffi::OsString::from("<FILE_PATH>.txt"), std::ffi::OsString::from_vec(vec![b'b', 0xff])] {
            std::fs::write(dir.path().join(name), b"hello").unwrap();
        }
        std::fs::write(dir.path().join(".hidden"), b"secret").unwrap();
        let listing = list_directory(dir.path(), false).unwrap();
        assert_eq!(listing.entries.len(), 4);
        assert_eq!(listing.entries[0].kind, FileKind::Directory);
        assert!(listing.entries.iter().any(|entry| entry.path.file_name().unwrap() == std::ffi::OsString::from_vec(vec![b'b', 0xff])));
        assert_eq!(list_directory(dir.path(), true).unwrap().entries.len(), 5);
        assert!(list_directory(&dir.path().join("missing"), false).is_err());
        assert!(list_directory(&dir.path().join("a file 文.txt"), false).is_err());
    }
    #[test]
    fn detects_supported_images_case_insensitively() {
        for name in ["a.PNG", "a.jpeg", "a.jpg", "a.webp", "a.gif", "a.bmp"] { assert!(is_image(Path::new(name))); }
        for name in ["a.svg", "a.png.txt", "no extension"] { assert!(!is_image(Path::new(name))); }
    }
    #[test]
    fn detects_pdf_documents_without_matching_other_extensions() {
        for name in ["document.pdf", "- 文.PDF", "mixed.PdF"] { assert!(is_pdf(Path::new(name))); }
        for name in ["pdf", "document.pdf.txt", "document.png"] { assert!(!is_pdf(Path::new(name))); }
    }
}
