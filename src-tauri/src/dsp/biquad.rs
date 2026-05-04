/// Biquad filter — Direct Form II Transposed.
///
/// Coefficient formulas from Robert Bristow-Johnson's Audio EQ Cookbook.
/// Supported types: Bell (peaking), HighPass, LowPass, HighShelf, LowShelf.
///
/// Double-buffered coefficient swap: two coefficient sets + AtomicBool index.
/// When params change, new coefficients are computed into the inactive set,
/// then the active flag is atomically swapped. The audio thread always sees a
/// complete, consistent coefficient set.
use std::sync::atomic::{AtomicBool, Ordering};
use crate::dsp::params::FilterType;

// ────────────────────────────────────────────────────────────────────────────
// 8th-order Butterworth Q values (4 cascaded biquad stages)
// ────────────────────────────────────────────────────────────────────────────

/// Butterworth Q values for an 8th-order filter implemented as 4 cascaded
/// 2nd-order sections. Derived from pole placement on the unit circle.
/// At the reference Q of 1/√2 ≈ 0.7071, these produce a maximally-flat
/// passband with 48 dB/octave rolloff.
pub const BUTTERWORTH_8TH_ORDER_QS: [f32; 4] = [0.5098, 0.6013, 0.8999, 2.5628];

/// Reference Q for Butterworth normalisation (1/√2).
pub const BUTTERWORTH_REF_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

// ────────────────────────────────────────────────────────────────────────────
// Coefficient set
// ────────────────────────────────────────────────────────────────────────────

/// One complete set of biquad coefficients.
#[derive(Clone, Copy, Debug)]
pub struct Coeffs {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32, // note: a0 is normalised to 1.0
    pub a2: f32,
}

impl Coeffs {
    /// Identity (passthrough) coefficients.
    pub const IDENTITY: Self = Self {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    /// Compute RBJ biquad coefficients for the given filter type.
    ///
    /// - `freq`  — centre / corner frequency in Hz
    /// - `gain`  — shelving/peaking gain in dB (unused for HP/LP)
    /// - `q`     — quality factor (bandwidth control)
    /// - `sr`    — sample rate in Hz
    pub fn compute(filter_type: FilterType, freq: f32, gain_db: f32, q: f32, sr: f32) -> Self {
        // Guard against degenerate inputs
        let freq = freq.clamp(1.0, sr * 0.499);
        let q = q.max(0.001);

        let w0 = 2.0 * std::f32::consts::PI * freq / sr;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q);

        match filter_type {
            FilterType::Bell => {
                // Peaking EQ (RBJ "Peaking EQ filter")
                let a = 10.0f32.powf(gain_db / 40.0); // sqrt of linear gain
                let b0 = 1.0 + alpha * a;
                let b1 = -2.0 * cos_w0;
                let b2 = 1.0 - alpha * a;
                let a0 = 1.0 + alpha / a;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha / a;
                Self::normalise(b0, b1, b2, a0, a1, a2)
            }
            FilterType::HighPass => {
                // High-pass filter (RBJ "HPF")
                let b0 = (1.0 + cos_w0) / 2.0;
                let b1 = -(1.0 + cos_w0);
                let b2 = (1.0 + cos_w0) / 2.0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha;
                Self::normalise(b0, b1, b2, a0, a1, a2)
            }
            FilterType::LowPass => {
                // Low-pass filter (RBJ "LPF")
                let b0 = (1.0 - cos_w0) / 2.0;
                let b1 = 1.0 - cos_w0;
                let b2 = (1.0 - cos_w0) / 2.0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha;
                Self::normalise(b0, b1, b2, a0, a1, a2)
            }
            FilterType::HighShelf => {
                // High-shelf filter (RBJ "High shelf EQ filter")
                let a = 10.0f32.powf(gain_db / 40.0);
                let a_sqrt = a.sqrt();
                let b0 = a * ((a + 1.0) + (a - 1.0) * cos_w0 + 2.0 * a_sqrt * alpha);
                let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0);
                let b2 = a * ((a + 1.0) + (a - 1.0) * cos_w0 - 2.0 * a_sqrt * alpha);
                let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + 2.0 * a_sqrt * alpha;
                let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_w0);
                let a2 = (a + 1.0) - (a - 1.0) * cos_w0 - 2.0 * a_sqrt * alpha;
                Self::normalise(b0, b1, b2, a0, a1, a2)
            }
            FilterType::LowShelf => {
                // Low-shelf filter (RBJ "Low shelf EQ filter")
                let a = 10.0f32.powf(gain_db / 40.0);
                let a_sqrt = a.sqrt();
                let b0 = a * ((a + 1.0) - (a - 1.0) * cos_w0 + 2.0 * a_sqrt * alpha);
                let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0);
                let b2 = a * ((a + 1.0) - (a - 1.0) * cos_w0 - 2.0 * a_sqrt * alpha);
                let a0 = (a + 1.0) + (a - 1.0) * cos_w0 + 2.0 * a_sqrt * alpha;
                let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0);
                let a2 = (a + 1.0) + (a - 1.0) * cos_w0 - 2.0 * a_sqrt * alpha;
                Self::normalise(b0, b1, b2, a0, a1, a2)
            }
            FilterType::BandPass => {
                // BPF with constant 0 dB peak gain (RBJ)
                // b0 = sin(w0)/2 = Q * alpha, b1 = 0, b2 = -Q * alpha
                let b0 = sin_w0 / 2.0;
                let b1 = 0.0;
                let b2 = -sin_w0 / 2.0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha;
                Self::normalise(b0, b1, b2, a0, a1, a2)
            }
            // 48 dB/oct types delegate to their base (HP/LP) — the cascade
            // logic lives in ParametricEq, not here. Coeffs::compute is only
            // called for individual biquad stages.
            FilterType::HighPass48 => Self::compute(FilterType::HighPass, freq, gain_db, q, sr),
            FilterType::LowPass48  => Self::compute(FilterType::LowPass, freq, gain_db, q, sr),
        }
    }

    #[inline]
    fn normalise(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// BiquadFilter
// ────────────────────────────────────────────────────────────────────────────

/// Single biquad filter with double-buffered coefficient swap.
///
/// The filter maintains two coefficient sets. `update()` is called exclusively
/// from the audio thread at the top of each `process()` call. When parameters
/// change, new coefficients are computed into the inactive buffer and the active
/// index is atomically swapped, ensuring `process_sample()` always sees a
/// complete, consistent set — never a half-written one from a mid-buffer update.
pub struct BiquadFilter {
    /// Double-buffered coefficients: [0] and [1]
    coeffs: [Coeffs; 2],
    /// Which buffer is currently active on the audio thread (0 or 1)
    active_buf: AtomicBool, // false = 0, true = 1

    /// Direct Form II Transposed delay state
    z1: f32,
    z2: f32,

    /// Cached parameters — detect changes to trigger coefficient recalc
    cached_type: FilterType,
    cached_freq: f32,
    cached_gain: f32,
    cached_q: f32,
    cached_sr: f32,
}

impl BiquadFilter {
    pub fn new() -> Self {
        Self {
            coeffs: [Coeffs::IDENTITY; 2],
            active_buf: AtomicBool::new(false),
            z1: 0.0,
            z2: 0.0,
            cached_type: FilterType::Bell,
            cached_freq: 1000.0,
            cached_gain: 0.0,
            cached_q: 1.0,
            cached_sr: 48000.0,
        }
    }

    /// Update filter parameters. If any parameter changed, recomputes
    /// coefficients into the inactive buffer and swaps the active flag.
    ///
    /// Called from the audio thread at the top of each `process()` call.
    /// The swap is a single atomic store — safe for the audio thread.
    pub fn update(&mut self, filter_type: FilterType, freq: f32, gain: f32, q: f32, sr: f32) {
        let changed = filter_type != self.cached_type
            || (freq - self.cached_freq).abs() > 0.01
            || (gain - self.cached_gain).abs() > 0.001
            || (q - self.cached_q).abs() > 0.0001
            || (sr - self.cached_sr).abs() > 0.1;

        if !changed {
            return;
        }

        // Write new coefficients into the inactive buffer
        let current = self.active_buf.load(Ordering::Relaxed);
        let inactive = !current;
        let inactive_idx = inactive as usize;
        self.coeffs[inactive_idx] = Coeffs::compute(filter_type, freq, gain, q, sr);

        // Atomic swap — audio thread will see the new set on the next sample
        self.active_buf.store(inactive, Ordering::Release);

        self.cached_type = filter_type;
        self.cached_freq = freq;
        self.cached_gain = gain;
        self.cached_q = q;
        self.cached_sr = sr;
    }

    /// Set coefficients directly (bypasses parameter caching).
    /// Used for sidechain filters in the de-esser.
    pub fn set_coeffs(&mut self, coeffs: Coeffs) {
        let current = self.active_buf.load(Ordering::Relaxed);
        let inactive_idx = (!current) as usize;
        self.coeffs[inactive_idx] = coeffs;
        self.active_buf.store(!current, Ordering::Release);
    }

    /// Process a single sample (Direct Form II Transposed).
    #[inline(always)]
    pub fn process_sample(&mut self, x: f32) -> f32 {
        let active = self.active_buf.load(Ordering::Acquire) as usize;
        let c = &self.coeffs[active];

        let y = c.b0 * x + self.z1;
        self.z1 = c.b1 * x - c.a1 * y + self.z2;
        self.z2 = c.b2 * x - c.a2 * y;

        // Flush denormals
        flush_denormal(&mut self.z1);
        flush_denormal(&mut self.z2);

        y
    }

    /// Process a buffer in-place.
    #[inline]
    pub fn process_buffer(&mut self, buffer: &mut [f32]) {
        for s in buffer.iter_mut() {
            let x = sanitise(*s);
            *s = self.process_sample(x);
        }
    }

    /// Reset delay state (called when the audio engine restarts).
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

impl Default for BiquadFilter {
    fn default() -> Self {
        Self::new()
    }
}

// ────────────────────────────────────────────────────────────────────────────
// DSP helpers
// ────────────────────────────────────────────────────────────────────────────

/// Clamp NaN/Inf to 0.0 — must run on all audio-thread inputs.
#[inline(always)]
pub fn sanitise(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}

/// Flush denormals: values with magnitude below 1e-15 → 0.
#[inline(always)]
pub fn flush_denormal(x: &mut f32) {
    if x.abs() < 1e-15 {
        *x = 0.0;
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    const SR: f32 = 48000.0;

    /// Generate N samples of a sine wave at `freq` Hz.
    fn sine_wave(freq: f32, sr: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq / sr * i as f32).sin())
            .collect()
    }

    /// RMS of a slice.
    fn rms(buf: &[f32]) -> f32 {
        let sum_sq: f32 = buf.iter().map(|s| s * s).sum();
        (sum_sq / buf.len() as f32).sqrt()
    }

    /// Linear amplitude → dBFS.
    fn to_db(linear: f32) -> f32 {
        20.0 * linear.log10()
    }

    /// Process a buffer of samples through a biquad configured with the given params.
    fn process_through(filter_type: FilterType, freq: f32, gain_db: f32, q: f32, signal: &[f32]) -> Vec<f32> {
        let mut f = BiquadFilter::new();
        f.update(filter_type, freq, gain_db, q, SR);
        let mut buf = signal.to_vec();
        // Run twice the signal length to allow transient to settle
        for s in buf.iter_mut() {
            *s = f.process_sample(*s);
        }
        buf
    }

    #[test]
    fn bell_boost_at_centre_frequency() {
        // Bell +6 dB at 1 kHz — 1 kHz sine should be boosted ~6 dB
        let sig = sine_wave(1000.0, SR, 8192);
        let out = process_through(FilterType::Bell, 1000.0, 6.0, 4.0, &sig);

        // Skip first 512 samples (transient settlement)
        let in_rms = rms(&sig[512..]);
        let out_rms = rms(&out[512..]);
        let gain_db = to_db(out_rms / in_rms);
        // Expect ~6 dB ±0.1 dB (steady-state sine at centre frequency — RBJ should be exact)
        assert!(
            (gain_db - 6.0).abs() < 0.1,
            "Bell +6 dB at centre: got {gain_db:.3} dB"
        );
    }

    #[test]
    fn bell_cut_at_centre_frequency() {
        // Bell -6 dB at 1 kHz
        let sig = sine_wave(1000.0, SR, 8192);
        let out = process_through(FilterType::Bell, 1000.0, -6.0, 4.0, &sig);
        let gain_db = to_db(rms(&out[512..]) / rms(&sig[512..]));
        assert!(
            (gain_db - (-6.0)).abs() < 0.1,
            "Bell -6 dB at centre: got {gain_db:.3} dB"
        );
    }

    #[test]
    fn bell_flat_when_gain_zero() {
        // Bell 0 dB should pass signal unchanged
        let sig = sine_wave(1000.0, SR, 4096);
        let out = process_through(FilterType::Bell, 1000.0, 0.0, 1.0, &sig);
        let gain_db = to_db(rms(&out[512..]) / rms(&sig[512..]));
        assert!(
            gain_db.abs() < 0.01,
            "Bell 0 dB should be flat: got {gain_db:.4} dB"
        );
    }

    #[test]
    fn high_pass_attenuates_below_cutoff() {
        // HP at 1 kHz: 100 Hz signal should be heavily attenuated
        let low_sig = sine_wave(100.0, SR, 8192);
        let out_low = process_through(FilterType::HighPass, 1000.0, 0.0, 0.707, &low_sig);
        let gain_db = to_db(rms(&out_low[1024..]) / rms(&low_sig[1024..]));
        // 100 Hz is a decade below 1 kHz cutoff: expect ~-40 dB attenuation for 2nd order
        assert!(gain_db < -20.0, "HP should attenuate 100 Hz heavily: got {gain_db:.1} dB");
    }

    #[test]
    fn high_pass_passes_above_cutoff() {
        // HP at 200 Hz: 8 kHz signal should pass near unity
        let high_sig = sine_wave(8000.0, SR, 8192);
        let out_high = process_through(FilterType::HighPass, 200.0, 0.0, 0.707, &high_sig);
        let gain_db = to_db(rms(&out_high[512..]) / rms(&high_sig[512..]));
        assert!(gain_db > -1.0, "HP should pass 8 kHz near unity: got {gain_db:.2} dB");
    }

    #[test]
    fn low_pass_attenuates_above_cutoff() {
        // LP at 1 kHz: 10 kHz signal should be heavily attenuated
        let high_sig = sine_wave(10000.0, SR, 8192);
        let out_high = process_through(FilterType::LowPass, 1000.0, 0.0, 0.707, &high_sig);
        let gain_db = to_db(rms(&out_high[1024..]) / rms(&high_sig[1024..]));
        assert!(gain_db < -20.0, "LP should attenuate 10 kHz: got {gain_db:.1} dB");
    }

    #[test]
    fn low_pass_passes_below_cutoff() {
        // LP at 8 kHz: 100 Hz signal should pass near unity
        let low_sig = sine_wave(100.0, SR, 8192);
        let out_low = process_through(FilterType::LowPass, 8000.0, 0.0, 0.707, &low_sig);
        let gain_db = to_db(rms(&out_low[512..]) / rms(&low_sig[512..]));
        assert!(gain_db > -1.0, "LP should pass 100 Hz near unity: got {gain_db:.2} dB");
    }

    #[test]
    fn silence_in_silence_out() {
        let mut f = BiquadFilter::new();
        f.update(FilterType::Bell, 1000.0, 6.0, 1.0, SR);
        let mut buf = vec![0.0f32; 256];
        f.process_buffer(&mut buf);
        for s in &buf {
            assert_relative_eq!(*s, 0.0, epsilon = 1e-12);
        }
    }

    #[test]
    fn nan_input_produces_finite_output() {
        let mut f = BiquadFilter::new();
        f.update(FilterType::Bell, 1000.0, 6.0, 1.0, SR);
        let mut buf = vec![f32::NAN; 32];
        f.process_buffer(&mut buf);
        for s in &buf {
            assert!(s.is_finite(), "NaN input must produce finite output");
        }
    }

    #[test]
    fn inf_input_produces_finite_output() {
        let mut f = BiquadFilter::new();
        f.update(FilterType::Bell, 1000.0, 6.0, 1.0, SR);
        let mut buf = vec![f32::INFINITY; 32];
        f.process_buffer(&mut buf);
        for s in &buf {
            assert!(s.is_finite(), "Inf input must produce finite output");
        }
    }

    #[test]
    fn high_shelf_boosts_high_frequencies() {
        let sig = sine_wave(16000.0, SR, 8192);
        let out = process_through(FilterType::HighShelf, 4000.0, 6.0, 0.707, &sig);
        let gain_db = to_db(rms(&out[512..]) / rms(&sig[512..]));
        assert!(gain_db > 3.0, "HighShelf should boost 16 kHz: got {gain_db:.2} dB");
    }

    #[test]
    fn low_shelf_boosts_low_frequencies() {
        let sig = sine_wave(100.0, SR, 8192);
        let out = process_through(FilterType::LowShelf, 2000.0, 6.0, 0.707, &sig);
        let gain_db = to_db(rms(&out[512..]) / rms(&sig[512..]));
        assert!(gain_db > 3.0, "LowShelf should boost 100 Hz: got {gain_db:.2} dB");
    }
}
