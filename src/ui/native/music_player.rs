use super::image_viewer::home_directory;
use crate::runtime::music::{MusicPlayer, PlaybackState, Snapshot};
use appcui::dialogs::{Location, OpenFileDialogFlags};
use appcui::prelude::*;
use std::path::PathBuf;
use std::time::Duration;

fn time_label(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
fn paint_spectrum(surface: &mut Surface, theme: &Theme, snapshot: &Snapshot, top: usize) {
    let frame_width = surface.size().width.min(4096) as usize;
    let rows = (surface.size().height as usize).saturating_sub(top);
    if frame_width < 3 || rows < 6 { return; }
    // Reserve the border and label row, leaving at least three rows for bars.
    surface.draw_rect(Rect::new(0, top as i32, (frame_width - 1) as i32, (top + rows - 2) as i32), LineType::Single, theme.lines.normal);
    let width = frame_width - 2;
    let height = rows - 3;
    let top = top + 1;
    let bars = width.div_ceil(2).min(snapshot.levels.len());
    let bar_character = Character::with_attributes('#', theme.text.enphasized_1);
    let peak_character = Character::with_attributes('-', theme.text.enphasized_2);
    for bar in 0..bars {
        let from = bar * snapshot.levels.len() / bars;
        let to = (bar + 1) * snapshot.levels.len() / bars;
        let level = snapshot.levels[from..to].iter().copied().fold(0.0_f32, f32::max).clamp(0.0, 1.0);
        let peak = snapshot.peaks[from..to].iter().copied().fold(level, f32::max).clamp(0.0, 1.0);
        // Keep columns thin as the window grows, centered in their band slots.
        let slot_left = bar * (width + 1) / bars;
        let slot_end = ((bar + 1) * (width + 1) / bars - 1).min(width);
        let bar_width = (slot_end - slot_left).min(2);
        let left = 1 + slot_left + (slot_end - slot_left - bar_width) / 2;
        let right = left + bar_width - 1;
        let filled = (level * height as f32).ceil() as usize;
        for row in top + height - filled..top + height {
            surface.fill_horizontal_line(left as i32, row as i32, right as i32, bar_character);
        }
        let peak_height = (peak * height as f32).ceil() as usize;
        if peak_height > 0 {
            surface.fill_horizontal_line(left as i32, (top + height - peak_height) as i32, right as i32, peak_character);
        }
    }
    let labels = [(40.0_f32,"40"), (63.0,"63"), (125.0,"125"), (250.0,"250"), (500.0,"500"),
        (1000.0,"1k"), (2000.0,"2k"), (4000.0,"4k"), (8000.0,"8k"), (16000.0,"16k")];
    let log_range = (snapshot.spectrum_max_hz / 40.0).ln();
    let mut next_label = width + 1;
    // Prefer the high-frequency endpoint when labels would collide. Draw only
    // whole labels, with a space between them, on the same log scale as the FFT.
    for (frequency, label) in labels.into_iter().rev() {
        if frequency > snapshot.spectrum_max_hz || label.len() > width { continue; }
        let center = ((frequency / 40.0).ln() / log_range * (width - 1) as f32).round() as usize;
        let start = center.saturating_sub(label.len() / 2).min(width - label.len());
        if start + label.len() < next_label {
            surface.write_string((start + 1) as i32, (top + height + 1) as i32, label, theme.text.normal, false);
            next_label = start;
        }
    }
}
#[CustomControl(overwrite = OnPaint+OnKeyPressed+OnMouseEvent)]
struct MusicStatus { player: MusicPlayer, snapshot: Snapshot }
impl MusicStatus {
    fn new() -> anyhow::Result<Self> { Ok(Self { base: ControlBase::new(Layout::fill(), true), player: MusicPlayer::new()?, snapshot: Snapshot::default() }) }
    fn choose_file(&mut self) {
        if let Some(path) = dialogs::open("Open audio (MP3, WAV, FLAC, Ogg Vorbis)", "", Location::Path(&home_directory()), None,
            OpenFileDialogFlags::Icons | OpenFileDialogFlags::CheckIfFileExists | OpenFileDialogFlags::PreserveBackground) {
            self.player.open(path);
        }
    }
}
impl OnPaint for MusicStatus {
    fn on_paint(&self, surface: &mut Surface, theme: &Theme) {
        surface.clear(Character::with_attributes(' ', theme.window.normal));
        surface.write_string(0, 0, "[ Open ]  [ Play/Pause ]  [ Stop ]", theme.text.normal, false);
        let snapshot = &self.snapshot;
        let filename = snapshot.path.as_ref().map(|path| path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned())
            .unwrap_or_else(|| "Choose a local audio file with Open or Ctrl+O.".into());
        surface.write_string(0, 2, &filename, theme.text.normal, false);
        let state = match snapshot.state {
            PlaybackState::Idle => "Ready", PlaybackState::Loading => "Loading", PlaybackState::Playing => "Playing",
            PlaybackState::Paused => "Paused", PlaybackState::Stopped => "Stopped", PlaybackState::Finished => "Finished",
            PlaybackState::Error => "Unable to play", PlaybackState::Closed => "Closed",
        };
        surface.write_string(0, 3, &format!("{state}  {} / {}  Vol {:.0}%", time_label(snapshot.position), time_label(snapshot.duration), snapshot.volume * 100.0), theme.text.normal, false);
        let width = surface.size().width.min(4096) as usize;
        let progress = if snapshot.duration > 0.0 { (snapshot.position / snapshot.duration).clamp(0.0, 1.0) } else { 0.0 };
        let filled = (width as f64 * progress) as usize;
        surface.write_string(0, 4, &("━".repeat(filled) + &"─".repeat(width - filled)), theme.text.normal, false);
        let message = snapshot.error.as_deref().unwrap_or("Space Play/Pause | S Stop | Left/Right Seek 10s\n+/- Volume | Ctrl+O Open (MP3, WAV, FLAC, Ogg Vorbis)");
        // Wrap errors, including long OS paths, within even very small surfaces.
        let rows = surface.size().height.saturating_sub(5) as usize;
        let mut row = 5;
        for line in message.lines() {
            let characters: Vec<_> = line.chars().collect();
            for part in characters.chunks(width.max(1)) {
                if row >= rows + 5 { return; }
                surface.write_string(0, row as i32, &part.iter().collect::<String>(), theme.text.normal, false);
                row += 1;
            }
        }
        paint_spectrum(surface, theme, snapshot, row);
    }
}

#[Window(events = TimerEvents+WindowEvents)]
pub(crate) struct MusicWindow { status: Handle<MusicStatus>, closing: bool }
impl MusicWindow {
    pub fn document_path(&self) -> Option<PathBuf> { self.control(self.status).and_then(|status| status.player.snapshot().path.clone()) }
    pub fn new() -> anyhow::Result<Self> {
        let status = MusicStatus::new()?;
        let mut window = Self {
            base: Window::new("Music Player", layout!("a:c,w:75%,h:70%"), window::Flags::Sizeable),
            status: Handle::None, closing: false,
        };
        window.status = window.add(status);
        let status = window.status;
        if let Some(status) = window.control_mut(status) { status.request_focus(); }
        window.timer().ok_or_else(|| anyhow::anyhow!("Music Player timer unavailable"))?.start(Duration::from_millis(100));
        Ok(window)
    }
    pub fn open(&mut self, path: PathBuf) {
        let status = self.status;
        if let Some(status) = self.control_mut(status) { status.player.open(path); }
        self.refresh();
    }
    fn refresh(&mut self) -> bool {
        let handle = self.status;
        let Some(status) = self.control(handle) else { return false; };
        let snapshot = status.player.snapshot();
        if status.snapshot == snapshot { return false; }
        let title = snapshot.path.as_ref().map(|path| format!("Music Player — {}", path.file_name().unwrap_or(path.as_os_str()).to_string_lossy()))
            .unwrap_or_else(|| "Music Player".into());
        self.set_title(&title);
        if let Some(status) = self.control_mut(handle) { status.snapshot = snapshot; }
        true
    }
    pub fn is_closing(&self) -> bool { self.closing }
    fn close_player(&mut self) {
        self.closing = true;
        let status = self.status;
        if let Some(status) = self.control_mut(status) { status.player.close(); }
    }
    pub fn shutdown(&mut self) { self.close_player(); self.close(); }
}
impl OnKeyPressed for MusicStatus {
    fn on_key_pressed(&mut self, key: Key, character: char) -> EventProcessStatus {
        if key.value() == key!("Ctrl+O") { self.choose_file(); return EventProcessStatus::Processed; }
        if key.modifier == KeyModifier::None || key.modifier == KeyModifier::Shift {
            match (key.code, character) {
                (KeyCode::Space, _) | (_, ' ') => self.player.toggle(),
                (KeyCode::Left, _) => self.player.seek_by(-10.0),
                (KeyCode::Right, _) => self.player.seek_by(10.0),
                (_, 's' | 'S') => self.player.stop(),
                (_, '+' | '=') => self.player.adjust_volume(0.05),
                (_, '-') => self.player.adjust_volume(-0.05),
                _ => return EventProcessStatus::Ignored,
            }
            self.snapshot = self.player.snapshot();
            return EventProcessStatus::Processed;
        }
        EventProcessStatus::Ignored
    }
}
impl OnMouseEvent for MusicStatus {
    fn on_mouse_event(&mut self, event: &MouseEvent) -> EventProcessStatus {
        if let MouseEvent::Pressed(data) = event {
            self.request_focus();
            if data.y == 0 {
                match data.x {
                    0..=7 => self.choose_file(),
                    10..=23 => self.player.toggle(),
                    26..=33 => self.player.stop(),
                    _ => return EventProcessStatus::Ignored,
                }
                self.snapshot = self.player.snapshot();
            }
            return EventProcessStatus::Processed;
        }
        EventProcessStatus::Ignored
    }
}
impl TimerEvents for MusicWindow {
    fn on_update(&mut self, _: u64) -> EventProcessStatus {
        if self.closing || !self.is_visible() { return EventProcessStatus::Ignored; }
        if self.refresh() { EventProcessStatus::Processed } else { EventProcessStatus::Ignored }
    }
}
impl WindowEvents for MusicWindow {
    fn on_cancel(&mut self) -> ActionRequest { self.close_player(); ActionRequest::Allow }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestDir, UI_LOCK};
    fn span(surface: &Surface, left: i32, y: i32, width: u32) -> String {
        (0..width).map(|x| surface.char(left + x as i32, y).unwrap().code).collect()
    }
    fn row(surface: &Surface, y: i32) -> String {
        span(surface, 0, y, surface.size().width)
    }
    #[test]
    fn spectrum_rectangle_encloses_bars_and_keeps_labels_below_its_bottom_edge() {
        let mut snapshot = Snapshot::default();
        snapshot.levels.fill(1.0);
        snapshot.peaks.fill(1.0);
        let mut theme = Theme::new(Themes::Light);
        theme.lines.normal.foreground = Color::RGB(21, 43, 65);
        let mut surface = Surface::new(5,6);
        surface.clear(Character::with_attributes(' ', theme.window.normal));
        paint_spectrum(&mut surface, &theme, &snapshot, 0);
        assert_eq!(row(&surface, 0), "┌───┐");
        assert_eq!(row(&surface, 1), "│- -│");
        assert_eq!(row(&surface, 2), "│# #│");
        assert_eq!(row(&surface, 3), "│# #│");
        assert_eq!(row(&surface, 4), "└───┘");
        assert_eq!(row(&surface, 5), " 16k ");
        assert_eq!(surface.char(0,0).unwrap().foreground, theme.lines.normal.foreground);
        for (width, height) in [(1,6), (2,6), (5,5)] {
            let mut surface = Surface::new(width,height);
            surface.clear(Character::with_attributes(' ', theme.window.normal));
            paint_spectrum(&mut surface, &theme, &snapshot, 0);
            for y in 0..height { assert!(row(&surface, y as i32).chars().all(|ch| ch == ' ')); }
        }
    }
    #[test]
    fn spectrum_columns_stay_thin_and_frequency_labels_follow_the_logarithmic_axis() {
        let mut snapshot = Snapshot::default();
        snapshot.levels.fill(1.0);
        snapshot.peaks.fill(1.0);
        let theme = Theme::new(Themes::Light);
        let mut surface = Surface::new(162,7);
        surface.clear(Character::with_attributes(' ', theme.window.normal));
        paint_spectrum(&mut surface, &theme, &snapshot, 0);
        let bars = span(&surface, 1, 2, 160);
        assert_eq!(bars.matches('#').count(), 32);
        assert!(bars.split_whitespace().all(|bar| bar == "##"));
        let mut surface = Surface::new(66,7);
        surface.clear(Character::with_attributes(' ', theme.window.normal));
        paint_spectrum(&mut surface, &theme, &snapshot, 0);
        let labels = span(&surface, 1, 6, 64);
        for (start, label) in [(0,"40"), (4,"63"), (11,"125"), (18,"250"), (26,"500"), (33,"1k"), (40,"2k"), (47,"4k"), (55,"8k"), (61,"16k")] {
            assert_eq!(&labels[start..start + label.len()], label, "{labels}");
        }
        assert_eq!(surface.char(34,6).unwrap().foreground, theme.text.normal.foreground);
        snapshot.spectrum_max_hz = 4000.0;
        surface.clear(Character::with_attributes(' ', theme.window.normal));
        paint_spectrum(&mut surface, &theme, &snapshot, 0);
        let labels = span(&surface, 1, 6, 64);
        assert!(labels.ends_with("4k"), "{labels}");
        assert!(!labels.contains("8k") && !labels.contains("16k"));
        // Crowded labels are omitted whole; even tiny axes stay within bounds.
        let mut surface = Surface::new(7,7);
        surface.clear(Character::with_attributes(' ', theme.window.normal));
        paint_spectrum(&mut surface, &theme, &snapshot, 0);
        assert_eq!(row(&surface, 6), " 63 4k ");
    }
    #[test]
    fn spectrum_renders_exact_bars_peaks_and_live_theme_colors_below_help() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut status = MusicStatus::new().unwrap();
        status.snapshot.levels[1] = 0.5;
        status.snapshot.peaks[1] = 1.0;
        status.snapshot.levels[2] = 1.0;
        status.snapshot.peaks[2] = 1.0;
        status.snapshot.levels[15] = 1.0;
        status.snapshot.peaks[15] = 1.0;
        for preset in [Themes::Light, Themes::DarkGray, Themes::Default] {
            let mut theme = Theme::new(preset);
            theme.text.enphasized_1.foreground = Color::RGB(23, 45, 67);
            theme.text.enphasized_2.foreground = Color::RGB(89, 123, 234);
            let mut surface = Surface::new(66, 14);
            status.on_paint(&mut surface, &theme);
            assert!(row(&surface, 6).starts_with("+/- Volume"));
            assert!(row(&surface, 7).starts_with('┌') && row(&surface, 7).ends_with('┐'));
            assert_eq!(span(&surface, 1, 8, 12), "    --  --  ");
            assert_eq!(span(&surface, 1, 9, 12), "        ##  ");
            assert_eq!(span(&surface, 1, 10, 12), "    ##  ##  ");
            assert_eq!(span(&surface, 1, 11, 12), "    ##  ##  ");
            assert_eq!(span(&surface, 61, 11, 4), " ## ");
            assert!(row(&surface, 12).starts_with('└') && row(&surface, 12).ends_with('┘'));
            assert!(row(&surface, 13).contains("1k"));
            assert_eq!(surface.char(5, 10).unwrap().foreground, theme.text.enphasized_1.foreground);
            assert_eq!(surface.char(5, 8).unwrap().foreground, theme.text.enphasized_2.foreground);
            assert_eq!(surface.char(5, 10).unwrap().background, theme.window.normal.background);
        }
        status.player.close();
        app.run();
    }
    #[test]
    fn narrow_spectrum_merges_neighboring_bands_and_tiny_surfaces_prioritize_text() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut status = MusicStatus::new().unwrap();
        status.snapshot.levels = [1.0; 16];
        status.snapshot.peaks = [1.0; 16];
        let theme = Theme::new(Themes::Light);
        for (width, height) in [(0,0), (1,1), (64,9), (64,10), (64,12), (20,8)] {
            let mut surface = Surface::new(width, height);
            status.on_paint(&mut surface, &theme);
            for y in 7..height { assert!(!row(&surface, y as i32).contains('#')); }
        }
        // Two help lines occupy 21 rows; three bar rows, a frame, and labels fit.
        status.snapshot.levels = [0.0; 16];
        status.snapshot.peaks = [0.0; 16];
        status.snapshot.levels[4] = 1.0;
        status.snapshot.peaks[4] = 1.0;
        status.snapshot.levels[9] = 0.5;
        status.snapshot.peaks[9] = 1.0;
        status.snapshot.levels[15] = 1.0;
        status.snapshot.peaks[15] = 1.0;
        let mut surface = Surface::new(5,32);
        status.on_paint(&mut surface, &theme);
        assert_eq!(row(&surface, 26), "┌───┐");
        assert_eq!(row(&surface, 27), "│- -│");
        assert_eq!(row(&surface, 28), "│# #│");
        assert_eq!(row(&surface, 29), "│# #│");
        assert_eq!(row(&surface, 30), "└───┘");
        assert_eq!(row(&surface, 31), " 16k ");
        status.player.close();
        app.run();
    }
    #[test]
    fn wrapped_errors_keep_all_rows_before_spectrum() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut status = MusicStatus::new().unwrap();
        status.snapshot.error = Some("Cannot open audio output. Check system sound, then reopen the file.".into());
        status.snapshot.levels = [1.0; 16];
        status.snapshot.peaks = [1.0; 16];
        let theme = Theme::new(Themes::Light);
        let mut surface = Surface::new(20,10);
        status.on_paint(&mut surface, &theme);
        assert_eq!(row(&surface, 5), "Cannot open audio ou");
        assert_eq!(row(&surface, 6), "tput. Check system s");
        assert_eq!(row(&surface, 7), "ound, then reopen th");
        assert_eq!(row(&surface, 8), "e file.             ");
        assert_eq!(row(&surface, 9), "                    ");
        status.player.close();
        app.run();
    }
    #[test]
    fn spectrum_only_changes_refresh_and_restoration_uses_current_snapshot() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut window = MusicWindow::new().unwrap();
        let handle = window.status;
        assert!(!window.refresh());
        window.control_mut(handle).unwrap().snapshot.levels[0] = 0.5;
        assert!(matches!(TimerEvents::on_update(&mut window, 0), EventProcessStatus::Processed));
        assert_eq!(window.control(handle).unwrap().snapshot.levels, [0.0; 16]);
        window.control_mut(handle).unwrap().snapshot.peaks[15] = 1.0;
        window.set_visible(false);
        assert!(matches!(TimerEvents::on_update(&mut window, 0), EventProcessStatus::Ignored));
        window.set_visible(true);
        assert!(matches!(TimerEvents::on_update(&mut window, 0), EventProcessStatus::Processed));
        assert_eq!(window.control(handle).unwrap().snapshot.peaks, [0.0; 16]);
        assert!(!window.refresh());
        window.shutdown();
        app.run();
    }
    #[test]
    fn opening_document_paths_preserves_os_bytes_and_replaces_the_selected_track() {
        use std::os::unix::ffi::OsStringExt;
        let _ui_lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut window = MusicWindow::new().unwrap();
        let path = dir.path().join(std::ffi::OsString::from_vec(b"space \xff <FILE_PATH>.WAV".to_vec()));
        window.open(path.clone());
        let status = window.status;
        assert_eq!(window.control(status).unwrap().player.snapshot().path, Some(path.clone()));
        assert_eq!(window.control(status).unwrap().snapshot.path, Some(path));
        let next = dir.path().join("next 文.mp3");
        window.open(next.clone());
        assert_eq!(window.control(status).unwrap().player.snapshot().path, Some(next.clone()));
        assert_eq!(window.control(status).unwrap().snapshot.path, Some(next));
        assert_eq!(window.title(), "Music Player — next 文.mp3");
        window.shutdown();
        app.run();
    }
    #[test]
    fn keyboard_volume_bounds_and_hidden_errors_use_native_window_without_a_device() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let app = App::debug(80,24,"Paint.Enable(false)").timers_count(4).build().unwrap();
        let mut window = MusicWindow::new().unwrap();
        let status = window.status;
        for _ in 0..30 { OnKeyPressed::on_key_pressed(window.control_mut(status).unwrap(), Key::None, '+'); }
        assert_eq!(window.control(window.status).unwrap().player.snapshot().volume, 1.0);
        for _ in 0..30 { OnKeyPressed::on_key_pressed(window.control_mut(status).unwrap(), Key::None, '-'); }
        assert_eq!(window.control(window.status).unwrap().player.snapshot().volume, 0.0);
        window.control_mut(status).unwrap().player.open(dir.path().join("not here.mp3"));
        window.set_visible(false);
        assert!(matches!(TimerEvents::on_update(&mut window, 0), EventProcessStatus::Ignored));
        window.set_visible(true);
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while window.control(window.status).unwrap().player.snapshot().state != PlaybackState::Error {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        window.refresh();
        assert!(window.control(window.status).unwrap().player.snapshot().error.unwrap().contains("Cannot read"));
        window.shutdown(); window.shutdown();
        app.run();
    }
    #[test]
    fn status_and_errors_render_with_theme_colors_on_small_surfaces() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut status = MusicStatus::new().unwrap();
        status.snapshot.error = Some("Cannot open audio output. Check system sound, then reopen the file.".into());
        for theme in [Theme::new(Themes::Light), Theme::new(Themes::DarkGray)] {
            for (width, height) in [(1,1), (20,8), (60,12), (100,20)] {
                let mut surface = Surface::new(width, height);
                OnPaint::on_paint(&status, &mut surface, &theme);
                assert_eq!(surface.char(0,0).unwrap().code, '[');
                assert_eq!(surface.char(0,0).unwrap().foreground, theme.text.normal.foreground);
                if height > 5 { assert_eq!(surface.char(0,5).unwrap().code, 'C'); }
            }
        }
        status.player.close();
        app.run();
    }
}
