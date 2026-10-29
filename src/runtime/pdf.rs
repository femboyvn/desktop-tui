use super::images::{self, Frame, RenderRequest};
use super::latest::LatestWorker;
use anyhow::{Context, bail};
use image::RgbaImage;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock, Weak, atomic::{AtomicBool, Ordering}};
use std::time::{Duration, Instant};

const MAX_INPUT_BYTES: u64 = 64 * 1024 * 1024;
const RASTER_EDGE: u32 = 2048;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);
type Workers = Mutex<Vec<(Weak<AtomicBool>, std::thread::JoinHandle<()>)>>;
static WORKERS: OnceLock<Workers> = OnceLock::new();

pub(crate) struct Request { pub path: PathBuf, pub revision: u64, pub page: u32, pub render: RenderRequest }
pub(crate) struct PdfFrame { pub frame: Frame, pub page: u32, pub pages: u32 }

/// Owns the cancellation signal as well as the replaceable request queue.
pub(crate) struct PdfWorker {
    worker: LatestWorker<Request, Result<PdfFrame, String>>,
    cancelled: Arc<AtomicBool>,
}
impl PdfWorker {
    pub fn new() -> std::io::Result<Self> {
        let mut workers = WORKERS.get_or_init(|| Mutex::new(Vec::new())).lock().map_err(|_| std::io::Error::other("PDF worker registry unavailable"))?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancelled);
        let mut cached: Option<(u64, PathBuf, Result<Document, String>)> = None;
        let (worker, thread) = LatestWorker::new_tracked(move |request: Request| {
            if cached.as_ref().is_none_or(|(revision, path, _)| *revision != request.revision || *path != request.path) {
                cached = Some((request.revision, request.path.clone(), Document::open(&request.path, &stop).map_err(|error| format!("{error:#}"))));
            }
            let Some((_, _, document)) = cached.as_mut() else { return Err("PDF document unavailable".into()); };
            match document {
                Ok(document) => {
                    let pages = document.pages;
                    document.page(request.page, &stop).and_then(|page| images::render(page, request.render))
                        .map(|frame| PdfFrame { frame, page: request.page, pages }).map_err(|error| format!("{error:#}"))
                }
                Err(error) => Err(error.clone()),
            }
        })?;
        workers.retain(|(_, thread)| !thread.is_finished());
        workers.push((Arc::downgrade(&cancelled), thread));
        Ok(Self { worker, cancelled })
    }
    pub fn submit(&mut self, request: Request) { self.worker.submit(request); }
    pub fn poll(&mut self) -> Option<Result<PdfFrame, String>> { self.worker.poll() }
    pub fn close(&mut self) { self.cancelled.store(true, Ordering::Relaxed); self.worker.close(); }
}
impl Drop for PdfWorker { fn drop(&mut self) { self.close(); } }

/// Called after AppCUI drops its controls, outside all event callbacks.
pub(crate) fn finish_shutdown() {
    let workers = WORKERS.get().and_then(|workers| workers.lock().ok().map(|mut workers| std::mem::take(&mut *workers))).unwrap_or_default();
    for (cancelled, _) in &workers { if let Some(cancelled) = cancelled.upgrade() { cancelled.store(true, Ordering::Relaxed); } }
    for (_, worker) in workers { let _ = worker.join(); }
}

// Anonymous, bounded input avoids temporary document files and preserves opaque
// filenames. Children read the snapshot through stdin, never a shell argument.
fn snapshot(path: &Path) -> anyhow::Result<File> {
    let source = File::options().read(true).custom_flags(libc::O_NONBLOCK).open(path)
        .with_context(|| format!("Cannot read {}", path.display()))?;
    let metadata = source.metadata()?;
    if !metadata.is_file() { bail!("Choose a regular PDF file."); }
    if metadata.len() > MAX_INPUT_BYTES { bail!("PDF input exceeds the 64 MiB limit."); }
    // SAFETY: the name is a static C string; a successful call returns an owned fd.
    let fd = unsafe { libc::memfd_create(c"desktop-tui-pdf".as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 { return Err(std::io::Error::last_os_error()).context("Cannot allocate PDF snapshot"); }
    let mut input = unsafe { File::from_raw_fd(fd) };
    if std::io::copy(&mut source.take(MAX_INPUT_BYTES + 1), &mut input)? > MAX_INPUT_BYTES { bail!("PDF input exceeds the 64 MiB limit."); }
    input.seek(SeekFrom::Start(0))?;
    let mut header = [0;1024];
    let count = input.read(&mut header)?;
    if !header[..count].windows(5).any(|bytes| bytes == b"%PDF-") { bail!("This file does not have a PDF header."); }
    input.seek(SeekFrom::Start(0))?;
    Ok(input)
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        // Runs only on the worker thread, including error/cancellation paths.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn nonblocking(pipe: &impl AsRawFd) -> std::io::Result<()> {
    let fd = pipe.as_raw_fd();
    // SAFETY: fcntl borrows a valid owned pipe fd and does not retain it.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 { return Err(std::io::Error::last_os_error()); }
    Ok(())
}
fn drain(pipe: &mut impl Read, bytes: &mut Vec<u8>, limit: usize) -> anyhow::Result<()> {
    let mut chunk = [0;8192];
    loop {
        let capacity = chunk.len().min(limit.saturating_sub(bytes.len()) + 1);
        match pipe.read(&mut chunk[..capacity]) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                if count > limit.saturating_sub(bytes.len()) { bail!("PDF helper output exceeds its size limit."); }
                bytes.extend_from_slice(&chunk[..count]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        }
    }
}
fn run_command(mut command: Command, cancelled: &AtomicBool, timeout: Duration, limit: usize) -> anyhow::Result<Vec<u8>> {
    if cancelled.load(Ordering::Relaxed) { bail!("PDF loading cancelled."); }
    let mut child = ChildGuard(command.env("LC_ALL", "C").stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()
        .context("Cannot start PDF helper. Install Poppler (pdfinfo and pdftoppm) and make it available on PATH")?);
    let mut stdout = child.0.stdout.take().context("PDF helper stdout unavailable")?;
    let mut stderr = child.0.stderr.take().context("PDF helper stderr unavailable")?;
    nonblocking(&stdout)?;
    nonblocking(&stderr)?;
    let (mut output, mut errors) = (Vec::new(), Vec::new());
    let started = Instant::now();
    loop {
        if cancelled.load(Ordering::Relaxed) { bail!("PDF loading cancelled."); }
        if started.elapsed() >= timeout { bail!("PDF helper timed out. Try a smaller or simpler document."); }
        drain(&mut stdout, &mut output, limit)?;
        drain(&mut stderr, &mut errors, 16 * 1024)?;
        if let Some(status) = child.0.try_wait()? {
            drain(&mut stdout, &mut output, limit)?;
            drain(&mut stderr, &mut errors, 16 * 1024)?;
            if !status.success() {
                let detail: String = String::from_utf8_lossy(&errors).chars().take(1000).collect();
                bail!("Cannot read PDF (invalid, damaged, or password-protected document). {}", detail.trim());
            }
            return Ok(output);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn page_count(bytes: &[u8]) -> anyhow::Result<u32> {
    let text = std::str::from_utf8(bytes).context("PDF helper returned invalid metadata")?;
    text.lines().find_map(|line| line.strip_prefix("Pages:")).and_then(|count| count.trim().parse::<u32>().ok())
        .filter(|count| *count > 0).context("PDF has no readable pages")
}

struct Document { input: File, pages: u32, raster: Option<(u32, RgbaImage)> }
impl Document {
    fn open(path: &Path, cancelled: &AtomicBool) -> anyhow::Result<Self> {
        let input = snapshot(path)?;
        let mut command = Command::new("pdfinfo");
        command.arg("-").stdin(Stdio::from(input.try_clone()?));
        let pages = page_count(&run_command(command, cancelled, COMMAND_TIMEOUT, 64 * 1024)?)?;
        Ok(Self { input, pages, raster: None })
    }
    fn page(&mut self, page: u32, cancelled: &AtomicBool) -> anyhow::Result<&RgbaImage> {
        if page == 0 || page > self.pages { bail!("Page must be between 1 and {}.", self.pages); }
        if self.raster.as_ref().is_none_or(|(number, _)| *number != page) {
            self.input.seek(SeekFrom::Start(0))?;
            let mut command = Command::new("pdftoppm");
            command.args(["-q", "-singlefile", "-f", &page.to_string(), "-l", &page.to_string(), "-scale-to", &RASTER_EDGE.to_string(), "-"])
                .stdin(Stdio::from(self.input.try_clone()?));
            let bytes = run_command(command, cancelled, COMMAND_TIMEOUT, (RASTER_EDGE * RASTER_EDGE * 3) as usize + 4096)?;
            self.raster = Some((page, decode_ppm(&bytes)?));
        }
        self.raster.as_ref().map(|(_, image)| image).context("PDF page raster unavailable")
    }
}
fn decode_ppm(bytes: &[u8]) -> anyhow::Result<RgbaImage> {
    let mut offset = 0;
    let mut token = || -> Option<&[u8]> {
        loop {
            while bytes.get(offset).is_some_and(u8::is_ascii_whitespace) { offset += 1; }
            if bytes.get(offset) != Some(&b'#') { break; }
            while bytes.get(offset).is_some_and(|byte| *byte != b'\n') { offset += 1; }
        }
        let start = offset;
        while bytes.get(offset).is_some_and(|byte| !byte.is_ascii_whitespace()) { offset += 1; }
        (offset > start).then(|| &bytes[start..offset])
    };
    if token() != Some(b"P6") { bail!("PDF helper returned an invalid page image."); }
    let width = token().and_then(|value| std::str::from_utf8(value).ok()?.parse::<u32>().ok()).context("Invalid PDF raster width")?;
    let height = token().and_then(|value| std::str::from_utf8(value).ok()?.parse::<u32>().ok()).context("Invalid PDF raster height")?;
    if token() != Some(b"255") || width == 0 || height == 0 || width > RASTER_EDGE || height > RASTER_EDGE { bail!("PDF raster exceeds supported bounds."); }
    if !bytes.get(offset).is_some_and(u8::is_ascii_whitespace) { bail!("Invalid PDF raster separator."); }
    // Consume only the header separator: binary pixels may also be whitespace.
    if bytes.get(offset..offset+2) == Some(b"\r\n") { offset += 2; } else { offset += 1; }
    let pixels = &bytes[offset..];
    if pixels.len() != (width * height * 3) as usize { bail!("Truncated PDF page image."); }
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for pixel in pixels.chunks_exact(3) { rgba.extend_from_slice(&[pixel[0],pixel[1],pixel[2],255]); }
    RgbaImage::from_raw(width, height, rgba).context("Cannot allocate PDF page image")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;

    #[test]
    fn ppm_preserves_binary_whitespace_pixels_and_rejects_invalid_rasters() {
        let image = decode_ppm(b"P6\n# page\n1 1\n255\n\n #").unwrap();
        assert_eq!(image.dimensions(), (1,1));
        assert_eq!(image.get_pixel(0,0).0, [10,32,35,255]);
        for bytes in [b"P6\n0 1\n255\n".as_slice(), b"P6\n2049 1\n255\n", b"P6\n1 1\n255\nxx", b"P3\n1 1\n255\n0 0 0", b"P6\n1 1\n65535\nxxxxxx"] {
            assert!(decode_ppm(bytes).is_err());
        }
    }
    #[test]
    fn page_count_requires_a_positive_pages_field() {
        assert_eq!(page_count(b"Title: a PDF\nPages:           2\nPage size: 200 x 300 pts\n").unwrap(), 2);
        for bytes in [b"Pages: 0".as_slice(), b"Pages: -1", b"Pages: unknown", b"Title: Pages: 3"] {
            assert!(page_count(bytes).is_err());
        }
    }
    #[test]
    fn snapshot_rejects_missing_special_invalid_and_oversized_inputs() {
        let dir = TestDir::new();
        assert!(snapshot(&dir.path().join("missing")).is_err());
        assert!(snapshot(dir.path()).is_err());
        let path = dir.path().join("a.pdf");
        std::fs::write(&path, b"not a pdf").unwrap();
        assert!(snapshot(&path).is_err());
        std::fs::File::create(&path).unwrap().set_len(64 * 1024 * 1024 + 1).unwrap();
        assert!(snapshot(&path).is_err());
        let fifo = dir.path().join("fifo.pdf");
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(snapshot(&fifo).is_err());
    }
    #[test]
    fn helper_timeout_and_output_limits_reap_the_child() {
        let cancelled = AtomicBool::new(false);
        let mut command = Command::new("sh");
        command.args(["-c", "while :; do :; done"]);
        assert!(run_command(command, &cancelled, Duration::from_millis(30), 100).unwrap_err().to_string().contains("timed out"));
        let mut command = Command::new("sh");
        command.args(["-c", "printf 123456789"]);
        assert!(run_command(command, &cancelled, Duration::from_secs(2), 4).is_err());
        cancelled.store(true, Ordering::Relaxed);
        assert!(run_command(Command::new("sh"), &cancelled, Duration::from_secs(2), 100).is_err());
        let error = run_command(Command::new("desktop-tui-missing-pdf-helper"), &AtomicBool::new(false), Duration::from_secs(2), 100).unwrap_err();
        assert!(format!("{error:#}").contains("Poppler"));
    }
    #[test]
    fn cancellation_during_loading_reaps_the_active_helper() {
        let dir = TestDir::new();
        let marker = dir.path().join("pid");
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancelled);
        let path = marker.clone();
        let worker = std::thread::spawn(move || {
            let mut command = Command::new("sh");
            command.args(["-c", "printf '%s' \"$$\" > \"$1\"; while :; do :; done", "pdf-test"]).arg(path);
            run_command(command, &stop, Duration::from_secs(3), 100)
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        let pid = loop {
            if let Ok(text) = std::fs::read_to_string(&marker) && let Ok(pid) = text.parse::<i32>() { break pid; }
            assert!(Instant::now() < deadline, "helper did not start");
            std::thread::sleep(Duration::from_millis(2));
        };
        cancelled.store(true, Ordering::Relaxed);
        assert!(worker.join().unwrap().unwrap_err().to_string().contains("cancelled"));
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    }
    #[test]
    fn unavailable_window_timer_does_not_orphan_a_pdf_worker() {
        use appcui::prelude::*;
        let _lock = crate::test_support::UI_LOCK.lock().unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)").timers_count(1).build().unwrap();
        let mut holder = Window::new("Timer holder", layout!("x:0,y:0,w:10,h:5"), window::Flags::None);
        holder.timer().unwrap();
        let before: Vec<_> = WORKERS.get_or_init(|| Mutex::new(Vec::new())).lock().unwrap().iter().map(|(weak,_)| weak.clone()).collect();
        assert!(crate::ui::native::PdfWindow::new(None, crate::config::preferences::Rgb(0,0,0)).is_err());
        let added: Vec<_> = WORKERS.get().unwrap().lock().unwrap().iter().map(|(weak,_)| weak.clone())
            .filter(|weak| !before.iter().any(|previous| Weak::ptr_eq(previous,weak))).collect();
        let deadline = Instant::now() + Duration::from_secs(1);
        while added.iter().any(|weak| weak.strong_count() > 0) && Instant::now() < deadline { std::thread::yield_now(); }
        let orphaned = added.iter().any(|weak| weak.strong_count() > 0);
        app.run();
        assert!(!orphaned, "failed window construction retained an unreachable PDF worker");
    }
    #[test]
    #[ignore = "requires Poppler pdfinfo and pdftoppm on PATH"]
    fn poppler_renders_both_pages_and_rejects_out_of_range_pages() {
        let dir = TestDir::new();
        let path = dir.path().join("- document 文.pdf");
        std::fs::write(&path, crate::test_support::two_page_pdf()).unwrap();
        let cancelled = AtomicBool::new(false);
        let mut document = Document::open(&path, &cancelled).unwrap();
        assert_eq!(document.pages, 2);
        std::fs::write(&path, b"document replaced after open").unwrap();
        assert_eq!(document.page(1, &cancelled).unwrap().get_pixel(100,100).0, [255,0,0,255]);
        assert_eq!(document.page(2, &cancelled).unwrap().get_pixel(100,100).0, [0,0,255,255]);
        assert!(document.page(3, &cancelled).is_err());
    }
}
