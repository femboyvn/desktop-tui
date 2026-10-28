use crate::config::preferences::Rgb;
use crate::runtime::images::{self, Frame, RenderRequest, Zoom};
use crate::runtime::latest::LatestWorker;
use appcui::prelude::*;
use appcui::dialogs::{Location, OpenFileDialogFlags};
use std::path::PathBuf;
use std::time::Duration;

pub(crate) fn home_directory() -> PathBuf {
    directories::UserDirs::new().map(|dirs| dirs.home_dir().to_owned()).unwrap_or_else(|| PathBuf::from("/"))
}
pub(crate) fn select_image() -> Option<PathBuf> {
    dialogs::open("Open image (PNG, JPEG, WebP, GIF, BMP)", "", Location::Path(&home_directory()), None,
        OpenFileDialogFlags::Icons | OpenFileDialogFlags::CheckIfFileExists | OpenFileDialogFlags::PreserveBackground)
}

struct Request { path: PathBuf, source_revision: u64, render: RenderRequest }

#[CustomControl(overwrite = OnPaint+OnKeyPressed+OnMouseEvent)]
struct ImageCanvas {
    worker: LatestWorker<Request, Result<Frame, String>>,
    path: PathBuf,
    source_revision: u64,
    render: RenderRequest,
    frame: Option<Frame>,
    drag: Option<(i32, i32)>,
    caption: String,
    reported_error: Option<u64>,
}
impl ImageCanvas {
    fn new(path: PathBuf, background: Rgb) -> anyhow::Result<Self> {
        let mut cached = None;
        let worker = LatestWorker::new(move |request: Request| {
            let (revision, path, decoded) = cached.get_or_insert_with(|| (request.source_revision, request.path.clone(), images::decode(&request.path).map_err(|e| format!("{e:#}"))));
            if *revision != request.source_revision || *path != request.path {
                *revision = request.source_revision;
                *path = request.path.clone();
                *decoded = images::decode(&request.path).map_err(|e| format!("{e:#}"));
            }
            match decoded {
                Ok(image) => images::render(image, request.render).map_err(|e| format!("{e:#}")),
                Err(error) => Err(error.clone()),
            }
        })?;
        Ok(Self {
            base: ControlBase::new(Layout::fill(), true), worker, path, source_revision: 0,
            render: RenderRequest { size: (0, 0), mode: Zoom::Fit, pan: (0.0, 0.0), background: [background.0, background.1, background.2] },
            frame: None, drag: None, reported_error: None, caption: "Image Viewer — loading".into(),
        })
    }
    fn request(&mut self) {
        self.worker.submit(Request { path: self.path.clone(), source_revision: self.source_revision, render: self.render });
    }
    fn open(&mut self, path: PathBuf) {
        self.path = path;
        self.source_revision = self.source_revision.wrapping_add(1);
        self.render.mode = Zoom::Fit;
        self.render.pan = (0.0, 0.0);
        self.frame = None;
        self.caption = "Image Viewer — loading".into();
        self.request();
    }
    fn tick(&mut self) -> bool {
        let size = self.size();
        // Bound intermediate viewport allocations even for pathological terminal sizes.
        let size = (size.width.clamp(1, 2048), size.height.clamp(1, 1024));
        if self.render.size != size { self.render.size = size; self.request(); }
        match self.worker.poll() {
            Some(Ok(frame)) => {
                self.render.pan = frame.geometry.pan;
                self.caption = format!("{} — {} × {} — {:.1}%", self.path.file_name().unwrap_or(self.path.as_os_str()).to_string_lossy(), frame.dimensions.0, frame.dimensions.1, frame.geometry.scale * 100.0);
                self.frame = Some(frame);
                true
            }
            Some(Err(error)) => {
                self.caption = "Image Viewer — unable to open image (Ctrl+O to retry)".into();
                if self.reported_error != Some(self.source_revision) {
                    self.reported_error = Some(self.source_revision);
                    dialogs::error("Cannot open image", &format!("{}\n{error}", self.path.display()));
                }
                true
            }
            None => false,
        }
    }
    fn scale(&self) -> f64 {
        match self.render.mode {
            Zoom::Scale(scale) => scale,
            Zoom::Fit => self.frame.as_ref().map_or(1.0, |frame| frame.geometry.scale),
        }
    }
    fn zoom(&mut self, factor: f64) {
        self.render.mode = Zoom::Scale((self.scale() * factor).clamp(0.05, 4.0));
        self.request();
    }
}
impl OnPaint for ImageCanvas {
    fn on_paint(&self, surface: &mut Surface, theme: &Theme) {
        surface.clear(Character::new(' ', theme.window.normal.foreground, Color::RGB(self.render.background[0], self.render.background[1], self.render.background[2]), CharFlags::None));
        if let Some(frame) = &self.frame { surface.draw_surface(0, 0, &frame.surface); }
    }
}
impl OnKeyPressed for ImageCanvas {
    fn on_key_pressed(&mut self, key: Key, character: char) -> EventProcessStatus {
        if key.value() == key!("Ctrl+O") {
            if let Some(path) = select_image() { self.open(path); }
            return EventProcessStatus::Processed;
        }
        if key.modifier != KeyModifier::None && key.modifier != KeyModifier::Shift { return EventProcessStatus::Ignored; }
        match character {
            '+' | '=' => self.zoom(1.25),
            '-' => self.zoom(1.0 / 1.25),
            'f' | 'F' => { self.render.mode = Zoom::Fit; self.render.pan = (0.0, 0.0); self.request(); }
            '1' => { self.render.mode = Zoom::Scale(1.0); self.render.pan = (0.0, 0.0); self.request(); }
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
impl OnMouseEvent for ImageCanvas {
    fn on_mouse_event(&mut self, event: &MouseEvent) -> EventProcessStatus {
        match event {
            MouseEvent::Wheel(MouseWheelDirection::Up) => { self.request_focus(); self.zoom(1.25); }
            MouseEvent::Wheel(MouseWheelDirection::Down) => { self.request_focus(); self.zoom(1.0 / 1.25); }
            MouseEvent::Pressed(data) => { self.request_focus(); self.drag = Some((data.x, data.y)); }
            MouseEvent::Drag(data) => {
                if let Some((x, y)) = self.drag {
                    self.render.pan.0 -= f64::from(data.x - x) / self.scale();
                    self.render.pan.1 -= f64::from(data.y - y) * 2.0 / self.scale();
                    self.drag = Some((data.x, data.y));
                    self.request();
                }
            }
            MouseEvent::Released(_) | MouseEvent::Leave => self.drag = None,
            _ => return EventProcessStatus::Ignored,
        }
        EventProcessStatus::Processed
    }
}

#[Window(events = [TimerEvents, WindowEvents])]
pub(crate) struct ImageWindow { image: Handle<ImageCanvas>, closing: bool }
impl ImageWindow {
    pub fn new(path: PathBuf, background: Rgb) -> anyhow::Result<Self> {
        let image = ImageCanvas::new(path, background)?;
        let mut window = Self { base: Window::new("Image Viewer — Ctrl+O Open | F Fit | 1 Actual | +/- Zoom | Arrows/drag Pan", layout!("a:c,w:70%,h:75%"), window::Flags::Sizeable), image: Handle::None, closing: false };
        window.image = window.add(image);
        window.timer().ok_or_else(|| anyhow::anyhow!("Image Viewer timer unavailable"))?.start(Duration::from_millis(40));
        Ok(window)
    }
    pub fn open(&mut self, path: PathBuf) { let handle = self.image; if let Some(image) = self.control_mut(handle) { image.open(path); } }
    pub fn apply_background(&mut self, background: Rgb) {
        let handle = self.image;
        if let Some(image) = self.control_mut(handle) {
            image.render.background = [background.0, background.1, background.2];
            image.request();
        }
    }
    pub fn is_closing(&self) -> bool { self.closing }
    pub fn shutdown(&mut self) {
        self.closing = true;
        let handle = self.image;
        if let Some(image) = self.control_mut(handle) { image.worker.close(); }
        self.close();
    }
}
impl TimerEvents for ImageWindow {
    fn on_update(&mut self, _: u64) -> EventProcessStatus {
        if self.closing || !self.is_visible() { return EventProcessStatus::Ignored; }
        let handle = self.image;
        if let Some(image) = self.control_mut(handle)
            && image.tick() {
            let caption = image.caption.clone();
            self.set_title(&caption);
            return EventProcessStatus::Processed;
        }
        EventProcessStatus::Ignored
    }
}
impl WindowEvents for ImageWindow {
    fn on_cancel(&mut self) -> ActionRequest {
        self.closing = true;
        let handle = self.image;
        if let Some(image) = self.control_mut(handle) { image.worker.close(); }
        ActionRequest::Allow
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestDir, UI_LOCK};
    fn wait_frame(canvas: &mut ImageCanvas) {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !canvas.tick() {
            assert!(std::time::Instant::now() < deadline, "image worker did not finish");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn mouse_wheel_zooms_with_the_same_steps_and_bounds_as_keyboard() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut canvas = ImageCanvas::new(dir.path().join("unused.png"), Rgb(0,0,0)).unwrap();
        canvas.render.mode = Zoom::Scale(1.0);
        assert!(matches!(OnMouseEvent::on_mouse_event(&mut canvas, &MouseEvent::Wheel(MouseWheelDirection::Up)), EventProcessStatus::Processed));
        assert_eq!(canvas.render.mode, Zoom::Scale(1.25));
        OnMouseEvent::on_mouse_event(&mut canvas, &MouseEvent::Wheel(MouseWheelDirection::Down));
        assert_eq!(canvas.render.mode, Zoom::Scale(1.0));
        for _ in 0..30 { OnMouseEvent::on_mouse_event(&mut canvas, &MouseEvent::Wheel(MouseWheelDirection::Up)); }
        assert_eq!(canvas.render.mode, Zoom::Scale(4.0));
        for _ in 0..60 { OnMouseEvent::on_mouse_event(&mut canvas, &MouseEvent::Wheel(MouseWheelDirection::Down)); }
        assert_eq!(canvas.render.mode, Zoom::Scale(0.05));
        assert!(matches!(OnMouseEvent::on_mouse_event(&mut canvas, &MouseEvent::Wheel(MouseWheelDirection::Left)), EventProcessStatus::Ignored));
        canvas.worker.close();
        app.run();
    }
    #[test]
    fn hidden_viewer_defers_failed_results_without_opening_a_dialog() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let path = dir.path().join("missing secret image.png");
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut window = ImageWindow::new(path.clone(), Rgb(0,0,0)).unwrap();
        window.open(path);
        window.set_visible(false);
        let image = window.image;
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            assert!(matches!(TimerEvents::on_update(&mut window, 0), EventProcessStatus::Ignored));
            if let Some(result) = window.control_mut(image).unwrap().worker.poll() { assert!(result.is_err()); break; }
            assert!(std::time::Instant::now() < deadline, "hidden window consumed its pending result");
            std::thread::sleep(Duration::from_millis(2));
        }
        window.shutdown();
        app.run();
    }
    #[test]
    fn keyboard_zoom_pan_and_theme_changes_prepare_new_visible_pixels() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let path = dir.path().join("alpha.png");
        ::image::RgbaImage::from_pixel(100,80,::image::Rgba([200,100,0,128])).save(&path).unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut canvas = ImageCanvas::new(path, Rgb(20,40,60)).unwrap();
        // An unattached control has a one-cell viewport; Fit must handle it.
        wait_frame(&mut canvas);
        assert_eq!(canvas.frame.as_ref().unwrap().geometry.scale, 0.01);
        OnKeyPressed::on_key_pressed(&mut canvas, Key::None, '1');
        wait_frame(&mut canvas);
        assert_eq!(canvas.frame.as_ref().unwrap().geometry.scale, 1.0);
        OnKeyPressed::on_key_pressed(&mut canvas, Key::None, '+');
        wait_frame(&mut canvas);
        assert_eq!(canvas.frame.as_ref().unwrap().geometry.scale, 1.25);
        OnKeyPressed::on_key_pressed(&mut canvas, key!("Right").into(), '\0');
        wait_frame(&mut canvas);
        assert!(canvas.frame.as_ref().unwrap().geometry.pan.0 > 0.0);
        OnKeyPressed::on_key_pressed(&mut canvas, Key::None, 'F');
        wait_frame(&mut canvas);
        assert_eq!(canvas.render.pan, (0.0,0.0));
        canvas.render.background = [0,0,0];
        canvas.request();
        wait_frame(&mut canvas);
        assert_eq!(canvas.frame.as_ref().unwrap().surface.char(0,0).unwrap().foreground, Color::RGB(100,50,0));
        canvas.worker.close();
        app.run();
    }
}
