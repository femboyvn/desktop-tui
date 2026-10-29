use crate::config::preferences::{AsciiBackground, MAX_ASCII_BACKGROUND_BYTES};
use anyhow::Context;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// Load a bounded copy of a regular text file. Call from a worker, not a UI callback.
pub(crate) fn load(path: &Path) -> anyhow::Result<AsciiBackground> {
    let file = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(path)
        .with_context(|| format!("Cannot read {}", path.display()))?;
    let metadata = file.metadata()?;
    anyhow::ensure!(metadata.is_file(), "Choose a regular ASCII text file.");
    anyhow::ensure!(metadata.len() <= MAX_ASCII_BACKGROUND_BYTES as u64, "ASCII background exceeds the 16 KiB limit.");
    let mut text = String::new();
    file.take(MAX_ASCII_BACKGROUND_BYTES as u64 + 1).read_to_string(&mut text)
        .with_context(|| format!("Cannot read ASCII text from {}", path.display()))?;
    AsciiBackground::try_from(text).map_err(anyhow::Error::msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use std::os::unix::ffi::OsStringExt;

    #[test]
    fn ascii_background_files_preserve_art_and_opaque_paths() {
        let dir = TestDir::new();
        for name in [std::ffi::OsString::from("cat art 文.ASC"), std::ffi::OsString::from_vec(b"cat\xff.txt".to_vec())] {
            let path = dir.path().join(name);
            std::fs::write(&path, b" /\\_/\\\r\n\t(o.o)\r\n").unwrap();
            assert_eq!(load(&path).unwrap().text(), " /\\_/\\\n    (o.o)\n");
        }
    }

    #[test]
    fn ascii_background_files_reject_invalid_missing_and_oversized_inputs() {
        let dir = TestDir::new();
        assert!(load(&dir.path().join("missing.asc")).is_err());
        assert!(load(dir.path()).is_err());
        let path = dir.path().join("bad.txt");
        for bytes in [b"\xff".to_vec(), b"\x1b[31mred".to_vec(), vec![b'x'; 16385], vec![b'\t'; 4097]] {
            std::fs::write(&path, bytes).unwrap();
            assert!(load(&path).is_err());
        }
        std::fs::write(&path, []).unwrap();
        assert!(load(&path).unwrap().is_empty());
    }

    #[test]
    fn ascii_background_fifo_is_rejected_without_waiting_for_a_writer() {
        use std::os::unix::ffi::OsStrExt;
        let dir = TestDir::new();
        let path = dir.path().join("pipe.asc");
        let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(load(&path).unwrap_err().to_string().contains("regular"));
    }
}
