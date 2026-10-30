use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{atomic::{AtomicU64, Ordering}, mpsc};

pub(crate) const MAX_WIDTH: usize = 200;
pub(crate) const MAX_HEIGHT: usize = 80;
const MAX_PROJECT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Rgb(pub [u8; 3]);
impl Rgb {
    pub fn parse_hex(text: &str) -> Result<Self, String> {
        let text = text.strip_prefix('#').unwrap_or(text);
        if text.len() != 6 || !text.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err("Use six hexadecimal RGB digits, for example #FF80C0.".into());
        }
        let mut value = [0; 3];
        for (i, channel) in value.iter_mut().enumerate() {
            *channel = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).map_err(|e| e.to_string())?;
        }
        Ok(Self(value))
    }
    pub fn to_hex(self) -> String { format!("#{:02X}{:02X}{:02X}", self.0[0], self.0[1], self.0[2]) }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cell {
    pub ch: char,
    pub foreground: Rgb,
    pub background: Rgb,
}
#[derive(Clone, Debug)]
enum Action {
    Cells(Vec<(usize, Cell, Cell)>),
    Resize { before: Snapshot, after: Snapshot },
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot { width: usize, height: usize, cells: Vec<Cell> }
#[derive(Clone, Debug)]
struct Stroke { previous: (usize, usize), ink: Cell, original: Vec<Cell> }
#[derive(Clone, Debug)]
/// Bounded row-major canvas. Undo records one action per finished stroke.
/// Saved-state comparison uses dimensions and cells, including across a save.
pub(crate) struct Document {
    width: usize,
    height: usize,
    blank: Cell,
    cells: Vec<Cell>,
    saved: Option<Snapshot>,
    undo: VecDeque<Action>,
    redo: Vec<Action>,
    stroke: Option<Stroke>,
}
impl Default for Document {
    fn default() -> Self { Self::new(80, 24).expect("default canvas dimensions are valid") }
}
impl Document {
    pub fn new(width: usize, height: usize) -> Result<Self, String> {
        if !(1..=MAX_WIDTH).contains(&width) || !(1..=MAX_HEIGHT).contains(&height) {
            return Err("Canvas dimensions must be 1–200 columns and 1–80 rows.".into());
        }
        let blank = Cell { ch: ' ', foreground: Rgb([255, 255, 255]), background: Rgb([0, 0, 0]) };
        let cells = vec![blank; width * height];
        let saved = Some(Snapshot { width, height, cells: cells.clone() });
        Ok(Self { width, height, blank, saved, cells, undo: VecDeque::new(), redo: Vec::new(), stroke: None })
    }
    pub fn width(&self) -> usize { self.width }
    pub fn height(&self) -> usize { self.height }
    pub fn blank(&self) -> Cell { self.blank }
    pub fn cell(&self, x: usize, y: usize) -> Option<Cell> {
        if x < self.width && y < self.height { Some(self.cells[y * self.width + x]) } else { None }
    }
    pub fn is_dirty(&self) -> bool {
        self.saved.as_ref().is_none_or(|saved| saved.width != self.width || saved.height != self.height || saved.cells != self.cells)
    }
    fn snapshot(&self) -> Snapshot { Snapshot { width: self.width, height: self.height, cells: self.cells.clone() } }
    fn restore(&mut self, snapshot: &Snapshot) {
        self.width = snapshot.width; self.height = snapshot.height; self.cells.clone_from(&snapshot.cells);
    }
    pub fn mark_saved(&mut self) { self.finish_stroke(); self.saved = Some(self.snapshot()); }
    /// Resize in cells, preserving the top-left overlap and the original blank.
    /// Cropping is one reversible action; invalid dimensions leave all state intact.
    pub fn resize(&mut self, width: usize, height: usize) -> Result<bool, String> {
        if !(1..=MAX_WIDTH).contains(&width) || !(1..=MAX_HEIGHT).contains(&height) {
            return Err("Canvas dimensions must be 1–200 columns and 1–80 rows.".into());
        }
        self.finish_stroke();
        if (width, height) == (self.width, self.height) { return Ok(false); }
        let before = self.snapshot();
        let mut cells = vec![self.blank; width * height];
        let overlap = width.min(self.width);
        for y in 0..height.min(self.height) {
            cells[y * width..y * width + overlap].copy_from_slice(&self.cells[y * self.width..y * self.width + overlap]);
        }
        self.width = width; self.height = height; self.cells = cells;
        self.push_action(Action::Resize { before, after: self.snapshot() });
        Ok(true)
    }
    pub fn begin_stroke(&mut self, x: usize, y: usize, ink: Cell) -> bool {
        self.finish_stroke();
        if self.cell(x, y).is_none() || !(' '..='~').contains(&ink.ch) { return false; }
        self.stroke = Some(Stroke { previous: (x, y), ink, original: self.cells.clone() });
        self.cells[y * self.width + x] = ink;
        true
    }
    pub fn continue_stroke(&mut self, x: usize, y: usize) -> bool {
        if self.cell(x, y).is_none() { return false; }
        let Some(stroke) = self.stroke.as_mut() else { return false; };
        let (mut ax, mut ay) = (stroke.previous.0 as isize, stroke.previous.1 as isize);
        let (bx, by) = (x as isize, y as isize);
        let dx = (bx - ax).abs(); let dy = -(by - ay).abs();
        let sx = if ax < bx { 1 } else { -1 }; let sy = if ay < by { 1 } else { -1 };
        let mut error = dx + dy;
        loop {
            self.cells[ay as usize * self.width + ax as usize] = stroke.ink;
            if ax == bx && ay == by { break; }
            let doubled = 2 * error;
            if doubled >= dy { error += dy; ax += sx; }
            if doubled <= dx { error += dx; ay += sy; }
        }
        stroke.previous = (x, y);
        true
    }
    fn record(&mut self, original: Vec<Cell>) {
        let changes = original.into_iter().zip(&self.cells).enumerate()
            .filter_map(|(i, (before, &after))| if before == after { None } else { Some((i, before, after)) }).collect::<Vec<_>>();
        if changes.is_empty() { return; }
        self.push_action(Action::Cells(changes));
    }
    fn push_action(&mut self, action: Action) {
        self.undo.push_back(action); self.redo.clear();
        if self.undo.len() > 64 { self.undo.pop_front(); }
    }
    pub fn finish_stroke(&mut self) { if let Some(stroke) = self.stroke.take() { self.record(stroke.original); } }
    pub fn undo(&mut self) -> bool {
        self.finish_stroke();
        let Some(action) = self.undo.pop_back() else { return false; };
        match &action {
            Action::Cells(changes) => for &(i, before, _) in changes { self.cells[i] = before; },
            Action::Resize { before, .. } => self.restore(before),
        }
        self.redo.push(action); true
    }
    pub fn redo(&mut self) -> bool {
        self.finish_stroke();
        let Some(action) = self.redo.pop() else { return false; };
        match &action {
            Action::Cells(changes) => for &(i, _, after) in changes { self.cells[i] = after; },
            Action::Resize { after, .. } => self.restore(after),
        }
        self.undo.push_back(action); true
    }
    pub fn clear(&mut self) -> bool {
        self.finish_stroke();
        if self.cells.iter().all(|cell| *cell == self.blank) { return false; }
        let old = self.cells.clone(); self.cells.fill(self.blank); self.record(old); true
    }
    pub fn to_text(&self) -> String {
        let mut text = String::with_capacity((self.width + 1) * self.height);
        for row in self.cells.chunks(self.width) { for cell in row { text.push(cell.ch); } text.push('\n'); }
        text
    }
    pub fn from_text(text: &str) -> Result<Self, String> {
        let art = crate::config::preferences::AsciiBackground::try_from(text.to_owned())?;
        let rows: Vec<&str> = art.text().split_terminator('\n').collect();
        let width = rows.iter().map(|row| row.len()).max().unwrap_or(1).max(1);
        let mut doc = Self::new(width, rows.len().max(1))?;
        for (y, row) in rows.iter().enumerate() { for (x, ch) in row.chars().enumerate() { doc.cells[y * width + x].ch = ch; } }
        doc.saved = None; Ok(doc)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Project { version: u32, width: usize, height: usize, blank: Cell, cells: Vec<Cell> }
fn load(path: &Path) -> Result<Document, String> {
    let extension = path.extension().and_then(|v| v.to_str()).unwrap_or("").to_ascii_lowercase();
    if extension == "txt" || extension == "asc" {
        return crate::runtime::ascii_background::load(path).map_err(|e| e.to_string()).and_then(|art| Document::from_text(art.text()));
    }
    if extension != "asciipaint" { return Err("Choose an .asciipaint, .txt, or .asc file.".into()); }
    let file = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(path).map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_PROJECT_BYTES as u64 { return Err("Choose a regular project file of at most 2 MiB.".into()); }
    let mut text = String::new();
    file.take(MAX_PROJECT_BYTES as u64 + 1).read_to_string(&mut text).map_err(|e| e.to_string())?;
    if text.len() > MAX_PROJECT_BYTES { return Err("Project exceeds 2 MiB.".into()); }
    let project: Project = toml::from_str(&text).map_err(|e| e.to_string())?;
    if project.version != 1 { return Err("Unsupported ASCII Paint project version.".into()); }
    let mut doc = Document::new(project.width, project.height)?;
    if project.cells.len() != project.width * project.height || project.blank.ch != ' '
        || project.cells.iter().any(|cell| !(' '..='~').contains(&cell.ch)) { return Err("Invalid project cells or blank cell.".into()); }
    doc.blank = project.blank; doc.cells = project.cells; doc.mark_saved(); Ok(doc)
}
fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => return Err("Destination must be a regular file, not a symlink or directory.".into()),
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.to_string()),
        _ => {}
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let temporary = parent.join(format!(".asciipaint-{}-{}.tmp", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temporary).map_err(|e| e.to_string())?;
    let result = (|| -> std::io::Result<()> { file.write_all(contents)?; file.sync_all()?; std::fs::rename(&temporary, path) })();
    if result.is_err() { let _ = std::fs::remove_file(&temporary); }
    result.map_err(|e| format!("Cannot save {}: {e}", path.display()))
}
fn save(doc: &Document, path: &Path) -> Result<(), String> {
    let project = Project { version: 1, width: doc.width, height: doc.height, blank: doc.blank, cells: doc.cells.clone() };
    let text = toml::to_string(&project).map_err(|e| e.to_string())?;
    if text.len() > MAX_PROJECT_BYTES { return Err("Project exceeds 2 MiB.".into()); }
    atomic_write(path, text.as_bytes())
}
/// Maximum output dimensions in cells; terminal characters are twice as tall as wide.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ImageOptions { pub width: usize, pub height: usize, pub color: bool }
fn convert_image(source: &image::RgbaImage, options: ImageOptions) -> Result<Document, String> {
    if !(1..=MAX_WIDTH).contains(&options.width) || !(1..=MAX_HEIGHT).contains(&options.height) {
        return Err("Canvas dimensions must be 1–200 columns and 1–80 rows.".into());
    }
    crate::runtime::images::validate_dimensions(source.width(), source.height()).map_err(|e| e.to_string())?;
    let scale = (options.width as f64 / f64::from(source.width()))
        .min(options.height as f64 * 2.0 / f64::from(source.height()));
    let width = (f64::from(source.width()) * scale).round().clamp(1.0, options.width as f64) as usize;
    let height = (f64::from(source.height()) * scale / 2.0).round().clamp(1.0, options.height as f64) as usize;
    // Composite first so fully transparent RGB cannot bleed into neighboring samples.
    let opaque = image::RgbImage::from_fn(source.width(), source.height(), |x, y| {
        let pixel = source.get_pixel(x, y).0;
        image::Rgb(std::array::from_fn(|i| ((u32::from(pixel[i]) * u32::from(pixel[3]) + 127) / 255) as u8))
    });
    // Thumbnail averaging allocates only the bounded output, including for long, thin images.
    let sampled = image::imageops::thumbnail(&opaque, width as u32, height as u32);
    let mut doc = Document::new(width, height)?;
    const RAMP: &[u8] = b" .:-=+*#%@";
    for (cell, pixel) in doc.cells.iter_mut().zip(sampled.pixels()) {
        let rgb = pixel.0;
        let luminance = (u32::from(rgb[0]) * 2126 + u32::from(rgb[1]) * 7152 + u32::from(rgb[2]) * 722 + 5000) / 10000;
        let density = (luminance * (RAMP.len() as u32 - 1) + 127) / 255;
        cell.ch = RAMP[density as usize] as char;
        if options.color { cell.foreground = Rgb(rgb); }
    }
    doc.saved = None;
    Ok(doc)
}
/// Owns an immutable canvas snapshot; file I/O runs outside UI callbacks.
pub(crate) enum Operation { Open(PathBuf), ConvertImage(PathBuf, ImageOptions), Save(Document, PathBuf), Export(Document, PathBuf) }
#[derive(Debug)]
pub(crate) enum Completion { Opened(Box<Document>), Saved, Exported }
/// One outstanding operation, retained until polled. Drop detaches I/O and never joins.
pub(crate) struct PaintWorker { receiver: Option<mpsc::Receiver<Result<Completion, String>>> }
impl PaintWorker {
    pub fn new() -> Self { Self { receiver: None } }
    pub fn is_busy(&self) -> bool { self.receiver.is_some() }
    pub fn submit(&mut self, operation: Operation) -> bool {
        if self.is_busy() { return false; }
        let (sender, receiver) = mpsc::sync_channel(1);
        let spawned = std::thread::Builder::new().name("ascii-paint-io".into()).spawn(move || {
            let result = match operation {
                Operation::Open(path) => load(&path).map(|document| Completion::Opened(Box::new(document))),
                Operation::ConvertImage(path, options) => crate::runtime::images::decode(&path).map_err(|e| e.to_string())
                    .and_then(|source| convert_image(&source, options)).map(|document| Completion::Opened(Box::new(document))),
                Operation::Save(doc, path) => save(&doc, &path).map(|_| Completion::Saved),
                Operation::Export(doc, path) => atomic_write(&path, doc.to_text().as_bytes()).map(|_| Completion::Exported),
            };
            // Closing the window drops the receiver; completed I/O may then be discarded.
            let _ = sender.send(result);
        });
        if spawned.is_err() { return false; }
        self.receiver = Some(receiver); true
    }
    pub fn poll(&mut self) -> Option<Result<Completion, String>> {
        let result = self.receiver.as_ref()?.try_recv();
        match result {
            Ok(result) => { self.receiver = None; Some(result) }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => { self.receiver = None; Some(Err("ASCII Paint worker stopped unexpectedly.".into())) }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_conversion_fits_character_aspect_and_enforces_bounds() {
        for (source_width, source_height, width, height, expected) in [
            (4, 4, 4, 4, (4, 2)), (2, 8, 4, 2, (1, 2)), (8, 2, 4, 4, (4, 1)),
            (1, 100, 200, 80, (2, 80)), (100, 1, 1, 1, (1, 1)),
        ] {
            let source = image::RgbaImage::from_pixel(source_width, source_height, image::Rgba([255; 4]));
            let doc = convert_image(&source, ImageOptions { width, height, color: true }).unwrap();
            assert_eq!((doc.width(), doc.height()), expected);
            assert!(doc.is_dirty());
            assert!(doc.cells.iter().all(|cell| (' '..='~').contains(&cell.ch)));
        }
        let source = image::RgbaImage::new(1, 1);
        for (width, height) in [(0, 1), (201, 1), (1, 0), (1, 81), (usize::MAX, 1)] {
            assert!(convert_image(&source, ImageOptions { width, height, color: false }).is_err());
        }
        assert!(convert_image(&image::RgbaImage::new(0, 0), ImageOptions { width: 1, height: 1, color: true }).is_err());
    }
    #[test]
    fn image_conversion_maps_brightness_and_optional_sampled_rgb() {
        let source = image::RgbaImage::from_fn(3, 2, |x, _| {
            let value = match x { 0 => 0, 1 => 128, _ => 255 };
            image::Rgba([value, value, value, 255])
        });
        let doc = convert_image(&source, ImageOptions { width: 3, height: 1, color: false }).unwrap();
        assert_eq!(doc.to_text(), " +@\n");
        assert!(doc.cells.iter().all(|cell| cell.foreground == Rgb([255; 3]) && cell.background == Rgb([0; 3])));
        let source = image::RgbaImage::from_fn(3, 2, |x, _| image::Rgba(match x {
            0 => [255, 0, 0, 255], 1 => [0, 255, 0, 255], _ => [0, 0, 255, 255],
        }));
        let doc = convert_image(&source, ImageOptions { width: 3, height: 1, color: true }).unwrap();
        assert_eq!(doc.to_text(), ":*.\n");
        assert_eq!(doc.cell(0, 0).unwrap().foreground, Rgb([255, 0, 0]));
        assert_eq!(doc.cell(1, 0).unwrap().foreground, Rgb([0, 255, 0]));
        assert_eq!(doc.cell(2, 0).unwrap().foreground, Rgb([0, 0, 255]));
        assert_eq!(doc.blank(), Cell { ch: ' ', foreground: Rgb([255; 3]), background: Rgb([0; 3]) });
    }
    #[test]
    fn image_conversion_composites_alpha_before_resampling() {
        let source = image::RgbaImage::from_fn(2, 2, |x, _| image::Rgba(if x == 0 { [255, 0, 0, 0] } else { [255; 4] }));
        let doc = convert_image(&source, ImageOptions { width: 1, height: 1, color: true }).unwrap();
        assert_eq!(doc.cell(0, 0).unwrap().foreground, Rgb([128; 3]));
        assert_eq!(doc.to_text(), "+\n");
        let source = image::RgbaImage::from_pixel(1, 2, image::Rgba([200, 100, 50, 128]));
        let doc = convert_image(&source, ImageOptions { width: 1, height: 1, color: true }).unwrap();
        assert_eq!(doc.cell(0, 0).unwrap().foreground, Rgb([100, 50, 25]));
        let source = image::RgbaImage::from_pixel(1, 2, image::Rgba([255, 120, 33, 0]));
        let doc = convert_image(&source, ImageOptions { width: 1, height: 1, color: true }).unwrap();
        assert_eq!(doc.to_text(), " \n");
        assert_eq!(doc.cell(0, 0).unwrap().foreground, Rgb([0; 3]));
        assert!(doc.is_dirty());
    }
    #[test]
    fn resize_preserves_overlap_and_undo_restores_cropped_rgb() {
        let mut doc = Document::new(3, 2).unwrap();
        let ink = Cell { ch: '@', foreground: Rgb([17, 128, 255]), background: Rgb([5, 34, 89]) };
        doc.begin_stroke(2, 1, ink); doc.finish_stroke(); doc.mark_saved();
        assert!(doc.resize(2, 1).unwrap());
        assert_eq!((doc.width(), doc.height(), doc.to_text()), (2, 1, "  \n".into()));
        assert!(doc.is_dirty());
        assert!(doc.undo());
        assert_eq!((doc.width(), doc.height()), (3, 2));
        assert_eq!(doc.cell(2, 1), Some(ink));
        assert!(!doc.is_dirty());
        assert!(doc.redo());
        doc.mark_saved();
        assert!(doc.undo()); assert!(doc.is_dirty());
        assert!(doc.redo()); assert!(!doc.is_dirty());
        assert!(doc.resize(4, 3).unwrap());
        assert_eq!(doc.cell(3, 2), Some(doc.blank()));
        assert_eq!(doc.cell(2, 1), Some(doc.blank()));
    }
    #[test]
    fn resize_tracks_shape_and_keeps_redo_on_noop_or_invalid_dimensions() {
        let mut doc = Document::new(2, 3).unwrap();
        assert!(doc.resize(3, 2).unwrap());
        assert!(doc.is_dirty());
        assert!(doc.undo()); assert!(!doc.is_dirty());
        assert!(!doc.resize(2, 3).unwrap());
        for (width, height) in [(0, 1), (201, 1), (1, 0), (1, 81), (usize::MAX, 1)] {
            assert!(doc.resize(width, height).is_err());
            assert_eq!((doc.width(), doc.height()), (2, 3));
            assert!(!doc.is_dirty());
        }
        assert!(doc.redo());
        assert_eq!((doc.width(), doc.height()), (3, 2));
    }
    #[test]
    fn resize_finishes_strokes_and_shares_bounded_history_with_draw_and_clear() {
        let mut doc = Document::new(3, 2).unwrap();
        let ink = Cell { ch: '#', ..doc.blank() };
        doc.begin_stroke(0, 0, ink); doc.continue_stroke(2, 1);
        assert!(doc.resize(2, 1).unwrap());
        assert!(!doc.continue_stroke(1, 0));
        assert!(doc.clear());
        assert!(doc.undo()); assert_eq!(doc.cell(0, 0), Some(ink));
        assert!(doc.undo()); assert_eq!((doc.width(), doc.height()), (3, 2));
        assert_eq!(doc.cell(2, 1), Some(ink));
        assert!(doc.undo()); assert!(!doc.is_dirty());
        assert!(doc.redo()); assert!(doc.redo()); assert!(doc.redo());
        assert_eq!(doc.to_text(), "  \n");
        for i in 0..70 { doc.resize(if i % 2 == 0 { 3 } else { 2 }, 1).unwrap(); }
        let mut count = 0;
        while doc.undo() { count += 1; }
        assert_eq!(count, 64);
    }
    #[test]
    fn resized_projects_keep_dimensions_blank_and_rgb_on_disk() {
        let dir = crate::test_support::TestDir::new();
        let path = dir.path().join("resized.asciipaint");
        let mut doc = Document::new(2, 1).unwrap();
        doc.blank = Cell { ch: ' ', foreground: Rgb([1, 2, 3]), background: Rgb([4, 5, 6]) };
        let ink = Cell { ch: 'x', foreground: Rgb([7, 8, 9]), background: Rgb([10, 11, 12]) };
        doc.begin_stroke(0, 0, ink); doc.finish_stroke();
        doc.resize(3, 2).unwrap();
        save(&doc, &path).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!((loaded.width(), loaded.height()), (3, 2));
        assert_eq!(loaded.cell(0, 0), Some(ink));
        assert_eq!(loaded.cell(2, 1), Some(doc.blank()));
        assert_eq!(loaded.blank(), doc.blank());
        assert!(!loaded.is_dirty());
    }
    #[test]
    fn continuous_stroke_is_one_undo_and_tracks_saved_state() {
        let mut doc = Document::new(8, 4).unwrap();
        let ink = Cell { ch: '#', foreground: Rgb([1, 2, 3]), background: Rgb([4, 5, 6]) };
        doc.begin_stroke(0, 0, ink);
        doc.continue_stroke(7, 3);
        doc.finish_stroke();
        assert_eq!(doc.cell(3, 1), Some(ink));
        assert!(doc.is_dirty());
        doc.mark_saved();
        assert!(doc.undo());
        assert!(doc.is_dirty());
        assert!(doc.redo());
        assert!(!doc.is_dirty());
        assert!(doc.clear());
        assert!(doc.undo());
        assert!(!doc.is_dirty());
    }
    #[test]
    fn dimensions_erase_history_and_input_bounds() {
        assert!(Document::new(0, 1).is_err());
        assert!(Document::new(201, 1).is_err());
        assert!(Document::new(1, 81).is_err());
        let mut doc = Document::new(1, 1).unwrap();
        for i in 0..70 {
            doc.begin_stroke(0, 0, Cell { ch: if i % 2 == 0 {'x'} else {'y'}, ..doc.blank() });
            doc.finish_stroke();
        }
        assert!(!doc.begin_stroke(2, 0, doc.blank()));
        let mut count = 0;
        while doc.undo() { count += 1; }
        assert_eq!(count, 64);
        doc.begin_stroke(0, 0, doc.blank());
        doc.finish_stroke();
        assert_eq!(doc.cell(0, 0), Some(doc.blank()));
        assert!(!doc.redo());
        assert!(!doc.begin_stroke(0, 0, Cell { ch: 'é', ..doc.blank() }));
    }
    #[test]
    fn text_normalization_and_padded_export() {
        let doc = Document::from_text("a\t\r\n\n b \n").unwrap();
        assert_eq!((doc.width(), doc.height()), (4, 3));
        assert_eq!(doc.to_text(), "a   \n    \n b  \n");
        assert!(doc.is_dirty());
        for bad in ["é".to_owned(), "x".repeat(201), "\n".repeat(81), "\t".repeat(4097)] {
            assert!(Document::from_text(&bad).is_err());
        }
    }
    #[test]
    fn project_roundtrip_validation_and_atomic_save_failure() {
        let dir = crate::test_support::TestDir::new();
        let path = dir.path().join("drawing.asciipaint");
        let mut doc = Document::new(2, 1).unwrap();
        let cell = Cell { ch: '@', foreground: Rgb([0, 128, 255]), background: Rgb([12, 34, 56]) };
        doc.begin_stroke(1, 0, cell); doc.finish_stroke();
        save(&doc, &path).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.cell(1, 0), Some(cell));
        assert!(!loaded.is_dirty());
        let source = std::fs::read_to_string(&path).unwrap();
        for invalid in [source.replace("version = 1", "version = 2"), source.replace("width = 2", "width = 3"), source.replace("128", "256"), source.replace("@", "é")] {
            std::fs::write(&path, invalid).unwrap(); assert!(load(&path).is_err());
        }
        let blocked = dir.path().join("blocked");
        std::fs::create_dir(&blocked).unwrap();
        std::fs::write(blocked.join("sentinel"), "original").unwrap();
        assert!(save(&doc, &blocked).is_err());
        assert_eq!(std::fs::read_to_string(blocked.join("sentinel")).unwrap(), "original");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
        assert_eq!(Rgb::parse_hex("#0080FF").unwrap().to_hex(), "#0080FF");
        assert!(Rgb::parse_hex("#GG0000").is_err());
    }
}

#[cfg(test)]
mod io_tests {
    use super::*;
    #[test]
    fn image_worker_converts_fixture_and_returns_errors_without_replacing_document() {
        let dir = crate::test_support::TestDir::new();
        let image_path = dir.path().join("sample.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([255; 4])).save(&image_path).unwrap();
        let mut worker = PaintWorker::new();
        let options = ImageOptions { width: 4, height: 4, color: true };
        assert!(worker.submit(Operation::ConvertImage(image_path.clone(), options)));
        assert!(!worker.submit(Operation::ConvertImage(image_path.clone(), options)));
        let result = worker.receiver.take().unwrap().recv_timeout(std::time::Duration::from_secs(5)).unwrap().unwrap();
        let Completion::Opened(mut doc) = result else { panic!("conversion must open a document"); };
        assert_eq!((doc.width(), doc.height()), (4, 2));
        assert_eq!(doc.to_text(), "@@@@\n@@@@\n");
        assert!(doc.is_dirty());
        assert!(!doc.undo());
        let project_path = dir.path().join("converted.asciipaint");
        save(&doc, &project_path).unwrap();
        assert_eq!(load(&project_path).unwrap().cells, doc.cells);
        std::fs::write(&image_path, b"invalid image").unwrap();
        for path in [image_path, dir.path().join("missing.png")] {
            assert!(worker.submit(Operation::ConvertImage(path, options)));
            assert!(worker.receiver.take().unwrap().recv_timeout(std::time::Duration::from_secs(5)).unwrap().is_err());
            assert_eq!(doc.to_text(), "@@@@\n@@@@\n");
        }
    }
    #[test]
    fn maximum_project_roundtrips_and_bounded_regular_inputs_are_enforced() {
        let dir = crate::test_support::TestDir::new();
        let path = dir.path().join("max.asciipaint");
        save(&Document::new(200, 80).unwrap(), &path).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!((loaded.width(), loaded.height()), (200, 80));
        std::fs::write(&path, vec![b'x'; MAX_PROJECT_BYTES + 1]).unwrap();
        assert!(load(&path).is_err());
        let fifo = dir.path().join("fifo.asciipaint");
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(load(&fifo).is_err());
        let text = dir.path().join("import.ASC");
        std::fs::write(&text, " hi  \r\n\r\n").unwrap();
        let imported = load(&text).unwrap();
        assert_eq!(imported.to_text(), " hi  \n     \n");
        assert!(imported.is_dirty());
    }
    #[test]
    fn outstanding_operation_is_retained_until_poll_and_export_does_not_save() {
        let dir = crate::test_support::TestDir::new();
        let path = dir.path().join("export.txt");
        let doc = Document::from_text("x").unwrap();
        let mut worker = PaintWorker::new();
        assert!(worker.submit(Operation::Export(doc.clone(), path.clone())));
        assert!(worker.is_busy());
        assert!(!worker.submit(Operation::Open(dir.path().join("missing.asciipaint"))));
        let receiver = worker.receiver.take().unwrap();
        assert!(matches!(receiver.recv().unwrap().unwrap(), Completion::Exported));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "x\n");
        assert!(doc.is_dirty());
        let (sender, receiver) = mpsc::sync_channel(1);
        worker.receiver = Some(receiver);
        assert!(worker.poll().is_none());
        sender.send(Ok(Completion::Saved)).unwrap();
        assert!(matches!(worker.poll(), Some(Ok(Completion::Saved))));
        assert!(!worker.is_busy());
        assert!(worker.poll().is_none());
    }
    #[test]
    fn failed_save_leaves_existing_file_and_symlink_target_untouched() {
        let dir = crate::test_support::TestDir::new();
        let path = dir.path().join("original.asciipaint");
        std::fs::write(&path, "original").unwrap();
        let mut invalid = Document::new(200, 80).unwrap();
        invalid.cells.fill(Cell { ch: '\\', foreground: Rgb([255,255,255]), background: Rgb([255,255,255]) });
        // A failed destination must never truncate an existing neighboring document.
        assert!(save(&invalid, &dir.path().join("missing/file")).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(save(&invalid, &link).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "original");
    }
}
