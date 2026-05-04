/// Brickwall limiter — no output sample may exceed the ceiling value.
///
/// Algorithm:
///   1. Instantaneous-attack peak envelope follower (`envelope ≥ |x|` always),
///      with one-pole release smoothing.
///   2. Compute target gain reduction in dB from the (instantaneous) peak
///      against an internal ceiling `ceiling_db − 0.1` (margin keeps the
///      safety clamp dormant on sane signals).
///   3. Snap-attack on `gr_db`; one-pole release smoothing in dB domain so
///      the user-facing `release_ms` feels uniform across GR depths.
///   4. Apply linear gain `db_to_linear(gr_db)`. A hard `clamp(±ceiling_linear)`
///      remains as a safety net for upstream Inf/NaN, never reached by sane
///      finite signals.
///
/// No look-ahead (would add latency).
use std::sync::Arc;
use crate::dsp::params::DspParams;
use crate::dsp::biquad::{flush_denormal, sanitise};
use crate::dsp::AudioProcessor;

pub struct Limiter {
    params: Arc<DspParams>,
    sample_rate: f32,

    /// Peak envelope follower state (linear, ≥ 0)
    pub(crate) envelope: f32,
    /// Smoothed gain-reduction state in dB (≤ 0)
    pub(crate) gr_db: f32,

    /// Gain reduction for metering (dB, ≤ 0)
    pub last_gain_reduction_db: f32,
}

impl Limiter {
    pub fn new(params: Arc<DspParams>, sample_rate: f32) -> Self {
        Self {
            params,
            sample_rate,
            envelope: 0.0,
            gr_db: 0.0,
            last_gain_reduction_db: 0.0,
        }
    }

    #[inline]
    fn time_coeff(time_ms: f32, sr: f32) -> f32 {
        let time_s = (time_ms * 0.001).max(1e-6);
        (-1.0 / (time_s * sr)).exp()
    }
}

impl AudioProcessor for Limiter {
    fn process(&mut self, buffer: &mut [f32], sample_rate: f32) {
        self.sample_rate = sample_rate;

        // Read parameters once per buffer
        let ceiling_db = self.params.limiter_ceiling_db().min(0.0);
        let ceiling_linear = db_to_linear(ceiling_db);
        // Internal target leaves a 0.1 dB margin so the output clamp stays dormant
        let ceiling_internal_db = ceiling_db - 0.1;

        // User-facing release controls gain-stage recovery (gr_db smoothing).
        let release_coeff = Self::time_coeff(self.params.limiter_release_ms(), sample_rate);
        // Envelope follower uses a fast internal release so the peak detector
        // tracks the signal closely; the audible release time comes from the
        // gain smoother above.
        let env_release_coeff = Self::time_coeff(5.0, sample_rate);

        let mut max_gr_db_this_buf: f32 = 0.0;

        for s in buffer.iter_mut() {
            let x = sanitise(*s);
            let abs_x = x.abs();

            // ── Instantaneous-attack peak follower ─────────────────────────
            // envelope ≥ |x| always — guarantees the computed gain reduction
            // is sufficient on every sample.
            let released = env_release_coeff * self.envelope + (1.0 - env_release_coeff) * abs_x;
            self.envelope = if abs_x > released { abs_x } else { released };

            // ── Target gain reduction (dB, ≤ 0) ────────────────────────────
            let env_db = linear_to_db(self.envelope);
            let target_gr_db = (ceiling_internal_db - env_db).min(0.0);

            // ── Snap-attack / one-pole release in dB domain ────────────────
            self.gr_db = if target_gr_db < self.gr_db {
                target_gr_db
            } else {
                release_coeff * self.gr_db + (1.0 - release_coeff) * target_gr_db
            };

            max_gr_db_this_buf = max_gr_db_this_buf.min(self.gr_db);

            // ── Apply gain + safety clamp ──────────────────────────────────
            let gain = db_to_linear(self.gr_db);
            let out = x * gain;
            *s = out.clamp(-ceiling_linear, ceiling_linear);

            // ── Denormal hygiene ───────────────────────────────────────────
            flush_denormal(&mut self.envelope);
            // gr_db ≤ 0; flush its absolute residual back to 0
            let mut residual = -self.gr_db;
            flush_denormal(&mut residual);
            self.gr_db = -residual;
        }

        self.last_gain_reduction_db = max_gr_db_this_buf;
    }

    fn reset(&mut self) {
        self.envelope = 0.0;
        self.gr_db = 0.0;
        self.last_gain_reduction_db = 0.0;
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────────────────────

#[inline]
fn db_to_linear(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

#[inline]
fn linear_to_db(linear: f32) -> f32 {
    if linear <= 0.0 { -96.0 } else { 20.0 * linear.log10() }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    const SR: f32 = 48000.0;

    fn make_limiter(ceiling_db: f32, release_ms: f32) -> Limiter {
        let params = Arc::new(DspParams::new());
        DspParams::set_f32(&params.limiter_ceiling, ceiling_db);
        DspParams::set_f32(&params.limiter_release, release_ms);
        Limiter::new(params, SR)
    }

    fn dc_signal(amp: f32, n: usize) -> Vec<f32> {
        vec![amp; n]
    }

    fn rms(buf: &[f32]) -> f32 {
        let s: f32 = buf.iter().map(|x| x * x).sum();
        (s / buf.len() as f32).sqrt()
    }

    #[test]
    fn no_sample_exceeds_ceiling() {
        let ceiling_db = -3.0_f32;
        let ceiling_linear = 10.0f32.powf(ceiling_db / 20.0);
        let mut lim = make_limiter(ceiling_db, 100.0);

        // Feed a very hot signal (3× the ceiling)
        let hot = dc_signal(ceiling_linear * 3.0, 4096);
        let mut buf = hot;
        lim.process(&mut buf, SR);

        for (i, &s) in buf.iter().enumerate() {
            assert!(
                s.abs() <= ceiling_linear + 1e-6,
                "Sample {i} exceeds ceiling: {:.6} > {:.6}", s.abs(), ceiling_linear
            );
        }
    }

    #[test]
    fn signal_below_ceiling_passes_unchanged() {
        let ceiling_db = 0.0_f32;
        let mut lim = make_limiter(ceiling_db, 100.0);

        // Signal at -6 dBFS — should pass through unmodified
        let amp = 0.5_f32; // -6 dBFS
        let sig = dc_signal(amp, 4096);
        let mut buf = sig.clone();
        lim.process(&mut buf, SR);

        // After envelope settles, gain should be 1.0
        let out_rms = rms(&buf[512..]);
        let in_rms = rms(&sig[512..]);
        let gain_db = 20.0 * (out_rms / in_rms).log10();
        assert_abs_diff_eq!(gain_db, 0.0_f32, epsilon = 0.01);
    }

    #[test]
    fn silence_in_silence_out() {
        let mut lim = make_limiter(-3.0, 100.0);
        let mut buf = vec![0.0f32; 512];
        lim.process(&mut buf, SR);
        for s in &buf {
            assert!(s.abs() < 1e-12, "silence → silence, got {s}");
        }
    }

    #[test]
    fn release_recovers_partially_in_db_domain() {
        // Drive limiter into >3 dB GR, then process a brief silence buffer
        // (≪ release_ms) and assert gr_db moves toward 0 but is still negative.
        let ceiling_db = -6.0_f32;
        let ceiling_linear = 10.0f32.powf(ceiling_db / 20.0);
        let release_ms = 200.0_f32;
        let mut lim = make_limiter(ceiling_db, release_ms);

        // Hot DC at 3× ceiling — quickly reaches deep GR
        let hot = dc_signal(ceiling_linear * 3.0, 4096);
        let mut burst = hot;
        lim.process(&mut burst, SR);

        let gr_before = lim.last_gain_reduction_db;
        assert!(
            gr_before < -3.0,
            "Burst should drive GR below -3 dB, got {gr_before}"
        );

        // ~10 ms of silence (480 samples) — much less than 200 ms release
        let mut silence = vec![0.0f32; 480];
        lim.process(&mut silence, SR);

        let gr_after = lim.last_gain_reduction_db;
        assert!(
            gr_after > gr_before,
            "After silence, GR should recover (less negative): before={gr_before}, after={gr_after}"
        );
        assert!(
            gr_after < 0.0,
            "Release ≫ silence buffer, GR should not have fully recovered: {gr_after}"
        );
    }

    #[test]
    fn onset_transient_limited_without_clamp() {
        // Ceiling = 0 dBFS → safety clamp(±1.0) is a no-op for in-range floats.
        // Any limiting we observe must come from the gain stage, not the clamp.
        let mut lim = make_limiter(0.0, 100.0);

        // Settle on silence
        let mut zeros = vec![0.0f32; 64];
        lim.process(&mut zeros, SR);
        assert_abs_diff_eq!(lim.last_gain_reduction_db, 0.0, epsilon = 1e-6);

        // Single full-scale sample
        let mut spike = vec![1.0f32];
        lim.process(&mut spike, SR);

        assert!(
            lim.last_gain_reduction_db < 0.0,
            "Onset transient should engage gain reduction (clamp inert at 0 dBFS): GR = {}",
            lim.last_gain_reduction_db
        );
    }

    #[test]
    fn nan_input_clamped() {
        let mut lim = make_limiter(-3.0, 100.0);
        let mut buf = vec![f32::NAN; 64];
        lim.process(&mut buf, SR);
        for s in &buf {
            assert!(s.is_finite(), "NaN input → finite output");
        }
        assert!(
            lim.envelope.is_finite() && lim.gr_db.is_finite(),
            "Internal state must remain finite: env={}, gr_db={}",
            lim.envelope, lim.gr_db
        );
    }

    #[test]
    fn inf_input_clamped() {
        let mut lim = make_limiter(-3.0, 100.0);
        let mut buf = vec![f32::INFINITY; 64];
        lim.process(&mut buf, SR);
        for s in &buf {
            assert!(s.is_finite(), "Inf input → finite output");
        }
        assert!(
            lim.envelope.is_finite() && lim.gr_db.is_finite(),
            "Internal state must remain finite: env={}, gr_db={}",
            lim.envelope, lim.gr_db
        );
    }

    #[test]
    fn no_sample_exceeds_ceiling_hot_sine() {
        // Use a sine wave at 2× ceiling amplitude over many cycles
        let ceiling_db = -6.0_f32;
        let ceiling_linear = 10.0f32.powf(ceiling_db / 20.0);
        let mut lim = make_limiter(ceiling_db, 50.0);

        let hot: Vec<f32> = (0..8192)
            .map(|i| {
                ceiling_linear * 2.0
                    * (2.0 * std::f32::consts::PI * 440.0 / SR * i as f32).sin()
            })
            .collect();
        let mut buf = hot;
        lim.process(&mut buf, SR);

        for (i, &s) in buf.iter().enumerate() {
            assert!(
                s.abs() <= ceiling_linear + 1e-5,
                "Sample {i} exceeds ceiling: {:.6} > {:.6}", s.abs(), ceiling_linear
            );
        }
    }
}
