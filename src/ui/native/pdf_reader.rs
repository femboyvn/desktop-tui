use super::image_viewer::home_directory;
use crate::config::preferences::Rgb;
use crate::runtime::images::{Frame, RenderRequest, Zoom};
use crate::runtime::pdf::{PdfWorker, Request};
use appcui::dialogs::{Location, OpenFileDialogFlags};
use appcui::prelude::*;
use std::path::PathBuf;
use std::time::Duration;

fn select_pdf() -> Option<PathBuf> {
    dialogs::open("Open PDF", "", Location::Path(&home_directory()), None,
        OpenFileDialogFlags::Icons | OpenFileDialogFlags::CheckIfFileExists | OpenFileDialogFlags::PreserveBackground)
}

#[CustomControl(overwrite = OnPaint+OnKeyPressed+OnMouseEvent)]
struct PdfCanvas {
    worker: PdfWorker,
    path: Option<PathBuf>,
    revision: u64,
    page: u32,
    pages: u32,
    render: RenderRequest,
    frame: Option<Frame>,
    loading: bool,
    error: Option<String>,
    drag: Option<(i32, i32)>,
}
impl PdfCanvas {
    fn new(path: Option<PathBuf>, background: Rgb) -> anyhow::Result<Self> {
        let mut canvas = Self {
            base: ControlBase::new(Layout::fill(), true), worker: PdfWorker::new()?, path,
            revision: 0, page: 1, pages: 0, frame: None, loading: false, error: None, drag: None,
            render: RenderRequest { size: (1,1), mode: Zoom::Fit, pan: (0.0,0.0), background: [background.0,background.1,background.2] },
        };
        canvas.request();
        Ok(canvas)
    }
    fn request(&mut self) {
        if let Some(path) = &self.path {
            self.loading = true;
            self.error = None;
            self.worker.submit(Request { path: path.clone(), revision: self.revision, page: self.page, render: self.render });
        }
    }
    fn open(&mut self, path: PathBuf) {
        self.path = Some(path);
        self.revision = self.revision.wrapping_add(1);
        self.page = 1;
        self.pages = 0;
        self.frame = None;
        self.drag = None;
        self.render.mode = Zoom::Fit;
        self.render.pan = (0.0,0.0);
        self.request();
    }
    fn choose_file(&mut self) { if let Some(path) = select_pdf() { self.open(path); } }
    fn change_page(&mut self, page: u32) {
        let page = page.clamp(1, self.pages.max(1));
        if page == self.page { return; }
        self.page = page;
        self.render.pan = (0.0,0.0);
        self.frame = None;
        self.request();
    }
    fn scale(&self) -> f64 {
        self.frame.as_ref().map_or(1.0, |frame| frame.geometry.scale)
    }
    fn zoom_factor(&self) -> f64 {
        match self.render.mode { Zoom::FitScale(factor) => factor, _ => 1.0 }
    }
    fn zoom(&mut self, factor: f64) {
        if self.frame.is_none() { return; }
        self.render.mode = Zoom::FitScale((self.zoom_factor() * factor).clamp(0.25, 16.0));
        self.request();
    }
    fn fit(&mut self) { self.render.mode = Zoom::Fit; self.render.pan = (0.0,0.0); self.request(); }
    fn tick(&mut self) -> bool {
        let size = self.size();
        let viewport = (size.width.clamp(1,2048), size.height.saturating_sub(2).clamp(1,1024));
        if self.render.size != viewport { self.render.size = viewport; self.request(); }
        match self.worker.poll() {
            Some(Ok(result)) => {
                self.page = result.page;
                self.pages = result.pages;
                self.render.pan = result.frame.geometry.pan;
                self.frame = Some(result.frame);
                self.loading = false;
                self.error = None;
                true
            }
            Some(Err(error)) => { self.frame = None; self.loading = false; self.error = Some(error); true }
            None => false,
        }
    }
    fn title(&self) -> String {
        self.path.as_ref().map(|path| format!("PDF Reader: {} — Page {}/{}", path.file_name().unwrap_or(path.as_os_str()).to_string_lossy(), self.page,
            if self.pages == 0 { "…".into() } else { self.pages.to_string() })).unwrap_or_else(|| "PDF Reader".into())
    }
}
impl OnPaint for PdfCanvas {
    fn on_paint(&self, surface: &mut Surface, theme: &Theme) {
        let [r,g,b] = self.render.background;
        surface.clear(Character::new(' ', theme.text.normal.foreground, Color::RGB(r,g,b), CharFlags::None));
        surface.write_string(0,0,"[ Open ]  [ Prev ]  [ Next ]  [ Fit ]  [ - ]  [ + ]", theme.text.normal, false);
        let status = if self.loading { format!("Page {} — Loading…", self.page) }
            else if self.pages > 0 { format!("Page {}/{} | {:.0}% Fit | PgUp/PgDn Pages | Arrows Pan | Ctrl+O Open", self.page, self.pages, self.zoom_factor() * 100.0) }
            else { "Ctrl+O Open PDF | PgUp/PgDn Pages | +/- or wheel Zoom | F Fit".into() };
        surface.write_string(0,1,&status,theme.text.normal,false);
        if let Some(frame) = &self.frame { surface.draw_surface(0,2,&frame.surface); }
        else {
            let message = self.error.as_deref().unwrap_or(if self.loading { "Loading PDF…" } else { "Open a local PDF with Open or Ctrl+O." });
            let width = surface.size().width.clamp(1,2048) as usize;
            let rows = surface.size().height.saturating_sub(3) as usize;
            let mut row = 3;
            for line in message.lines() {
                let characters: Vec<_> = line.chars().collect();
                for part in characters.chunks(width) {
                    if row >= rows + 3 { return; }
                    surface.write_string(0,row as i32,&part.iter().collect::<String>(),theme.text.normal,false);
                    row += 1;
                }
            }
        }
    }
}
impl OnKeyPressed for PdfCanvas {
    fn on_key_pressed(&mut self, key: Key, character: char) -> EventProcessStatus {
        if key.value() == key!("Ctrl+O") { self.choose_file(); return EventProcessStatus::Processed; }
        if key.modifier != KeyModifier::None && key.modifier != KeyModifier::Shift { return EventProcessStatus::Ignored; }
        match (key.code, character) {
            (KeyCode::PageDown, _) | (_, 'n' | 'N') => self.change_page(self.page.saturating_add(1)),
            (KeyCode::PageUp, _) | (_, 'p' | 'P') => self.change_page(self.page.saturating_sub(1)),
            (KeyCode::Home, _) => self.change_page(1),
            (KeyCode::End, _) => self.change_page(self.pages.max(1)),
            (_, '+' | '=') => self.zoom(1.25),
            (_, '-') => self.zoom(1.0 / 1.25),
            (_, 'f' | 'F') => self.fit(),
            _ => {
                let step = 8.0 / self.scale();
                match key.code {
                    KeyCode::Left => self.render.pan.0 -= step,
                    KeyCode::Right => self.render.pan.0 += step,
                    KeyCode::Up => self.render.pan.1 -= step,
                    KeyCode::Down => self.render.pan.1 += step,
                    _ => return EventProcessStatus::Ignored,
                }
                self.request();
            }
        }
        EventProcessStatus::Processed
    }
}
impl OnMouseEvent for PdfCanvas {
    fn on_mouse_event(&mut self, event: &MouseEvent) -> EventProcessStatus {
        match event {
            MouseEvent::Wheel(MouseWheelDirection::Up) => { self.request_focus(); self.zoom(1.25); }
            MouseEvent::Wheel(MouseWheelDirection::Down) => { self.request_focus(); self.zoom(1.0 / 1.25); }
            MouseEvent::Pressed(data) => {
                self.request_focus();
                if data.y == 0 {
                    match data.x {
                        0..=7 => self.choose_file(),
                        10..=17 => self.change_page(self.page.saturating_sub(1)),
                        20..=27 => self.change_page(self.page.saturating_add(1)),
                        30..=36 => self.fit(),
                        39..=43 => self.zoom(1.0 / 1.25),
                        46..=50 => self.zoom(1.25),
                        _ => return EventProcessStatus::Ignored,
                    }
                } else if data.y >= 2 { self.drag = Some((data.x,data.y)); }
            }
            MouseEvent::Drag(data) => {
                if let Some((x,y)) = self.drag {
                    self.render.pan.0 -= f64::from(data.x - x) / self.scale();
                    self.render.pan.1 -= f64::from(data.y - y) * 2.0 / self.scale();
                    self.drag = Some((data.x,data.y));
                    self.request();
                }
            }
            MouseEvent::Released(_) | MouseEvent::Leave => self.drag = None,
            _ => return EventProcessStatus::Ignored,
        }
        EventProcessStatus::Processed
    }
}

#[Window(events = TimerEvents+WindowEvents)]
pub(crate) struct PdfWindow { canvas: Handle<PdfCanvas>, closing: bool }
impl PdfWindow {
    pub fn new(path: Option<PathBuf>, background: Rgb) -> anyhow::Result<Self> {
        let canvas = PdfCanvas::new(path, background)?;
        let mut window = Self { base: Window::new("PDF Reader", layout!("a:c,w:80%,h:80%"), window::Flags::Sizeable), canvas: Handle::None, closing: false };
        // Reserve the timer before registering the worker-owning child globally.
        window.timer().ok_or_else(|| anyhow::anyhow!("PDF Reader timer unavailable"))?.start(Duration::from_millis(40));
        window.canvas = window.add(canvas);
        let handle = window.canvas;
        if let Some(canvas) = window.control_mut(handle) { canvas.request_focus(); }
        Ok(window)
    }
    pub fn open(&mut self, path: PathBuf) { let handle = self.canvas; if let Some(canvas) = self.control_mut(handle) { canvas.open(path); } }
    pub fn apply_background(&mut self, background: Rgb) {
        let handle = self.canvas;
        if let Some(canvas) = self.control_mut(handle) { canvas.render.background = [background.0,background.1,background.2]; canvas.request(); }
    }
    pub fn is_closing(&self) -> bool { self.closing }
    fn cancel(&mut self) { self.closing = true; let handle = self.canvas; if let Some(canvas) = self.control_mut(handle) { canvas.worker.close(); } }
    pub fn shutdown(&mut self) { self.cancel(); self.close(); }
}
impl TimerEvents for PdfWindow {
    fn on_update(&mut self, _: u64) -> EventProcessStatus {
        if self.closing || !self.is_visible() { return EventProcessStatus::Ignored; }
        let handle = self.canvas;
        if let Some(canvas) = self.control_mut(handle) && canvas.tick() {
            let title = canvas.title();
            self.set_title(&title);
            return EventProcessStatus::Processed;
        }
        EventProcessStatus::Ignored
    }
}
impl WindowEvents for PdfWindow { fn on_cancel(&mut self) -> ActionRequest { self.cancel(); ActionRequest::Allow } }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestDir, UI_LOCK};

    #[test]
    fn zoom_out_from_a_tiny_fit_shrinks_and_wheel_matches_keyboard() {
        let _lock = UI_LOCK.lock().unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut canvas = PdfCanvas::new(None, Rgb(0,0,0)).unwrap();
        let source = ::image::RgbaImage::from_pixel(100,100,::image::Rgba([255,255,255,255]));
        canvas.frame = Some(crate::runtime::images::render(&source,canvas.render).unwrap());
        OnKeyPressed::on_key_pressed(&mut canvas, Key::None, '-');
        let smaller = crate::runtime::images::render(&source,canvas.render).unwrap();
        assert!((smaller.geometry.scale - 0.008).abs() < 1e-9);
        OnMouseEvent::on_mouse_event(&mut canvas, &MouseEvent::Wheel(MouseWheelDirection::Up));
        assert_eq!(canvas.zoom_factor(), 1.0);
        for _ in 0..50 { OnKeyPressed::on_key_pressed(&mut canvas, Key::None, '+'); }
        assert_eq!(canvas.zoom_factor(), 16.0);
        for _ in 0..50 { OnMouseEvent::on_mouse_event(&mut canvas, &MouseEvent::Wheel(MouseWheelDirection::Down)); }
        assert_eq!(canvas.zoom_factor(), 0.25);
        OnKeyPressed::on_key_pressed(&mut canvas, Key::None, 'f');
        assert_eq!(canvas.render.mode, Zoom::Fit);
        canvas.worker.close();
        app.run();
    }

    #[test]
    fn page_navigation_clamps_and_open_resets_document_state() {
        let _lock = UI_LOCK.lock().unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut canvas = PdfCanvas::new(None, Rgb(0,0,0)).unwrap();
        canvas.pages = 3;
        canvas.render.pan = (10.0,20.0);
        OnKeyPressed::on_key_pressed(&mut canvas, key!("PageDown").into(), '\0');
        assert_eq!(canvas.page, 2);
        assert_eq!(canvas.render.pan, (0.0,0.0));
        OnKeyPressed::on_key_pressed(&mut canvas, key!("End").into(), '\0');
        OnKeyPressed::on_key_pressed(&mut canvas, key!("PageDown").into(), '\0');
        assert_eq!(canvas.page, 3);
        OnKeyPressed::on_key_pressed(&mut canvas, key!("Home").into(), '\0');
        OnKeyPressed::on_key_pressed(&mut canvas, key!("PageUp").into(), '\0');
        assert_eq!(canvas.page, 1);
        assert!(matches!(OnKeyPressed::on_key_pressed(&mut canvas, key!("Ctrl+N").into(), 'n'), EventProcessStatus::Ignored));
        canvas.open(PathBuf::from("new.pdf"));
        assert_eq!((canvas.page, canvas.pages), (1,0));
        assert!(canvas.frame.is_none());
        assert_eq!(canvas.render.mode, Zoom::Fit);
        canvas.worker.close();
        app.run();
    }
    #[test]
    fn hidden_reader_defers_errors_and_close_cancels_loading() {
        let _lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut window = PdfWindow::new(Some(dir.path().join("missing.pdf")), Rgb(0,0,0)).unwrap();
        window.set_visible(false);
        let handle = window.canvas;
        window.control_mut(handle).unwrap().request();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            assert!(matches!(TimerEvents::on_update(&mut window, 0), EventProcessStatus::Ignored));
            if let Some(result) = window.control_mut(handle).unwrap().worker.poll() { assert!(result.is_err()); break; }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        window.shutdown();
        assert!(window.is_closing());
        app.run();
    }
}
