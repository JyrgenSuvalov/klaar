/// Spectrum analyzer for real-time frequency visualization.
///
/// Architecture (writer/reader split):
/// - `SpectrumWriter` (audio thread): lock-free, zero-allocation atomic writes via `push_samples()`
/// - `SpectrumReader` (IPC thread): snapshots ring buffer, runs FFT, returns smoothed dB values
/// - Both share a `SpectrumRingBuffer` via `Arc` — no `unsafe impl` needed
///
/// Created together via `create_spectrum_pair(sample_rate)`.
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;

use realfft::RealFftPlanner;
use realfft::RealToComplex;
use rustfft::num_complex::Complex;

// ────────────────────────────────────────────────────────────────────────────
// Constants
// ────────────────────────────────────────────────────────────────────────────

pub const FFT_SIZE: usize = 4096;
pub const SPECTRUM_BINS: usize = 256;
pub const FREQ_MIN: f32 = 20.0;
pub const FREQ_MAX: f32 = 20_000.0;
/// Minimum dB value for spectrum display. Raised from -90 to -80 to reduce
/// visual noise from interface/quantization noise floor, keeping the display
/// clean during silence while retaining useful dynamic range for speech.
pub const DB_FLOOR: f32 = -80.0;
pub const DB_CEILING: f32 = 0.0;
/// Smoothing attack coefficient — how fast the spectrum rises to follow a new peak.
/// Tuned for FabFilter-like fluidity: fast enough to track speech formants
/// (~300 Hz, ~2 kHz, ~3 kHz) without jittery frame-to-frame flicker.
pub const SMOOTHING_ATTACK: f32 = 0.45;

/// Smoothing decay coefficient — how fast the spectrum falls after a peak.
/// At 30 fps polling, 0.08 gives ~1.3s full decay — responsive enough to
/// follow speech pauses without sluggish lingering.
pub const SMOOTHING_DECAY: f32 = 0.08;

// ────────────────────────────────────────────────────────────────────────────
// BinRange — maps a display band to a range of FFT bins
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct BinRange {
    start: usize, // inclusive
    end: usize,   // exclusive
}

// ────────────────────────────────────────────────────────────────────────────
// SpectrumRingBuffer — shared between writer and reader via Arc
// ────────────────────────────────────────────────────────────────────────────

/// Lock-free ring buffer shared between the audio thread (writer) and the
/// IPC thread (reader). All fields are atomic — inherently `Send + Sync`.
pub struct SpectrumRingBuffer {
    /// Per-sample atomic storage. f32 values are stored as u32 via `to_bits()`.
    sample_buffer: Box<[AtomicU32]>,
    /// Monotonically increasing write cursor (wraps via modulo).
    write_pos: AtomicUsize,
    /// Current sample rate, written by `update_sample_rate`, read by `get_spectrum`.
    sample_rate: AtomicU32,
    /// Flag: bin mapping needs recomputation after sample rate change.
    needs_recompute: AtomicBool,
}

impl SpectrumRingBuffer {
    fn new(sample_rate: u32) -> Self {
        let sample_buffer: Box<[AtomicU32]> = (0..FFT_SIZE)
            .map(|_| AtomicU32::new(0u32))
            .collect::<Vec<_>>()
            .into_boxed_slice();

        Self {
            sample_buffer,
            write_pos: AtomicUsize::new(0),
            sample_rate: AtomicU32::new(sample_rate),
            needs_recompute: AtomicBool::new(false),
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// SpectrumWriter — audio thread half (Send + Sync, &self only)
// ────────────────────────────────────────────────────────────────────────────

/// Writer half of the spectrum analyzer, used on the audio thread.
///
/// Only touches atomic fields in the shared ring buffer — no allocations,
/// no locks, no syscalls. Automatically `Send + Sync` (no `unsafe impl`).
pub struct SpectrumWriter {
    ring: Arc<SpectrumRingBuffer>,
}

// Compile-time assertion: SpectrumWriter must be Send + Sync without unsafe.
#[allow(dead_code)]
const _: () = {
    fn assert_send_sync<T: Send + Sync>() {}
    fn check() {
        assert_send_sync::<SpectrumWriter>();
    }
};

impl SpectrumWriter {
    /// Write audio samples into the ring buffer.
    ///
    /// # Real-time safety
    /// This method performs ONLY atomic stores — no allocation, no lock, no syscall.
    /// Safe to call from the CoreAudio real-time callback thread.
    #[inline]
    pub fn push_samples(&self, samples: &[f32]) {
        let mut pos = self.ring.write_pos.load(Ordering::Relaxed);
        for &sample in samples {
            let idx = pos % FFT_SIZE;
            self.ring.sample_buffer[idx].store(sample.to_bits(), Ordering::Relaxed);
            pos = pos.wrapping_add(1);
        }
        // Release ordering ensures all sample stores are visible before the cursor update
        self.ring.write_pos.store(pos, Ordering::Release);
    }

    /// Update the sample rate. The bin mapping will be recomputed on the next
    /// `get_spectrum()` call on the reader side.
    ///
    /// Can be called from any thread (uses atomics).
    #[allow(dead_code)] // Called during engine device-change restart (not yet wired)
    pub fn update_sample_rate(&self, rate: u32) {
        self.ring.sample_rate.store(rate, Ordering::Relaxed);
        // Release ordering: reader loads this with Acquire, guaranteeing it sees
        // the new sample_rate value before acting on the flag.
        self.ring.needs_recompute.store(true, Ordering::Release);
    }
}

// ────────────────────────────────────────────────────────────────────────────
// SpectrumReader — IPC thread half (&mut self, owns FFT state)
// ────────────────────────────────────────────────────────────────────────────

/// Reader half of the spectrum analyzer, used on the IPC thread.
///
/// Owns all mutable FFT state (plan, window, scratch buffers, smoothed bins).
/// Takes `&mut self` for `get_spectrum` — only one thread may call it at a time.
/// Stored behind a `Mutex` in `EngineHandle` (never touched by the audio thread).
pub struct SpectrumReader {
    ring: Arc<SpectrumRingBuffer>,

    // FFT state
    fft: Arc<dyn RealToComplex<f32>>,
    window: Box<[f32; FFT_SIZE]>,
    /// Dual-purpose buffer: used as FFT input during `process_with_scratch`,
    /// then reused as scratch for dB magnitude values in `get_spectrum`.
    /// Do NOT read this as sample data after the FFT has run.
    fft_input: Vec<f32>,
    fft_output: Vec<Complex<f32>>,
    fft_scratch: Vec<Complex<f32>>,

    // Post-processing state
    smoothed_bins: Vec<f32>,
    bin_mapping: Vec<BinRange>,
}

impl SpectrumReader {
    /// Compute and return the current spectrum as 256 smoothed dB values.
    ///
    /// This method:
    /// 1. Snapshots the ring buffer (Acquire on cursor)
    /// 2. Applies the pre-computed Hann window
    /// 3. Executes a 4096-point real FFT
    /// 4. Converts magnitudes to dB, clamps to [DB_FLOOR, 0]
    /// 5. Resamples to 256 log-frequency bands (max-pick)
    /// 6. Applies asymmetric exponential smoothing
    ///
    /// # Thread safety
    /// Takes `&mut self` — must be called from a single thread (the IPC thread).
    pub fn get_spectrum(&mut self) -> &[f32] {
        // Check if bin mapping needs recomputation (sample rate changed)
        // Acquire ordering: pairs with Release in update_sample_rate to ensure
        // the new sample_rate value is visible when we see the flag as true.
        if self.ring.needs_recompute.load(Ordering::Acquire) {
            let rate = self.ring.sample_rate.load(Ordering::Relaxed);
            self.bin_mapping = compute_bin_mapping(rate);
            self.ring.needs_recompute.store(false, Ordering::Relaxed);
        }

        // 1. Snapshot ring buffer
        let write_pos = self.ring.write_pos.load(Ordering::Acquire);
        for i in 0..FFT_SIZE {
            // Read the most recent FFT_SIZE samples, oldest first
            let idx = (write_pos.wrapping_sub(FFT_SIZE).wrapping_add(i)) % FFT_SIZE;
            let bits = self.ring.sample_buffer[idx].load(Ordering::Relaxed);
            let sample = f32::from_bits(bits);
            // Sanitize: clamp NaN/Inf to zero
            self.fft_input[i] = if sample.is_finite() { sample } else { 0.0 };
        }

        // 2. Apply Hann window
        for i in 0..FFT_SIZE {
            self.fft_input[i] *= self.window[i];
        }

        // 3. Execute real FFT
        if let Err(e) = self.fft.process_with_scratch(
            &mut self.fft_input,
            &mut self.fft_output,
            &mut self.fft_scratch,
        ) {
            // FFT failed — return the previous smoothed bins rather than garbage
            log::error!("FFT processing failed: {}", e);
            return &self.smoothed_bins;
        }

        // 4. Compute magnitudes in dB
        // Skip sqrt: 20*log10(sqrt(x)) == 10*log10(x)
        let norm_sq = ((FFT_SIZE / 2) as f32) * ((FFT_SIZE / 2) as f32);
        let num_bins = self.fft_output.len(); // FFT_SIZE/2 + 1 = 2049

        // Reuse fft_input as scratch for dB values (already consumed by FFT).
        // NOTE: After this loop, fft_input[0..num_bins] contains dB values, NOT audio samples.
        for i in 0..num_bins {
            let c = self.fft_output[i];
            let mag_sq = (c.re * c.re + c.im * c.im) / norm_sq;
            let db = 10.0 * (mag_sq + 1e-20).log10();
            self.fft_input[i] = db.clamp(DB_FLOOR, DB_CEILING);
        }

        // 5. Log-frequency resample: max-pick per display band
        for band in 0..SPECTRUM_BINS {
            let range = &self.bin_mapping[band];
            let start = range.start.min(num_bins);
            let end = range.end.min(num_bins);

            let mut max_db = DB_FLOOR;
            for bin in start..end {
                if self.fft_input[bin] > max_db {
                    max_db = self.fft_input[bin];
                }
            }

            // 6. Asymmetric exponential smoothing
            let current = self.smoothed_bins[band];
            if max_db > current {
                self.smoothed_bins[band] = current + SMOOTHING_ATTACK * (max_db - current);
            } else {
                self.smoothed_bins[band] = current + SMOOTHING_DECAY * (max_db - current);
            }
        }

        &self.smoothed_bins
    }

    /// Reset all smoothed bins to DB_FLOOR and zero the ring buffer.
    ///
    /// Call this when the audio engine restarts (e.g., device change) to avoid
    /// stale spectrum data lingering on screen. Does NOT recompute the FFT plan
    /// or Hann window.
    ///
    /// Safe to call only when the writer is no longer active.
    #[allow(dead_code)] // Called during engine device-change restart (not yet wired)
    pub fn reset(&mut self) {
        for bin in &mut self.smoothed_bins {
            *bin = DB_FLOOR;
        }
        for sample in self.ring.sample_buffer.iter() {
            sample.store(0u32, Ordering::Relaxed);
        }
        self.ring.write_pos.store(0, Ordering::Relaxed);
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Factory function
// ────────────────────────────────────────────────────────────────────────────

/// Create a matched writer/reader pair sharing a ring buffer.
///
/// The writer goes to the audio thread; the reader goes to the IPC thread.
pub fn create_spectrum_pair(sample_rate: u32) -> (SpectrumWriter, SpectrumReader) {
    let ring = Arc::new(SpectrumRingBuffer::new(sample_rate));

    // Pre-compute Hann window
    let window = {
        let mut w = [0.0f32; FFT_SIZE];
        for (n, w) in w.iter_mut().enumerate() {
            *w = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * n as f32 / FFT_SIZE as f32).cos());
        }
        Box::new(w)
    };

    // Create FFT plan
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);

    // Allocate scratch buffers
    let fft_input = fft.make_input_vec();
    let fft_output = fft.make_output_vec();
    let fft_scratch = fft.make_scratch_vec();

    // Compute bin mapping
    let bin_mapping = compute_bin_mapping(sample_rate);

    // Initialize smoothed bins to floor
    let smoothed_bins = vec![DB_FLOOR; SPECTRUM_BINS];

    let writer = SpectrumWriter {
        ring: Arc::clone(&ring),
    };

    let reader = SpectrumReader {
        ring,
        fft,
        window,
        fft_input,
        fft_output,
        fft_scratch,
        smoothed_bins,
        bin_mapping,
    };

    (writer, reader)
}

// ────────────────────────────────────────────────────────────────────────────
// Bin mapping computation
// ────────────────────────────────────────────────────────────────────────────

/// Compute the log-frequency bin mapping table.
///
/// Maps 256 display bands (log-spaced from 20 Hz to 20 kHz) to ranges of FFT bins.
/// At low frequencies, multiple display bands may map to the same FFT bin — this is
/// expected given the FFT's frequency resolution.
fn compute_bin_mapping(sample_rate: u32) -> Vec<BinRange> {
    let sr = sample_rate as f32;
    let ratio = FREQ_MAX / FREQ_MIN;
    let max_bin = FFT_SIZE / 2; // Nyquist bin index

    (0..SPECTRUM_BINS)
        .map(|i| {
            let freq_lo = FREQ_MIN * ratio.powf(i as f32 / SPECTRUM_BINS as f32);
            let freq_hi = FREQ_MIN * ratio.powf((i + 1) as f32 / SPECTRUM_BINS as f32);

            let bin_start = (freq_lo * FFT_SIZE as f32 / sr).floor() as usize;
            let bin_end = (freq_hi * FFT_SIZE as f32 / sr).ceil() as usize;

            // Clamp to valid range and ensure at least one bin
            let bin_start = bin_start.min(max_bin);
            let bin_end = bin_end.clamp(bin_start + 1, max_bin + 1);

            BinRange {
                start: bin_start,
                end: bin_end,
            }
        })
        .collect()
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;
    use std::thread;

    const SAMPLE_RATE: u32 = 48000;

    /// Helper: fill the ring buffer with a sine wave at the given frequency and amplitude.
    fn fill_sine(writer: &SpectrumWriter, freq_hz: f32, amplitude: f32, sample_rate: u32) {
        let mut samples = vec![0.0f32; FFT_SIZE];
        for (n, s) in samples.iter_mut().enumerate() {
            *s = amplitude
                * (2.0 * std::f32::consts::PI * freq_hz * n as f32 / sample_rate as f32).sin();
        }
        writer.push_samples(&samples);
    }

    /// Helper: fill the ring buffer with silence.
    fn fill_silence(writer: &SpectrumWriter) {
        let silence = vec![0.0f32; FFT_SIZE];
        writer.push_samples(&silence);
    }

    // ── Test: 1 kHz sine wave peak detection ───────────────────────────

    #[test]
    fn test_sine_1khz_peak_detection() {
        let (writer, mut reader) = create_spectrum_pair(SAMPLE_RATE);

        // Fill with 1 kHz sine at 0 dBFS (amplitude = 1.0)
        fill_sine(&writer, 1000.0, 1.0, SAMPLE_RATE);

        // Run get_spectrum multiple times to let smoothing converge
        let mut spectrum = vec![DB_FLOOR; SPECTRUM_BINS];
        for _ in 0..20 {
            fill_sine(&writer, 1000.0, 1.0, SAMPLE_RATE);
            let s = reader.get_spectrum();
            spectrum.copy_from_slice(s);
        }

        // Find the peak band
        let (peak_band, peak_db) = spectrum
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();

        // Convert peak band index to frequency
        let peak_freq =
            FREQ_MIN * (FREQ_MAX / FREQ_MIN).powf(peak_band as f32 / SPECTRUM_BINS as f32);

        // FFT bin width at 48 kHz = 48000 / 4096 ≈ 11.7 Hz
        let bin_width = SAMPLE_RATE as f32 / FFT_SIZE as f32;

        assert!(
            (peak_freq - 1000.0).abs() < bin_width * 3.0,
            "Peak frequency {:.1} Hz is too far from 1000 Hz",
            peak_freq
        );

        assert!(
            *peak_db > -8.0,
            "Peak magnitude {:.1} dB is too low (expected ≈ 0 dB, ±8 dB tolerance for Hann window)",
            peak_db
        );
        assert!(
            *peak_db <= DB_CEILING,
            "Peak magnitude {:.1} dB exceeds ceiling",
            peak_db
        );
    }

    // ── Test: silence → all bins at DB_FLOOR ───────────────────────────

    #[test]
    fn test_silence_all_bins_at_floor() {
        let (writer, mut reader) = create_spectrum_pair(SAMPLE_RATE);

        fill_silence(&writer);

        let mut spectrum = vec![0.0f32; SPECTRUM_BINS];
        for _ in 0..10 {
            let s = reader.get_spectrum();
            spectrum.copy_from_slice(s);
        }

        for (i, &db) in spectrum.iter().enumerate() {
            assert!(
                db <= DB_FLOOR + 1.0,
                "Band {} has value {:.1} dB, expected ≤ {:.1} dB",
                i,
                db,
                DB_FLOOR + 1.0
            );
        }
    }

    // ── Test: bin mapping coverage ─────────────────────────────────────

    #[test]
    fn test_bin_mapping_coverage() {
        let mapping = compute_bin_mapping(SAMPLE_RATE);

        assert_eq!(mapping.len(), SPECTRUM_BINS);

        let expected_first_bin = (FREQ_MIN * FFT_SIZE as f32 / SAMPLE_RATE as f32).floor() as usize;
        assert_eq!(
            mapping[0].start, expected_first_bin,
            "First band should start at bin {} (≈20 Hz)",
            expected_first_bin
        );

        let expected_last_bin = (FREQ_MAX * FFT_SIZE as f32 / SAMPLE_RATE as f32).ceil() as usize;
        assert!(
            mapping[SPECTRUM_BINS - 1].end >= expected_last_bin.min(FFT_SIZE / 2 + 1),
            "Last band should extend to bin {} (≈20 kHz), got {}",
            expected_last_bin,
            mapping[SPECTRUM_BINS - 1].end
        );

        for (i, range) in mapping.iter().enumerate() {
            assert!(
                range.end > range.start,
                "Band {} has empty range: [{}, {})",
                i,
                range.start,
                range.end
            );
        }

        for i in 1..SPECTRUM_BINS {
            assert!(
                mapping[i].start >= mapping[i - 1].start,
                "Band {} start ({}) < band {} start ({})",
                i,
                mapping[i].start,
                i - 1,
                mapping[i - 1].start
            );
        }
    }

    // ── Test: Hann window coefficients ─────────────────────────────────

    #[test]
    fn test_hann_window_coefficients() {
        let (_writer, reader) = create_spectrum_pair(SAMPLE_RATE);

        assert!(
            reader.window[0].abs() < 1e-6,
            "Window[0] = {}, expected ≈ 0",
            reader.window[0]
        );

        assert!(
            (reader.window[FFT_SIZE / 2] - 1.0).abs() < 1e-6,
            "Window[2048] = {}, expected ≈ 1.0",
            reader.window[FFT_SIZE / 2]
        );

        assert!(
            reader.window[FFT_SIZE - 1].abs() < 0.01,
            "Window[4095] = {}, expected ≈ 0",
            reader.window[FFT_SIZE - 1]
        );

        for (i, &w) in reader.window.iter().enumerate() {
            assert!(
                (0.0..=1.0).contains(&w),
                "Window[{}] = {} is out of range [0, 1]",
                i,
                w
            );
        }
    }

    // ── Test: asymmetric smoothing ─────────────────────────────────────

    #[test]
    fn test_asymmetric_smoothing() {
        let (writer, mut reader) = create_spectrum_pair(SAMPLE_RATE);

        // Start from silence (all bins at DB_FLOOR)
        fill_silence(&writer);
        reader.get_spectrum();

        // Inject a loud sine to create a rising signal
        fill_sine(&writer, 1000.0, 1.0, SAMPLE_RATE);
        let spectrum_after_rise = reader.get_spectrum().to_vec();

        let peak_band = spectrum_after_rise
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;

        let risen_value = spectrum_after_rise[peak_band];

        assert!(
            risen_value > DB_FLOOR + 20.0,
            "Peak band should rise quickly (attack), got {:.1} dB",
            risen_value
        );

        // Now switch to silence — the peak should decay slowly
        fill_silence(&writer);
        let spectrum_after_one_decay = reader.get_spectrum().to_vec();

        let decayed_value = spectrum_after_one_decay[peak_band];

        let expected_decay_step = risen_value + SMOOTHING_DECAY * (DB_FLOOR - risen_value);
        assert!(
            (decayed_value - expected_decay_step).abs() < 1.0,
            "Decay step: got {:.1}, expected ≈ {:.1}",
            decayed_value,
            expected_decay_step
        );

        let rise_amount = risen_value - DB_FLOOR;
        let decay_amount = (risen_value - decayed_value).abs();
        assert!(
            rise_amount > decay_amount * 5.0,
            "Attack ({:.1}) should be much faster than decay ({:.1})",
            rise_amount,
            decay_amount
        );
    }

    // ── Test: silent decay convergence ──────────────────────────────────

    #[test]
    fn test_silent_decay_convergence() {
        let (writer, mut reader) = create_spectrum_pair(SAMPLE_RATE);

        for _ in 0..10 {
            fill_sine(&writer, 1000.0, 1.0, SAMPLE_RATE);
            reader.get_spectrum();
        }

        fill_silence(&writer);
        for _ in 0..60 {
            reader.get_spectrum();
        }

        let spectrum = reader.get_spectrum();

        for (i, &db) in spectrum.iter().enumerate() {
            assert!(
                db < DB_FLOOR + 5.0,
                "Band {} still at {:.1} dB after 60 decay steps (expected near {:.1})",
                i,
                db,
                DB_FLOOR
            );
        }
    }

    // ── Test: concurrent write/read with barriers ──────────────────────

    #[test]
    fn test_concurrent_write_read() {
        let (writer, mut reader) = create_spectrum_pair(SAMPLE_RATE);

        let barrier_write = Arc::new(Barrier::new(2));
        let barrier_read = Arc::new(Barrier::new(2));

        let bw = Arc::clone(&barrier_write);
        let br = Arc::clone(&barrier_read);

        // Writer thread: push samples, then wait for reader, repeat
        let writer_handle = thread::spawn(move || {
            for round in 0..20 {
                let freq = 440.0 + (round as f32 * 50.0);
                let mut samples = [0.0f32; 256];
                for (n, s) in samples.iter_mut().enumerate() {
                    *s = (2.0 * std::f32::consts::PI * freq * n as f32 / SAMPLE_RATE as f32)
                        .sin();
                }
                writer.push_samples(&samples);
                // Signal: "I've written, you may read"
                bw.wait();
                // Wait for reader to finish before writing again
                br.wait();
            }
        });

        for _ in 0..20 {
            // Wait for writer to finish its batch
            barrier_write.wait();

            let spectrum = reader.get_spectrum();
            assert_eq!(spectrum.len(), SPECTRUM_BINS);
            for &db in spectrum.iter() {
                assert!(db.is_finite());
                assert!((DB_FLOOR..=DB_CEILING).contains(&db));
            }

            // Signal: "I've read, you may write"
            barrier_read.wait();
        }

        writer_handle.join().unwrap();
    }

    // ── Test: sample rate update ───────────────────────────────────────

    #[test]
    fn test_sample_rate_update() {
        let (writer, mut reader) = create_spectrum_pair(48000);

        fill_sine(&writer, 1000.0, 1.0, 48000);
        let spectrum_48k = reader.get_spectrum().to_vec();

        // Update sample rate to 44100
        writer.update_sample_rate(44100);

        // Fill with 1 kHz sine at 44100
        let mut samples = vec![0.0f32; FFT_SIZE];
        for (n, s) in samples.iter_mut().enumerate() {
            *s = (2.0 * std::f32::consts::PI * 1000.0 * n as f32 / 44100.0).sin();
        }
        for _ in 0..20 {
            writer.push_samples(&samples);
            reader.get_spectrum();
        }
        let spectrum_44k = reader.get_spectrum().to_vec();

        let peak_48k = spectrum_48k.iter().cloned().fold(DB_FLOOR, f32::max);
        let peak_44k = spectrum_44k.iter().cloned().fold(DB_FLOOR, f32::max);

        assert!(
            peak_48k > DB_FLOOR + 30.0,
            "48k spectrum should have a clear peak, got {:.1} dB",
            peak_48k
        );
        assert!(
            peak_44k > DB_FLOOR + 30.0,
            "44.1k spectrum should have a clear peak, got {:.1} dB",
            peak_44k
        );

        for &db in &spectrum_44k {
            assert!((DB_FLOOR..=DB_CEILING).contains(&db));
        }
    }

    // ── Test: NaN/Inf input sanitization ───────────────────────────────

    #[test]
    fn test_nan_inf_input_sanitized() {
        let (writer, mut reader) = create_spectrum_pair(SAMPLE_RATE);

        let mut samples = vec![0.0f32; FFT_SIZE];
        samples[0] = f32::NAN;
        samples[1] = f32::INFINITY;
        samples[2] = f32::NEG_INFINITY;
        samples[100] = f32::NAN;
        writer.push_samples(&samples);

        let spectrum = reader.get_spectrum();

        assert_eq!(spectrum.len(), SPECTRUM_BINS);
        for (i, &db) in spectrum.iter().enumerate() {
            assert!(
                db.is_finite(),
                "Band {} has non-finite value {} after NaN/Inf input",
                i,
                db
            );
            assert!(
                (DB_FLOOR..=DB_CEILING).contains(&db),
                "Band {} has out-of-range value {:.1} dB after NaN/Inf input",
                i,
                db
            );
        }
    }

    // ── Test: multi-frequency peak resolution ──────────────────────────

    #[test]
    fn test_multi_frequency_peaks() {
        let (writer, mut reader) = create_spectrum_pair(SAMPLE_RATE);

        let mut samples = vec![0.0f32; FFT_SIZE];
        for (n, s) in samples.iter_mut().enumerate() {
            let t = n as f32 / SAMPLE_RATE as f32;
            *s = 0.5 * (2.0 * std::f32::consts::PI * 500.0 * t).sin()
                + 0.5 * (2.0 * std::f32::consts::PI * 4000.0 * t).sin();
        }

        for _ in 0..30 {
            writer.push_samples(&samples);
            reader.get_spectrum();
        }
        let spectrum = reader.get_spectrum().to_vec();

        let band_to_freq =
            |band: usize| FREQ_MIN * (FREQ_MAX / FREQ_MIN).powf(band as f32 / SPECTRUM_BINS as f32);

        let find_peak_in_range = |lo_hz: f32, hi_hz: f32| -> (usize, f32) {
            spectrum
                .iter()
                .enumerate()
                .filter(|&(i, _)| {
                    let f = band_to_freq(i);
                    f >= lo_hz && f <= hi_hz
                })
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
                .map(|(i, &db)| (i, db))
                .unwrap()
        };

        let (low_band, low_db) = find_peak_in_range(300.0, 800.0);
        let (high_band, high_db) = find_peak_in_range(2000.0, 6000.0);

        let low_freq = band_to_freq(low_band);
        let high_freq = band_to_freq(high_band);

        assert!(
            low_db > DB_FLOOR + 20.0,
            "500 Hz region peak at {:.1} Hz = {:.1} dB, expected well above floor",
            low_freq,
            low_db
        );
        assert!(
            high_db > DB_FLOOR + 20.0,
            "4 kHz region peak at {:.1} Hz = {:.1} dB, expected well above floor",
            high_freq,
            high_db
        );

        let bin_width = SAMPLE_RATE as f32 / FFT_SIZE as f32;

        assert!(
            (low_freq - 500.0).abs() < bin_width * 5.0,
            "Lower peak at {:.1} Hz, expected ≈ 500 Hz",
            low_freq
        );
        assert!(
            (high_freq - 4000.0).abs() < bin_width * 5.0,
            "Upper peak at {:.1} Hz, expected ≈ 4000 Hz",
            high_freq
        );

        let (_, valley_db) = find_peak_in_range(1200.0, 1800.0);
        assert!(
            valley_db < low_db - 10.0 && valley_db < high_db - 10.0,
            "Valley between peaks ({:.1} dB) should be significantly lower than peaks ({:.1}, {:.1} dB)",
            valley_db,
            low_db,
            high_db
        );
    }

    // ── Test: reset clears state ───────────────────────────────────────

    #[test]
    fn test_reset() {
        let (writer, mut reader) = create_spectrum_pair(SAMPLE_RATE);

        for _ in 0..20 {
            fill_sine(&writer, 1000.0, 1.0, SAMPLE_RATE);
            reader.get_spectrum();
        }

        let peak_before = reader
            .get_spectrum()
            .iter()
            .cloned()
            .fold(DB_FLOOR, f32::max);
        assert!(peak_before > DB_FLOOR + 30.0);

        reader.reset();

        let spectrum = reader.get_spectrum();
        for (i, &db) in spectrum.iter().enumerate() {
            assert!(
                db <= DB_FLOOR + 1.0,
                "Band {} should be near floor after reset, got {:.1} dB",
                i,
                db
            );
        }
    }

    // ── Test: push_samples timing (audio callback overhead) ──

    #[test]
    fn test_push_samples_overhead() {
        use std::time::Instant;

        let (writer, _reader) = create_spectrum_pair(SAMPLE_RATE);
        let buffer = [0.0f32; 256]; // typical callback buffer size

        // Warm up
        for _ in 0..100 {
            writer.push_samples(&buffer);
        }

        // Measure 10000 iterations
        let iterations = 10_000;
        let start = Instant::now();
        for _ in 0..iterations {
            writer.push_samples(&buffer);
        }
        let elapsed = start.elapsed();
        let avg_us = elapsed.as_micros() as f64 / iterations as f64;

        println!(
            "[Klaar Perf] push_samples avg: {:.3} μs per call ({} iterations)",
            avg_us, iterations
        );

        // Target: < 10 μs per call (should be ~0.5-2 μs on modern hardware)
        assert!(
            avg_us < 10.0,
            "push_samples took {:.3} μs avg — exceeds 10 μs budget",
            avg_us
        );
    }
}
