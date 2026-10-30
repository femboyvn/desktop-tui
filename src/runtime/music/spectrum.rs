use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;
use symphonia::core::dsp::{complex::Complex, fft::Fft};

pub(super) const BANDS: usize = 16;

pub(super) struct Capture { samples: VecDeque<f32>, limit: usize }
impl Capture {
    pub fn new(rate: u32, channels: usize) -> Self {
        let limit = fft_size(rate) * channels;
        Self { samples: VecDeque::with_capacity(limit), limit }
    }
    // Called only after successful nonblocking locking by the audio callback.
    // Evict before pushing: capacity never changes, even for oversized callbacks.
    pub fn push(&mut self, samples: &[f32]) {
        for &sample in samples {
            if self.samples.len() == self.limit { self.samples.pop_front(); }
            self.samples.push_back(sample);
        }
    }
    pub fn clear(&mut self) { self.samples.clear(); }
}
fn fft_size(rate: u32) -> usize { (rate as usize).div_ceil(20).next_power_of_two().min(16384) }
pub(super) struct Spectrum {
    channels: usize, fft: Fft, window: Vec<f32>, samples: Vec<f32>, work: Vec<Complex>,
    bin_bands: Vec<Option<usize>>, normalization: f32,
    levels: [f32; BANDS], peaks: [f32; BANDS],
}
impl Spectrum {
    // Track validates 8–192 kHz and one to eight channels before construction.
    pub fn new(rate: u32, channels: usize) -> Self {
        let frames = fft_size(rate);
        let window: Vec<_> = (0..frames).map(|frame| 0.5 - 0.5 * (std::f32::consts::TAU * frame as f32 / (frames - 1) as f32).cos()).collect();
        // Normalize one-sided power to a full-scale sine, correcting Hann energy.
        let normalization = 4.0 / (frames as f32 * window.iter().map(|value| value * value).sum::<f32>() * channels as f32);
        let upper = (rate as f32 / 2.0).min(16000.0);
        let log_range = (upper / 40.0).ln();
        let bin_bands = (0..=frames / 2).map(|bin| {
            let frequency = bin as f32 * rate as f32 / frames as f32;
            if frequency < 40.0 || frequency > upper { None }
            else { Some((((frequency / 40.0).ln() / log_range * BANDS as f32) as usize).min(BANDS - 1)) }
        }).collect();
        Self { channels, fft: Fft::new(frames), window, samples: vec![0.0; frames * channels], work: vec![Complex::default(); frames],
            bin_bands, normalization, levels: [0.0; BANDS], peaks: [0.0; BANDS] }
    }
    pub fn update(&mut self, capture: &Mutex<Capture>, elapsed: Duration) -> ([f32; BANDS], [f32; BANDS]) {
        self.samples.fill(0.0);
        let mut count = 0;
        if let Ok(mut capture) = capture.lock() {
            count = capture.samples.len();
            let start = self.samples.len() - count;
            for (slot, value) in self.samples[start..].iter_mut().zip(capture.samples.drain(..)) { *slot = value; }
        }
        // Copy under the capture lock, then do all FFT work after releasing it.
        // Draining prevents an underrun from repeatedly analyzing stale samples.
        let mut power = [0.0_f32; BANDS];
        if count > 0 {
            for channel in 0..self.channels {
                for (frame, value) in self.work.iter_mut().enumerate() {
                    *value = Complex::new(self.samples[frame * self.channels + channel] * self.window[frame], 0.0);
                }
                self.fft.fft_inplace(&mut self.work);
                for (value, band) in self.work.iter().zip(&self.bin_bands) {
                    if let Some(band) = band { power[*band] += value.re * value.re + value.im * value.im; }
                }
            }
        }
        let seconds = elapsed.as_secs_f32();
        for (band, power) in power.iter().enumerate() {
            let db = 10.0 * (power * self.normalization).max(1.0e-6).log10();
            let target = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
            self.levels[band] = target.max((self.levels[band] - seconds / 0.250).max(0.0));
            self.peaks[band] = self.levels[band].max((self.peaks[band] - seconds).max(0.0));
        }
        (self.levels, self.peaks)
    }
    pub fn clear(&mut self) { self.levels.fill(0.0); self.peaks.fill(0.0); self.samples.fill(0.0); }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, frames: usize, frequency: f32, amplitudes: &[f32]) -> Vec<f32> {
        (0..frames).flat_map(|frame| {
            let value = (std::f32::consts::TAU * frequency * frame as f32 / rate as f32).sin();
            amplitudes.iter().map(move |amplitude| value * amplitude)
        }).collect()
    }
    fn analyze(rate: u32, frequency: f32, amplitudes: &[f32]) -> [f32; BANDS] {
        let capture = Mutex::new(Capture::new(rate, amplitudes.len()));
        capture.lock().unwrap().push(&tone(rate, fft_size(rate), frequency, amplitudes));
        Spectrum::new(rate, amplitudes.len()).update(&capture, Duration::from_millis(100)).0
    }
    fn strongest(levels: &[f32; BANDS]) -> usize {
        levels.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0
    }
    #[test]
    fn bass_midrange_and_treble_land_in_logarithmic_bands() {
        for (frequency, band) in [(100.0, 2), (1000.0, 8), (8000.0, 14)] {
            let levels = analyze(48000, frequency, &[1.0]);
            assert_eq!(strongest(&levels), band, "{frequency}: {levels:?}");
            assert!(levels[band] > 0.98, "{levels:?}");
        }
    }
    #[test]
    fn fixed_db_scale_preserves_volume_and_silence() {
        let loud = analyze(48000, 1000.0, &[1.0])[8];
        let quiet = analyze(48000, 1000.0, &[0.1])[8];
        assert!((loud - quiet - 1.0 / 3.0).abs() < 0.005);
        assert_eq!(analyze(48000, 1000.0, &[0.0]), [0.0; BANDS]);
        assert_eq!(analyze(48000, 1000.0, &[0.00001]), [0.0; BANDS]);
    }
    #[test]
    fn independent_channel_power_keeps_opposite_phase_and_surround_visible() {
        let mono = analyze(48000, 1000.0, &[1.0]);
        let stereo = analyze(48000, 1000.0, &[1.0, -1.0]);
        assert!((mono[8] - stereo[8]).abs() < 0.001);
        let surround = analyze(48000, 8000.0, &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        assert_eq!(strongest(&surround), 14);
        assert!(surround[14] > 0.84);
    }
    #[test]
    fn sample_rate_limits_keep_resolution_and_nyquist_bounded() {
        for (rate, frames) in [(8000, 512), (44100, 4096), (48000, 4096), (192000, 16384)] {
            assert_eq!(fft_size(rate), frames);
            let levels = analyze(rate, 1000.0, &[1.0]);
            assert!(levels.iter().all(|level| level.is_finite() && (0.0..=1.0).contains(level)));
            assert!(levels.iter().any(|level| *level > 0.98));
        }
        assert!(analyze(8000, 3500.0, &[1.0])[15] > 0.98);
        assert_eq!(analyze(48000, 20000.0, &[1.0]), [0.0; BANDS]);
    }
    #[test]
    fn underruns_release_bars_in_250ms_and_peaks_in_one_second() {
        let capture = Mutex::new(Capture::new(48000, 1));
        capture.lock().unwrap().push(&tone(48000, 4096, 1000.0, &[1.0]));
        let mut spectrum = Spectrum::new(48000, 1);
        let (start, peaks) = spectrum.update(&capture, Duration::ZERO);
        assert!(start[8] > 0.98);
        assert_eq!(start, peaks);
        let (levels, peaks) = spectrum.update(&capture, Duration::from_millis(100));
        assert!((levels[8] - (start[8] - 0.4)).abs() < 0.001);
        assert!((peaks[8] - (start[8] - 0.1)).abs() < 0.001);
        let (levels, peaks) = spectrum.update(&capture, Duration::from_millis(150));
        assert_eq!(levels, [0.0; BANDS]);
        assert!((peaks[8] - (start[8] - 0.25)).abs() < 0.001);
        assert_eq!(spectrum.update(&capture, Duration::from_secs(1)).1, [0.0; BANDS]);
    }
    #[test]
    fn capture_retains_only_latest_frames_without_growing_and_clear_removes_history() {
        let mut capture = Capture::new(8000, 2);
        let capacity = capture.samples.capacity();
        capture.push(&vec![0.5; 4096]);
        capture.push(&vec![0.0; 1024]);
        assert_eq!(capture.samples.len(), 1024);
        assert_eq!(capture.samples.capacity(), capacity);
        assert!(capture.samples.iter().all(|sample| *sample == 0.0));
        let capture = Mutex::new(capture);
        let mut spectrum = Spectrum::new(8000, 2);
        capture.lock().unwrap().push(&tone(8000, 512, 1000.0, &[1.0, -1.0]));
        assert!(spectrum.update(&capture, Duration::ZERO).0.iter().any(|level| *level > 0.9));
        capture.lock().unwrap().clear();
        spectrum.clear();
        assert_eq!(spectrum.update(&capture, Duration::ZERO), ([0.0; BANDS], [0.0; BANDS]));
    }
}
