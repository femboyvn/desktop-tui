use anyhow::{Context, bail};
use symphonia::core::audio::{Channels, SampleBuffer};
use symphonia::core::codecs::{CodecParameters, Decoder, DecoderOptions};
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::thread::JoinHandle;
use std::time::Duration;

pub(crate) const MAX_AUDIO_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlaybackState { Idle, Loading, Playing, Paused, Stopped, Finished, Error, Closed }
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Snapshot {
    pub path: Option<PathBuf>, pub state: PlaybackState, pub position: f64,
    pub duration: f64, pub volume: f32, pub error: Option<String>,
    seek_revision: u64,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self { path: None, state: PlaybackState::Idle, position: 0.0, duration: 0.0, volume: 0.7, error: None, seek_revision: 0 }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Transport { Playing, Paused, Stopped }
#[derive(Clone)]
struct Request {
    revision: u64, generation: u64, path: Option<PathBuf>, transport: Transport,
    seek_revision: u64, seek_to: f64, volume: f32,
}
impl Default for Request {
    fn default() -> Self {
        Self { revision: 0, generation: 0, path: None, transport: Transport::Stopped, seek_revision: 0, seek_to: 0.0, volume: 0.7 }
    }
}
struct Shared { request: Request, snapshot: Snapshot, closed: bool, silence: Arc<AtomicBool> }
type SharedState = (Mutex<Shared>, Condvar);
type Workers = Mutex<Vec<(Weak<SharedState>, JoinHandle<()>)>>;
static WORKERS: OnceLock<Workers> = OnceLock::new();
pub(crate) struct MusicPlayer { shared: Arc<SharedState> }
impl MusicPlayer {
    pub fn new() -> std::io::Result<Self> { Self::with_factory(NativePlayer::new) }
    #[cfg(test)]
    fn with_backend(backend: impl Playback + Send + 'static) -> std::io::Result<Self> { Self::with_factory(move |_| backend) }
    fn with_factory<B: Playback + 'static>(make: impl FnOnce(Arc<AtomicBool>) -> B + Send + 'static) -> std::io::Result<Self> {
        let silence = Arc::new(AtomicBool::new(false));
        let shared = Arc::new((Mutex::new(Shared { request: Request::default(), snapshot: Snapshot::default(), closed: false, silence: silence.clone() }), Condvar::new()));
        let state = shared.clone();
        let task = std::thread::Builder::new().name("desktop-audio".into()).spawn(move || {
            let mut backend = make(silence);
            run(&state, &mut backend);
            backend.stop();
            // Release the device before reporting completion.
            drop(backend);
            if let Ok(mut shared) = state.0.lock() { shared.snapshot.state = PlaybackState::Closed; }
        })?;
        if let Ok(mut workers) = WORKERS.get_or_init(|| Mutex::new(Vec::new())).lock() {
            workers.retain(|(_, task)| !task.is_finished());
            workers.push((Arc::downgrade(&shared), task));
        }
        Ok(Self { shared })
    }
    fn change(&self, action: impl FnOnce(&mut Shared)) {
        if let Ok(mut shared) = self.shared.0.lock() {
            if shared.closed { return; }
            action(&mut shared);
            shared.snapshot.volume = shared.request.volume;
            shared.request.revision = shared.request.revision.wrapping_add(1);
            self.shared.1.notify_one();
        }
    }
    pub fn open(&self, path: PathBuf) {
        self.change(|shared| {
            shared.request.generation = shared.request.generation.wrapping_add(1);
            shared.request.path = Some(path.clone());
            shared.request.transport = Transport::Playing;
            shared.request.seek_revision = shared.request.seek_revision.wrapping_add(1);
            shared.request.seek_to = 0.0;
            shared.snapshot = Snapshot { path: Some(path), state: PlaybackState::Loading, volume: shared.request.volume, ..Snapshot::default() };
        });
    }
    pub fn toggle(&self) {
        self.change(|shared| {
            shared.request.transport = match shared.request.transport { Transport::Playing => Transport::Paused, _ => Transport::Playing };
        });
    }
    pub fn stop(&self) {
        self.change(|shared| {
            shared.request.transport = Transport::Stopped;
            shared.request.seek_to = 0.0;
            shared.request.seek_revision = shared.request.seek_revision.wrapping_add(1);
        });
    }
    pub fn seek_by(&self, seconds: f64) {
        if !seconds.is_finite() { return; }
        self.change(|shared| {
            let base = if shared.request.seek_revision != shared.snapshot.seek_revision { shared.request.seek_to } else { shared.snapshot.position };
            shared.request.seek_to = (base + seconds).clamp(0.0, (shared.snapshot.duration - 0.01).max(0.0));
            shared.request.seek_revision = shared.request.seek_revision.wrapping_add(1);
        });
    }
    pub fn adjust_volume(&self, change: f32) {
        if !change.is_finite() { return; }
        self.change(|shared| shared.request.volume = (shared.request.volume + change).clamp(0.0, 1.0));
    }
    pub fn snapshot(&self) -> Snapshot { self.shared.0.lock().map(|shared| shared.snapshot.clone()).unwrap_or_default() }
    pub fn close(&self) { close(&self.shared); }
}
impl Drop for MusicPlayer { fn drop(&mut self) { self.close(); } }
fn close(state: &SharedState) {
    if let Ok(mut shared) = state.0.lock() { shared.closed = true; shared.silence.store(true, Ordering::Release); state.1.notify_one(); }
}
/// Called after the UI loop; no UI callback waits for audio I/O or device teardown.
pub(crate) fn finish_shutdown() {
    let workers = WORKERS.get().and_then(|workers| workers.lock().ok().map(|mut workers| std::mem::take(&mut *workers))).unwrap_or_default();
    for (shared, _) in &workers { if let Some(shared) = shared.upgrade() { close(&shared); } }
    for (_, task) in workers { let _ = task.join(); }
}

trait Playback {
    fn open(&mut self, path: &Path, active: &dyn Fn() -> bool) -> anyhow::Result<()>;
    fn update(&mut self, request: &Request, active: &dyn Fn() -> bool) -> anyhow::Result<Snapshot>;
    fn stop(&mut self);
}
fn run(state: &SharedState, backend: &mut impl Playback) {
    let mut seen_revision = u64::MAX;
    let mut generation = 0;
    let mut failure = None;
    loop {
        let request = {
            let Ok(mut shared) = state.0.lock() else { return; };
            if !shared.closed && shared.request.revision == seen_revision {
                let Ok((next, _)) = state.1.wait_timeout(shared, Duration::from_millis(100)) else { return; };
                shared = next;
            }
            if shared.closed { return; }
            shared.request.clone()
        };
        seen_revision = request.revision;
        if request.path.is_none() { continue; }
        if request.generation != generation {
            backend.stop();
            let active = || state.0.lock().is_ok_and(|shared| !shared.closed && shared.request.generation == request.generation);
            failure = backend.open(request.path.as_ref().unwrap(), &active).err().map(|error| format!("{error:#}"));
            generation = request.generation;
        }
        // Never start an obsolete track, including closure during a slow open.
        let current = match state.0.lock() { Ok(shared) if !shared.closed => shared.request.clone(), _ => return };
        if current.generation != generation { continue; }
        let mut snapshot = if let Some(error) = &failure {
            Snapshot { state: PlaybackState::Error, error: Some(error.clone()), ..Snapshot::default() }
        } else {
            let active = || state.0.lock().is_ok_and(|shared| !shared.closed && shared.request.revision == current.revision);
            match backend.update(&current, &active) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    backend.stop();
                    let error = format!("{error:#}");
                    failure = Some(error.clone());
                    Snapshot { state: PlaybackState::Error, error: Some(error), ..Snapshot::default() }
                }
            }
        };
        snapshot.path = current.path.clone();
        snapshot.volume = current.volume;
        snapshot.seek_revision = current.seek_revision;
        if let Ok(mut shared) = state.0.lock()
            && !shared.closed && shared.request.revision == current.revision {
            if snapshot.state == PlaybackState::Finished {
                shared.request.transport = Transport::Stopped;
                shared.request.seek_to = 0.0;
                shared.request.seek_revision = shared.request.seek_revision.wrapping_add(1);
            }
            shared.snapshot = snapshot;
        }
    }
}

struct Track {
    format: Box<dyn FormatReader>, decoder: Box<dyn Decoder>, params: CodecParameters,
    id: u32, rate: u32, channels: usize, channel_map: Vec<u8>, duration: f64, skip_until: f64, eof: bool,
}
fn channel_map(channels: Channels) -> anyhow::Result<Vec<u8>> {
    if !(1..=8).contains(&channels.count()) { bail!("Audio must have between one and eight channels."); }
    // Symphonia's standard mono layout is FRONT_LEFT. Map ordinary mono to
    // miniaudio's MONO position so both front speakers receive it.
    if channels == Channels::FRONT_LEFT || channels == Channels::FRONT_CENTRE { return Ok(vec![1]); }
    if channels.bits() & !0x3ffff != 0 { bail!("This audio channel layout is unsupported."); }
    // In the pinned miniaudio header, positions FRONT_LEFT through TOP_BACK_RIGHT
    // are IDs 2..19 in the same order as Symphonia's first eighteen mask bits.
    Ok(channels.iter().map(|channel| channel.bits().trailing_zeros() as u8 + 2).collect())
}
impl Track {
    fn seconds(&self, timestamp: u64) -> f64 {
        if let Some(base) = self.params.time_base {
            let time = base.calc_time(timestamp);
            time.seconds as f64 + time.frac
        } else { timestamp as f64 / f64::from(self.rate) }
    }
    #[cfg(test)]
    fn open(path: &Path) -> anyhow::Result<Self> { Self::open_cancellable(path, &|| true) }
    fn open_cancellable(path: &Path, active: &dyn Fn() -> bool) -> anyhow::Result<Self> {
        let file = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(path)
            .with_context(|| format!("Cannot read {}", path.display()))?;
        let metadata = file.metadata()?;
        if !metadata.is_file() { bail!("Choose a regular MP3, WAV, FLAC, or Ogg Vorbis file."); }
        if metadata.len() > MAX_AUDIO_BYTES { bail!("Audio files must be at most 128 MiB."); }
        let mut encoded = Vec::with_capacity(metadata.len() as usize);
        let mut file = file.take(MAX_AUDIO_BYTES + 1);
        let mut chunk = [0; 64 * 1024];
        loop {
            if !active() { bail!("Audio loading cancelled."); }
            let count = file.read(&mut chunk).context("Cannot read audio file")?;
            if count == 0 { break; }
            encoded.extend_from_slice(&chunk[..count]);
        }
        if encoded.len() as u64 > MAX_AUDIO_BYTES { bail!("Audio files must be at most 128 MiB."); }
        let source = MediaSourceStream::new(Box::new(std::io::Cursor::new(encoded)), Default::default());
        let options = FormatOptions { prebuild_seek_index: true, seek_index_fill_rate: 1, ..FormatOptions::default() };
        let format = symphonia::default::get_probe().format(&Hint::new(), source, &options, &Default::default())
            .context("Cannot decode audio; choose MP3, WAV, FLAC, or Ogg Vorbis")?.format;
        let selected = format.default_track().ok_or_else(|| anyhow::anyhow!("Audio contains no playable track"))?;
        let id = selected.id;
        let params = selected.codec_params.clone();
        let rate = params.sample_rate.ok_or_else(|| anyhow::anyhow!("Audio sample rate is missing"))?;
        let layout = params.channels.ok_or_else(|| anyhow::anyhow!("Audio channel count is missing"))?;
        let channel_map = channel_map(layout)?;
        let channels = layout.count();
        if !(8000..=192000).contains(&rate) { bail!("Audio sample rate must be between 8 and 192 kHz."); }
        let decoder = symphonia::default::get_codecs().make(&params, &DecoderOptions::default()).context("Cannot decode this audio format")?;
        let mut track = Self { format, decoder, params, id, rate, channels, channel_map, duration: 0.0, skip_until: 0.0, eof: false };
        let frames = if let Some(frames) = track.params.n_frames { frames } else {
            // Local input is bounded. Index packet timestamps off the UI thread
            // when a file (e.g. MP3 without Xing metadata) omits its duration.
            let mut end = 0;
            loop {
                if !active() { bail!("Audio loading cancelled."); }
                match track.format.next_packet() {
                    Ok(packet) if packet.track_id() == id => end = end.max(packet.ts.saturating_add(packet.dur)),
                    Ok(_) => {},
                    Err(symphonia::core::errors::Error::IoError(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
                    Err(error) => return Err(error).context("Cannot index audio duration"),
                }
            }
            end
        };
        track.duration = track.seconds(frames);
        if !track.duration.is_finite() || track.duration <= 0.0 { bail!("Audio contains no playable samples."); }
        track.seek(0.0)?;
        Ok(track)
    }
    fn seek(&mut self, seconds: f64) -> anyhow::Result<()> {
        let seconds = seconds.clamp(0.0, self.duration);
        // Decode a short pre-roll after resetting codec state (notably MP3's
        // bit reservoir), then discard samples before the requested timestamp.
        let from = (seconds - 1.0).max(0.0);
        self.format.seek(SeekMode::Accurate, SeekTo::Time { time: Time::new(from as u64, from.fract()), track_id: Some(self.id) })
            .context("Cannot seek in this audio file")?;
        self.decoder.reset();
        self.skip_until = seconds;
        self.eof = false;
        Ok(())
    }
    fn next_samples(&mut self) -> anyhow::Result<Option<Vec<f32>>> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(symphonia::core::errors::Error::IoError(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => { self.eof = true; return Ok(None); }
                Err(error) => return Err(error).context("Cannot read audio samples"),
            };
            if packet.track_id() != self.id { continue; }
            let start = self.seconds(packet.ts);
            let decoded = self.decoder.decode(&packet).context("Cannot decode audio samples")?;
            if decoded.spec().rate != self.rate || Some(decoded.spec().channels) != self.params.channels { bail!("Audio format changes inside the file are unsupported."); }
            if decoded.capacity() > 131072 { bail!("Audio packet exceeds the playback buffer limit."); }
            let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
            buffer.copy_interleaved_ref(decoded);
            let skip = ((self.skip_until - start).max(0.0) * f64::from(self.rate)).round() as usize;
            let skip = skip.saturating_mul(self.channels).min(buffer.samples().len());
            if skip == buffer.samples().len() { continue; }
            return Ok(Some(buffer.samples()[skip..].to_vec()));
        }
    }
}

struct Output {
    queue: Mutex<VecDeque<f32>>, playing: AtomicBool, closed: Arc<AtomicBool>,
    played: AtomicU64, volume: AtomicU32, channels: usize,
}
impl Output {
    fn new(channels: usize, closed: Arc<AtomicBool>) -> Self {
        Self { queue: Mutex::new(VecDeque::new()), playing: AtomicBool::new(false), closed,
            played: AtomicU64::new(0), volume: AtomicU32::new(0.7_f32.to_bits()), channels }
    }
    fn render(&self, output: &mut [f32]) {
        output.fill(0.0);
        if self.closed.load(Ordering::Acquire) || !self.playing.load(Ordering::Acquire) { return; }
        // A real-time callback never waits for the decoder/seek worker.
        let Ok(mut queue) = self.queue.try_lock() else { return; };
        let volume = f32::from_bits(self.volume.load(Ordering::Relaxed));
        let mut count = 0;
        for sample in output {
            let Some(value) = queue.pop_front() else { break; };
            *sample = if value.is_finite() { (value * volume).clamp(-1.0, 1.0) } else { 0.0 };
            count += 1;
        }
        self.played.fetch_add((count / self.channels) as u64, Ordering::Relaxed);
    }
}
unsafe extern "C" {
    fn desktop_audio_open(rate: u32, channels: u32, channel_map: *const u8, render: unsafe extern "C" fn(*mut std::ffi::c_void, *mut f32, u32), user: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn desktop_audio_close(handle: *mut std::ffi::c_void);
}
unsafe extern "C" fn render_audio(user: *mut std::ffi::c_void, output: *mut f32, frames: u32) {
    if user.is_null() || output.is_null() { return; }
    // C retains this boxed Arc until device uninitialization has joined its
    // callback thread. The configured device writes interleaved f32 frames.
    let state = unsafe { &*user.cast::<Arc<Output>>() };
    let Some(count) = (frames as usize).checked_mul(state.channels) else { return; };
    state.render(unsafe { std::slice::from_raw_parts_mut(output, count) });
}
struct NativeDevice { handle: *mut std::ffi::c_void, user: *mut Arc<Output> }
impl NativeDevice {
    fn open(rate: u32, channel_map: &[u8], output: Arc<Output>) -> anyhow::Result<Self> {
        let user = Box::into_raw(Box::new(output));
        let handle = unsafe { desktop_audio_open(rate, channel_map.len() as u32, channel_map.as_ptr(), render_audio, user.cast()) };
        if handle.is_null() {
            unsafe { drop(Box::from_raw(user)); }
            bail!("Cannot open audio output. Check the system sound device and audio session, then reopen the file.");
        }
        Ok(Self { handle, user })
    }
}
impl Drop for NativeDevice {
    fn drop(&mut self) {
        // Close joins native callbacks before freeing the Rust callback owner.
        unsafe { desktop_audio_close(self.handle); drop(Box::from_raw(self.user)); }
    }
}
struct NativePlayer {
    track: Option<Track>, device: Option<NativeDevice>, output: Option<Arc<Output>>,
    closed: Arc<AtomicBool>, seek_revision: u64, base_position: f64,
}
impl NativePlayer {
    fn new(closed: Arc<AtomicBool>) -> Self { Self { track: None, device: None, output: None, closed, seek_revision: u64::MAX, base_position: 0.0 } }
}
impl Playback for NativePlayer {
    fn open(&mut self, path: &Path, active: &dyn Fn() -> bool) -> anyhow::Result<()> {
        self.stop();
        self.track = None;
        let track = Track::open_cancellable(path, active)?;
        self.output = Some(Arc::new(Output::new(track.channels, self.closed.clone())));
        self.track = Some(track);
        Ok(())
    }
    fn update(&mut self, request: &Request, active: &dyn Fn() -> bool) -> anyhow::Result<Snapshot> {
        let duration = self.track.as_ref().map_or(0.0, |track| track.duration);
        if request.transport == Transport::Stopped {
            self.stop();
            return Ok(Snapshot { state: PlaybackState::Stopped, duration, ..Snapshot::default() });
        }
        let track = self.track.as_mut().ok_or_else(|| anyhow::anyhow!("Open an audio file first."))?;
        let output = self.output.as_ref().unwrap();
        output.volume.store(request.volume.to_bits(), Ordering::Relaxed);
        if self.seek_revision != request.seek_revision {
            output.playing.store(false, Ordering::Release);
            output.queue.lock().map_err(|_| anyhow::anyhow!("Audio buffer unavailable"))?.clear();
            output.played.store(0, Ordering::Relaxed);
            track.seek(request.seek_to)?;
            self.base_position = request.seek_to;
            self.seek_revision = request.seek_revision;
        }
        let capacity = track.rate as usize * track.channels / 2;
        while !track.eof && active() {
            if output.queue.lock().map_err(|_| anyhow::anyhow!("Audio buffer unavailable"))?.len() >= capacity { break; }
            let Some(samples) = track.next_samples()? else { break; };
            output.queue.lock().map_err(|_| anyhow::anyhow!("Audio buffer unavailable"))?.extend(samples);
        }
        // A new control revision only invalidates publication; it must not
        // reset decoded/played position. The worker stops on close/new source.
        if !active() { return Ok(Snapshot::default()); }
        if track.eof && output.queue.lock().map_err(|_| anyhow::anyhow!("Audio buffer unavailable"))?.is_empty() {
            self.stop();
            return Ok(Snapshot { state: PlaybackState::Finished, position: duration, duration, ..Snapshot::default() });
        }
        if self.device.is_none() && request.transport == Transport::Playing {
            self.device = Some(NativeDevice::open(track.rate, &track.channel_map, output.clone())?);
        }
        output.playing.store(request.transport == Transport::Playing, Ordering::Release);
        let position = (self.base_position + output.played.load(Ordering::Relaxed) as f64 / f64::from(track.rate)).min(duration);
        Ok(Snapshot { state: if request.transport == Transport::Paused { PlaybackState::Paused } else { PlaybackState::Playing }, position, duration, ..Snapshot::default() })
    }
    fn stop(&mut self) {
        if let Some(output) = &self.output {
            output.playing.store(false, Ordering::Release);
            self.device = None;
            if let Ok(mut queue) = output.queue.lock() { queue.clear(); }
            output.played.store(0, Ordering::Relaxed);
        }
        self.seek_revision = u64::MAX;
        self.base_position = 0.0;
    }
}
impl Drop for NativePlayer { fn drop(&mut self) { self.stop(); } }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use std::sync::mpsc;
    use std::time::Instant;

    struct TestPlayback { started: Option<mpsc::Sender<()>>, release: Option<mpsc::Receiver<()>>, plays: Arc<Mutex<usize>> }
    impl Playback for TestPlayback {
        fn open(&mut self, _: &Path, _: &dyn Fn() -> bool) -> anyhow::Result<()> {
            if let Some(started) = self.started.take() { started.send(()).unwrap(); }
            if let Some(release) = self.release.take() { release.recv().unwrap(); }
            Ok(())
        }
        fn update(&mut self, request: &Request, _: &dyn Fn() -> bool) -> anyhow::Result<Snapshot> {
            *self.plays.lock().unwrap() += 1;
            Ok(Snapshot { state: match request.transport { Transport::Playing => PlaybackState::Playing, Transport::Paused => PlaybackState::Paused, Transport::Stopped => PlaybackState::Stopped }, duration: 120.0, position: request.seek_to, ..Snapshot::default() })
        }
        fn stop(&mut self) {}
    }
    fn wait(player: &MusicPlayer, predicate: impl Fn(&Snapshot) -> bool) -> Snapshot {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let snapshot = player.snapshot();
            if predicate(&snapshot) { return snapshot; }
            assert!(Instant::now() < deadline, "last state: {snapshot:?}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn test_player() -> MusicPlayer { MusicPlayer::with_backend(TestPlayback { started: None, release: None, plays: Arc::default() }).unwrap() }
    #[test]
    fn controls_coalesce_and_clamp_seek_and_volume() {
        let player = test_player();
        player.open(PathBuf::from("a song.mp3"));
        wait(&player, |snapshot| snapshot.state == PlaybackState::Playing);
        player.toggle();
        wait(&player, |snapshot| snapshot.state == PlaybackState::Paused);
        for _ in 0..30 { player.seek_by(10.0); player.adjust_volume(0.05); }
        let snapshot = wait(&player, |snapshot| snapshot.position == 119.99 && snapshot.volume == 1.0);
        assert_eq!(snapshot.state, PlaybackState::Paused);
        for _ in 0..30 { player.seek_by(-10.0); player.adjust_volume(-0.05); }
        wait(&player, |snapshot| snapshot.position == 0.0 && snapshot.volume == 0.0);
        player.toggle();
        wait(&player, |snapshot| snapshot.state == PlaybackState::Playing);
        player.stop();
        wait(&player, |snapshot| snapshot.state == PlaybackState::Stopped);
        player.close(); player.close();
        wait(&player, |snapshot| snapshot.state == PlaybackState::Closed);
    }
    #[test]
    fn close_during_load_cannot_start_playback_or_update_a_reopened_player() {
        let (started, loading) = mpsc::channel(); let (release, blocked) = mpsc::channel();
        let plays = Arc::new(Mutex::new(0));
        let old = MusicPlayer::with_backend(TestPlayback { started: Some(started), release: Some(blocked), plays: plays.clone() }).unwrap();
        old.open(PathBuf::from("old.mp3"));
        loading.recv_timeout(Duration::from_secs(3)).unwrap();
        old.close();
        let new = test_player();
        new.open(PathBuf::from("new.mp3"));
        wait(&new, |snapshot| snapshot.state == PlaybackState::Playing);
        release.send(()).unwrap();
        wait(&old, |snapshot| snapshot.state == PlaybackState::Closed);
        assert_eq!(*plays.lock().unwrap(), 0);
        assert_eq!(new.snapshot().path, Some(PathBuf::from("new.mp3")));
    }
    #[test]
    fn invalid_and_oversized_files_report_clear_errors_without_audio_hardware() {
        let dir = TestDir::new();
        let missing = Track::open(&dir.path().join("missing.mp3")).err().unwrap();
        assert!(format!("{missing:#}").contains("Cannot read"));
        let bad = dir.path().join("corrupt.mp3");
        std::fs::write(&bad, b"this is not audio").unwrap();
        assert!(format!("{:#}", Track::open(&bad).err().unwrap()).contains("Cannot decode"));
        let huge = dir.path().join("huge.wav");
        std::fs::File::create(&huge).unwrap().set_len(MAX_AUDIO_BYTES + 1).unwrap();
        assert!(Track::open(&huge).err().unwrap().to_string().contains("128 MiB"));
        assert!(Track::open(dir.path()).err().unwrap().to_string().contains("regular"));
        let mut channels = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio/tone.wav")).unwrap();
        channels[22..24].copy_from_slice(&9_u16.to_le_bytes());
        std::fs::write(&bad, channels).unwrap();
        assert!(Track::open(&bad).err().unwrap().to_string().contains("eight channels"));
    }
    #[test]
    fn bundled_formats_decode_and_preserve_opaque_paths() {
        use std::os::unix::ffi::OsStringExt;
        let dir = TestDir::new();
        for extension in ["mp3", "wav", "flac", "ogg"] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/audio/tone.{extension}"));
            let mut track = Track::open(&path).unwrap();
            assert!(track.duration >= 0.19 && track.duration < 0.5, "{extension}: {}", track.duration);
            for seconds in [0.0, 0.1, 0.0, 0.19] {
                track.seek(seconds).unwrap();
                let samples = track.next_samples().unwrap().expect("seek must leave playable samples");
                assert!(samples.iter().any(|sample| sample.abs() > 0.0001), "{extension}: seek {seconds}");
            }
        }
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio/tone.wav");
        for name in [std::ffi::OsString::from("space 文 <FILE_PATH>.wav"), std::ffi::OsString::from_vec(vec![b'a', 0xff, b'.', b'w', b'a', b'v'])] {
            let path = dir.path().join(name);
            std::fs::copy(&fixture, &path).unwrap();
            assert!(Track::open(&path).unwrap().duration > 0.19);
        }
    }
    #[test]
    fn audio_callback_applies_volume_pause_and_immediate_close_silence() {
        let closed = Arc::new(AtomicBool::new(false));
        let output = Output::new(2, closed.clone());
        output.queue.lock().unwrap().extend([0.2, -0.4, 0.6, -0.8]);
        output.playing.store(true, Ordering::Release);
        output.volume.store(0.5_f32.to_bits(), Ordering::Relaxed);
        let mut samples = [0.0; 4];
        output.render(&mut samples);
        assert_eq!(samples, [0.1, -0.2, 0.3, -0.4]);
        assert_eq!(output.played.load(Ordering::Relaxed), 2);
        output.queue.lock().unwrap().extend([0.2, -0.4, 0.6, -0.8]);
        output.playing.store(false, Ordering::Release);
        output.render(&mut samples);
        assert_eq!(samples, [0.0; 4]);
        assert_eq!(output.queue.lock().unwrap().len(), 4);
        output.playing.store(true, Ordering::Release);
        closed.store(true, Ordering::Release);
        // Even a stalled decoder/seek worker cannot keep producing sound after close.
        let output = Arc::new(output);
        unsafe { render_audio((&output as *const Arc<Output>).cast_mut().cast(), samples.as_mut_ptr(), 2); }
        assert_eq!(samples, [0.0; 4]);
    }
    #[test]
    fn obsolete_file_reads_cancel_before_decoding() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio/tone.wav");
        let calls = AtomicU32::new(0);
        let result = Track::open_cancellable(&path, &|| calls.fetch_add(1, Ordering::Relaxed) == 0);
        assert!(result.err().unwrap().to_string().contains("cancelled"));
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }
    #[test]
    fn a_control_revision_during_refill_preserves_playback_position() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio/tone.wav");
        let mut player = NativePlayer::new(Arc::new(AtomicBool::new(false)));
        player.open(&path, &|| true).unwrap();
        let mut request = Request { path: Some(path), transport: Transport::Paused, seek_revision: 1, ..Request::default() };
        player.update(&request, &|| true).unwrap();
        let output = player.output.as_ref().unwrap();
        output.playing.store(true, Ordering::Release);
        output.render(&mut vec![0.0; 2205]);
        output.playing.store(false, Ordering::Release);
        let before = player.update(&request, &|| true).unwrap().position;
        assert!(before > 0.0);
        // A volume/pause command arriving during refill invalidates publication,
        // but must not discard already played frames or seek the decoder to zero.
        player.update(&request, &|| false).unwrap();
        request.volume = 0.75;
        let after = player.update(&request, &|| true).unwrap().position;
        assert_eq!(after, before);
    }
    #[test]
    fn output_channel_maps_preserve_quad_lfe_and_surround_positions() {
        assert_eq!(channel_map(Channels::FRONT_LEFT).unwrap(), [1]);
        let stereo = Channels::FRONT_LEFT | Channels::FRONT_RIGHT;
        assert_eq!(channel_map(stereo).unwrap(), [2, 3]);
        assert_eq!(channel_map(stereo | Channels::LFE1).unwrap(), [2, 3, 5]);
        let quad = stereo | Channels::REAR_LEFT | Channels::REAR_RIGHT;
        assert_eq!(channel_map(quad).unwrap(), [2, 3, 6, 7]);
        let surround = quad | Channels::FRONT_CENTRE | Channels::LFE1 | Channels::SIDE_LEFT | Channels::SIDE_RIGHT;
        assert_eq!(channel_map(surround).unwrap(), [2, 3, 4, 5, 6, 7, 11, 12]);
        assert!(channel_map(Channels::REAR_LEFT_CENTRE).is_err());
    }
}
