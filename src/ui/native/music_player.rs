use super::image_viewer::home_directory;
use crate::runtime::music::{MusicPlayer, PlaybackState, Snapshot};
use appcui::dialogs::{Location, OpenFileDialogFlags};
use appcui::prelude::*;
use std::time::Duration;

fn time_label(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
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
    }
}

#[Window(events = TimerEvents+WindowEvents)]
pub(crate) struct MusicWindow { status: Handle<MusicStatus>, closing: bool }
impl MusicWindow {
    pub fn new() -> anyhow::Result<Self> {
        let status = MusicStatus::new()?;
        let mut window = Self {
            base: Window::new("Music Player", layout!("a:c,w:75%,h:60%"), window::Flags::Sizeable),
            status: Handle::None, closing: false,
        };
        window.status = window.add(status);
        let status = window.status;
        if let Some(status) = window.control_mut(status) { status.request_focus(); }
        window.timer().ok_or_else(|| anyhow::anyhow!("Music Player timer unavailable"))?.start(Duration::from_millis(100));
        Ok(window)
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
