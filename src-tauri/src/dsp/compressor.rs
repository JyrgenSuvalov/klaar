/// Feed-forward compressor with log-domain gain computation.
///
/// Topology (Giannoulis, Massberg & Reiss, JAES 2012, eq. 16):
///   1. Peak-hold envelope detector on |input|: instantaneous on rising peaks,
///      exponential decay at the user-set release rate between peaks. Removes
///      audio-rate ripple at the source — vanilla Giannoulis uses raw |x| and
///      can still ripple below ~200 Hz when attack is faster than the period.
///   2. Convert envelope to dB → static gain computer:
///      gain_reduction_dB = -(level_dB - threshold_dB) * (1 - 1/ratio)
///   3. Smoothed branching peak detector on `gain_reduction_db` (attack when
///      more reduction is needed, release when less). This is the single
///      attack/release stage and binds the user-facing `attack_ms` / `release_ms`.
///   4. Convert smoothed GR back to linear, multiply by sample, multiply by
///      makeup gain.
use std::sync::Arc;
use crate::dsp::params::DspParams;
use crate::dsp::biquad::sanitise;
use crate::dsp::AudioProcessor;

pub struct Compressor {
    params: Arc<DspParams>,
    sample_rate: f32,

    /// Envelope follower state (linear amplitude)
    envelope: f32,
    /// Smoothed gain reduction state (dB, ≤ 0)
    gain_reduction_db: f32,

    /// Cached gain reduction for metering (dB, ≤ 0)
    pub last_gain_reduction_db: f32,
}

impl Compressor {
    pub fn new(params: Arc<DspParams>, sample_rate: f32) -> Self {
        Self {
            params,
            sample_rate,
            envelope: 0.0,
            gain_reduction_db: 0.0,
            last_gain_reduction_db: 0.0,
        }
    }

    #[inline]
    fn time_coeff(time_ms: f32, sr: f32) -> f32 {
        let time_s = (time_ms * 0.001).max(1e-6);
        (-1.0 / (time_s * sr)).exp()
    }
}

impl AudioProcessor for Compressor {
    fn process(&mut self, buffer: &mut [f32], sample_rate: f32) {
        self.sample_rate = sample_rate;

        // Read parameters once per buffer
        let threshold_db = self.params.comp_threshold();
        let ratio = self.params.comp_ratio().max(1.0);
        let attack_coeff = Self::time_coeff(self.params.comp_attack_ms(), sample_rate);
        let release_coeff = Self::time_coeff(self.params.comp_release_ms(), sample_rate);
        let makeup_linear = db_to_linear(self.params.comp_makeup_db());

        let slope = 1.0 - 1.0 / ratio; // > 0 for ratio > 1
        let mut max_reduction_db: f32 = 0.0;

        for s in buffer.iter_mut() {
            let x = sanitise(*s);

            // ── Peak-hold envelope detector ────────────────────────────────
            // Instantaneous on rising peaks; exponential decay between them.
            let abs_x = x.abs();
            self.envelope = abs_x.max(self.envelope * release_coeff);

            // ── Gain computation in dB ─────────────────────────────────────
            let level_db = linear_to_db(self.envelope);
            let target_reduction_db = if level_db > threshold_db {
                -((level_db - threshold_db) * slope) // negative = reducing
            } else {
                0.0
            };

            // ── Smoothed branching peak detector on gain_reduction_db ──────
            // Single attack/release stage; binds user-facing attack/release.
            if target_reduction_db < self.gain_reduction_db {
                // More reduction needed: attack
                self.gain_reduction_db = attack_coeff * self.gain_reduction_db
                    + (1.0 - attack_coeff) * target_reduction_db;
            } else {
                // Less reduction needed: release
                self.gain_reduction_db = release_coeff * self.gain_reduction_db
                    + (1.0 - release_coeff) * target_reduction_db;
            }

            max_reduction_db = max_reduction_db.min(self.gain_reduction_db);

            // ── Apply gain reduction + makeup ──────────────────────────────
            let gain_linear = db_to_linear(self.gain_reduction_db) * makeup_linear;
            *s = x * gain_linear;
        }

        self.last_gain_reduction_db = max_reduction_db;
    }

    fn reset(&mut self) {
        self.envelope = 0.0;
        self.gain_reduction_db = 0.0;
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
    if linear <= 1e-15 { -300.0 } else { 20.0 * linear.log10() }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    const SR: f32 = 48000.0;

    fn make_comp(threshold_db: f32, ratio: f32, attack_ms: f32, release_ms: f32, makeup_db: f32) -> Compressor {
        let params = Arc::new(DspParams::new());
        DspParams::set_f32(&params.comp_threshold, threshold_db);
        DspParams::set_f32(&params.comp_ratio, ratio);
        DspParams::set_f32(&params.comp_attack, attack_ms);
        DspParams::set_f32(&params.comp_release, release_ms);
        DspParams::set_f32(&params.comp_makeup, makeup_db);
        Compressor::new(params, SR)
    }

    fn dc_signal(amp: f32, n: usize) -> Vec<f32> {
        vec![amp; n]
    }

    fn sine_signal(freq_hz: f32, amp: f32, n: usize, sr: f32) -> Vec<f32> {
        let two_pi_f_over_sr = 2.0 * std::f32::consts::PI * freq_hz / sr;
        (0..n)
            .map(|i| amp * (two_pi_f_over_sr * i as f32).sin())
            .collect()
    }

    fn peak(buf: &[f32]) -> f32 {
        buf.iter().fold(0.0f32, |acc, x| acc.max(x.abs()))
    }

    fn rms(buf: &[f32]) -> f32 {
        let s: f32 = buf.iter().map(|x| x * x).sum();
        (s / buf.len() as f32).sqrt()
    }

    fn to_db(linear: f32) -> f32 {
        20.0 * linear.log10()
    }

    #[test]
    fn silence_in_silence_out() {
        let mut comp = make_comp(-20.0, 4.0, 1.0, 100.0, 0.0);
        let mut buf = vec![0.0f32; 512];
        comp.process(&mut buf, SR);
        for s in &buf {
            assert!(s.abs() < 1e-12, "silence → silence, got {s}");
        }
    }

    #[test]
    fn signal_below_threshold_passes_at_unity() {
        // Threshold -20 dB; signal at -40 dB (well below)
        let amp = db_to_linear(-40.0);
        let mut comp = make_comp(-20.0, 4.0, 1.0, 100.0, 0.0);

        // Warmup
        let mut warmup = dc_signal(amp, 9600);
        comp.process(&mut warmup, SR);

        let mut buf = dc_signal(amp, 1024);
        comp.process(&mut buf, SR);

        let out_rms = rms(&buf);
        let gain_db = to_db(out_rms / amp);
        assert!(
            gain_db.abs() < 0.1,
            "Below threshold: expected unity, got {gain_db:.3} dB"
        );
    }

    #[test]
    fn steady_state_gain_reduction_matches_formula() {
        // Threshold -20 dB, ratio 4:1, signal at -10 dB (10 dB above threshold)
        // Expected GR = -(10 * (1 - 1/4)) = -7.5 dB
        let amp = db_to_linear(-10.0);
        let mut comp = make_comp(-20.0, 4.0, 1.0, 500.0, 0.0);

        // Long warmup to reach steady state
        let mut warmup = dc_signal(amp, 48000);
        comp.process(&mut warmup, SR);

        let mut buf = dc_signal(amp, 1024);
        comp.process(&mut buf, SR);

        let out_rms = rms(&buf);
        // gain_db = 20*log10(out/in) = the gain change applied, i.e. the gain reduction
        let gain_db = to_db(out_rms / amp);
        // GR = -(10 dB above threshold) * (1 - 1/4) = -7.5 dB
        let expected_db = -7.5_f32;
        assert_abs_diff_eq!(gain_db, expected_db, epsilon = 0.2);
    }

    #[test]
    fn makeup_gain_applied_post_compression() {
        // Threshold -20 dB, ratio 4:1, +7.5 dB makeup gain
        // Signal at -10 dB: GR = -7.5 dB, output = -10 - 7.5 + 7.5 = -10 dB
        let amp = db_to_linear(-10.0);
        let mut comp = make_comp(-20.0, 4.0, 1.0, 500.0, 7.5);

        let mut warmup = dc_signal(amp, 48000);
        comp.process(&mut warmup, SR);

        let mut buf = dc_signal(amp, 1024);
        comp.process(&mut buf, SR);

        let out_rms = rms(&buf);
        let gain_db = to_db(out_rms / amp);
        // With makeup, output should be close to input level (~-10 dB)
        assert_abs_diff_eq!(gain_db, 0.0_f32, epsilon = 0.5);
    }

    #[test]
    fn ratio_1_is_transparent() {
        // Ratio 1:1 = no compression regardless of level
        let amp = db_to_linear(-6.0); // well above threshold
        let mut comp = make_comp(-20.0, 1.0, 1.0, 100.0, 0.0);

        let mut warmup = dc_signal(amp, 9600);
        comp.process(&mut warmup, SR);

        let mut buf = dc_signal(amp, 1024);
        comp.process(&mut buf, SR);

        let out_rms = rms(&buf);
        let gain_db = to_db(out_rms / amp);
        assert_abs_diff_eq!(gain_db, 0.0_f32, epsilon = 0.1);
    }

    #[test]
    fn attack_slows_gain_reduction() {
        // Slow attack (100 ms): signal starts 0, jumps to above threshold.
        // After one 256-sample buffer (~5.3 ms), GR should not be at steady state.
        let amp = db_to_linear(-6.0);
        let mut comp = make_comp(-20.0, 10.0, 100.0, 100.0, 0.0);

        let mut buf = vec![0.0f32; 256];
        // First: silence (envelope = 0)
        comp.process(&mut buf, SR);

        // Now hot signal
        let mut buf2 = dc_signal(amp, 256);
        comp.process(&mut buf2, SR);

        // With 100 ms attack at 48 kHz, 256 samples is ~5.3 ms << 100 ms
        // Gain reduction should still be << steady state
        let out_rms = rms(&buf2);
        let gain_db = to_db(out_rms / amp);
        // Signal should be mostly uncompressed (< 2 dB reduction after 5.3 ms of 100 ms attack)
        assert!(
            gain_db > -2.0,
            "With slow attack, compression should not be immediate: got {gain_db:.3} dB"
        );
    }

    #[test]
    fn sine_1khz_steady_state_gr_matches_formula() {
        // 1 kHz sine, peak -10 dB, threshold -20 dB, ratio 4:1
        // Expected GR = -(10 * (1 - 1/4)) = -7.5 dB
        let amp = db_to_linear(-10.0);
        let mut comp = make_comp(-20.0, 4.0, 1.0, 500.0, 0.0);

        // Warmup ≥ 1 s
        let mut warmup = sine_signal(1000.0, amp, 48000, SR);
        comp.process(&mut warmup, SR);

        let mut buf = sine_signal(1000.0, amp, 4096, SR);
        comp.process(&mut buf, SR);

        let gain_db = to_db(peak(&buf) / amp);
        let expected_db = -7.5_f32;
        assert_abs_diff_eq!(gain_db, expected_db, epsilon = 0.5);
    }

    #[test]
    fn sine_100hz_steady_state_gr_matches_formula() {
        // 100 Hz sine — regression guard for audio-rate ripple bug.
        let amp = db_to_linear(-10.0);
        let mut comp = make_comp(-20.0, 4.0, 1.0, 500.0, 0.0);

        // ≥ 2 s warm-up to settle the envelope at low frequency.
        let mut warmup = sine_signal(100.0, amp, 96000, SR);
        comp.process(&mut warmup, SR);

        let mut buf = sine_signal(100.0, amp, 4096, SR);
        comp.process(&mut buf, SR);

        let gain_db = to_db(peak(&buf) / amp);
        let expected_db = -7.5_f32;
        assert_abs_diff_eq!(gain_db, expected_db, epsilon = 0.5);
    }

    #[test]
    fn release_returns_to_unity() {
        // Hot DC for 1 s, then 6× release of zeros, then a soft DC block well
        // below threshold. Output peak must match input peak (no residual GR).
        //
        // Note: 6× release (rather than 3×) is required because the peak-hold
        // envelope must first decay below threshold (~1.6 τ_R after a -6 dB
        // → silence step against a -20 dB threshold) before the gain smoother
        // even starts to release. The remaining ~4.4 τ_R settles the smoother
        // well past the 0.2 dB tolerance. Any single-stage topology with a
        // peak-hold detector has this property.
        let release_ms = 100.0_f32;
        let mut comp = make_comp(-20.0, 4.0, 1.0, release_ms, 0.0);

        // Drive hot
        let hot_amp = db_to_linear(-6.0);
        let mut hot = dc_signal(hot_amp, 48000);
        comp.process(&mut hot, SR);

        // 6× release time of zeros
        let release_samples = (release_ms * 0.001 * SR) as usize;
        let mut silence = vec![0.0f32; 6 * release_samples];
        comp.process(&mut silence, SR);

        // Final block well below threshold — should pass at unity
        let soft_amp = db_to_linear(-30.0);
        let mut buf = dc_signal(soft_amp, 1024);
        comp.process(&mut buf, SR);

        let gain_db = to_db(peak(&buf) / soft_amp);
        assert_abs_diff_eq!(gain_db, 0.0_f32, epsilon = 0.2);
    }

    #[test]
    fn attack_time_within_tolerance() {
        // Attack 10 ms, ratio 10:1, threshold -20 dB, signal at -6 dB.
        // Steady-state GR = -(14 * 0.9) = -12.6 dB.
        // (1 - 1/e) × steady-state ≈ 0.6321 × -12.6 ≈ -7.965 dB.
        // Process sample-by-sample (1-sample buffers) so `last_gain_reduction_db`
        // gives instantaneous GR. Find first sample where GR ≤ target.
        let attack_ms = 10.0_f32;
        let mut comp = make_comp(-20.0, 10.0, attack_ms, 500.0, 0.0);

        // Warm with silence so envelope/GR start at rest.
        let mut silence = vec![0.0f32; 4800];
        comp.process(&mut silence, SR);

        let amp = db_to_linear(-6.0);
        let steady_state_gr = -((-6.0_f32 - (-20.0_f32)) * (1.0 - 1.0 / 10.0));
        let target_gr = (1.0 - (-1.0_f32).exp()) * steady_state_gr; // (1 - 1/e) × steady-state, both negative

        let max_samples = (attack_ms * 0.001 * SR * 2.0) as usize; // 2× attack worth of room
        let mut hit_at: Option<usize> = None;
        for i in 0..max_samples {
            let mut one = [amp];
            comp.process(&mut one, SR);
            // Both target_gr and last_gain_reduction_db are ≤ 0; "reached" means
            // GR has descended to or past the target (more negative).
            if comp.last_gain_reduction_db <= target_gr {
                hit_at = Some(i + 1);
                break;
            }
        }

        let hit = hit_at.expect("GR should reach (1-1/e) × steady-state within 2× attack time");
        let expected = attack_ms * 0.001 * SR; // ≈ 480 samples
        let tol = 0.2 * expected; // ±20 %
        let diff = (hit as f32 - expected).abs();
        assert!(
            diff <= tol,
            "Attack time: hit at {hit} samples, expected ≈ {expected} ± {tol} (steady_state_gr={steady_state_gr:.2}, target={target_gr:.2})"
        );
    }

    #[test]
    fn nan_input_clamped() {
        let mut comp = make_comp(-20.0, 4.0, 1.0, 100.0, 0.0);
        let mut buf = vec![f32::NAN; 64];
        comp.process(&mut buf, SR);
        for s in &buf {
            assert!(s.is_finite(), "NaN input → finite output");
        }
    }

    #[test]
    fn inf_input_clamped() {
        let mut comp = make_comp(-20.0, 4.0, 1.0, 100.0, 0.0);
        let mut buf = vec![f32::INFINITY; 64];
        comp.process(&mut buf, SR);
        for s in &buf {
            assert!(s.is_finite(), "Inf input → finite output");
        }
    }
}
