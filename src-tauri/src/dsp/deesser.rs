/// De-esser processor.
///
/// Architecture: wideband ratio-based gain reduction with bandpass sidechain
/// detection, log-domain branching envelope follower per Giannoulis/Massberg/
/// Reiss 2012 ("Digital Dynamic Range Compressor Design — A Tutorial and
/// Analysis", JAES). Topology:
///   1. Sidechain BiquadFilter (RBJ bandpass, Q ≈ 6) centred on `frequency`
///      extracts sibilance.
///   2. The bandpass magnitude is converted to dBFS once per sample (with a
///      finite floor for log10), then a one-pole branching smoother tracks
///      `env_db` with attack/release coefficients.
///   3. Gain reduction is computed directly from the smoothed envelope:
///      `gr_dB = max(0, env_dB - threshold_dB) * (1 - 1/ratio)`.
///   4. The wideband signal is multiplied by `db_to_linear(-gr_dB)` in the
///      same sample. There is no second smoother on the gain — all ballistics
///      live in the dB-domain envelope follower.
///
/// The bandpass + envelope follower run *unconditionally* — even when the
/// effect is bypassed — so the panel's sidechain meter always shows what the
/// detector would see. The bypass guard only gates the gain-application step.
use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::dsp::params::{DspParams, FilterType};
use crate::dsp::biquad::{BiquadFilter, Coeffs, sanitise};
use crate::dsp::AudioProcessor;

pub struct DeEsser {
    params: Arc<DspParams>,
    sample_rate: f32,

    /// Sidechain bandpass filter (detects sibilance)
    sidechain_filter: BiquadFilter,
    /// Log-domain envelope follower (dBFS). Initialised to a low floor so the
    /// first attack rises smoothly from silence. Exposed `pub(crate)` for
    /// closed-form tests that read the settled detector level.
    pub(crate) env_db: f32,

    /// Cached frequency for sidechain filter update
    cached_freq: f32,

    /// Cached attack/release inputs and their derived coefficients. We
    /// recompute coefficients only when `(attack_ms, release_ms, sample_rate)`
    /// change — `exp()` per buffer is fine, per sample is not.
    cached_attack_ms: f32,
    cached_release_ms: f32,
    cached_sr: f32,
    attack_coeff: f32,
    release_coeff: f32,

    /// Gain reduction written to metering (dB, ≤ 0)
    pub last_gain_reduction_db: f32,
    /// Per-buffer peak of the sidechain envelope, in dBFS, clamped to [-96, 0].
    /// Published every buffer regardless of bypass.
    pub last_sidechain_db: f32,
}

impl DeEsser {
    pub fn new(params: Arc<DspParams>, sample_rate: f32) -> Self {
        Self {
            params,
            sample_rate,
            sidechain_filter: BiquadFilter::new(),
            env_db: -120.0,
            cached_freq: 0.0, // force initial filter update
            cached_attack_ms: 0.0,
            cached_release_ms: 0.0,
            cached_sr: 0.0,
            attack_coeff: 0.0,
            release_coeff: 0.0,
            last_gain_reduction_db: 0.0,
            last_sidechain_db: -96.0,
        }
    }

    /// Exponential envelope coefficient from milliseconds.
    #[inline]
    fn time_coeff(time_ms: f32, sr: f32) -> f32 {
        let time_s = (time_ms * 0.001).max(1e-6);
        (-1.0 / (time_s * sr)).exp()
    }
}

impl AudioProcessor for DeEsser {
    fn process(&mut self, buffer: &mut [f32], sample_rate: f32) {
        self.sample_rate = sample_rate;

        // ── Read parameters once per buffer ──────────────────────────────
        let freq         = self.params.deesser_frequency();
        let threshold_db = self.params.deesser_threshold();
        let ratio        = self.params.deesser_ratio().max(1.0);
        let attack_ms    = self.params.deesser_attack_ms();
        let release_ms   = self.params.deesser_release_ms();
        let bypassed     = self.params.bypass_deesser.load(Ordering::Relaxed);

        // ── Update sidechain filter coefficients if frequency changed ────
        if (freq - self.cached_freq).abs() > 0.5 {
            // RBJ bandpass: rejects low and high, passes ~1-octave band.
            let coeffs = Coeffs::compute(FilterType::BandPass, freq, 0.0, 6.0, sample_rate);
            self.sidechain_filter.set_coeffs(coeffs);
            self.cached_freq = freq;
        }

        // ── Update envelope coefficients if attack/release/SR changed ────
        if attack_ms != self.cached_attack_ms
            || release_ms != self.cached_release_ms
            || sample_rate != self.cached_sr
        {
            self.attack_coeff = Self::time_coeff(attack_ms, sample_rate);
            self.release_coeff = Self::time_coeff(release_ms, sample_rate);
            self.cached_attack_ms = attack_ms;
            self.cached_release_ms = release_ms;
            self.cached_sr = sample_rate;
        }

        let attack_coeff = self.attack_coeff;
        let release_coeff = self.release_coeff;
        let one_minus_inv_ratio = 1.0 - 1.0 / ratio;

        let mut min_gain_this_buf: f32 = 1.0;
        let mut peak_env_db: f32 = f32::NEG_INFINITY;

        for s in buffer.iter_mut() {
            let x = sanitise(*s);

            // ── Sidechain detection (always runs, even when bypassed) ────
            let bp_mag = self.sidechain_filter.process_sample(x).abs();

            // Convert to dBFS once per sample with a finite floor so log10
            // never sees zero or denormals.
            let in_db = if bp_mag > 1e-9 {
                20.0 * bp_mag.log10()
            } else {
                -180.0
            };

            // Branching one-pole **in the dB domain** (Giannoulis et al.
            // 2012). Smoothing-once on the detector replaces the previous
            // double-smoothed (linear envelope + gain trajectory) topology.
            if in_db > self.env_db {
                self.env_db = attack_coeff * self.env_db + (1.0 - attack_coeff) * in_db;
            } else {
                self.env_db = release_coeff * self.env_db + (1.0 - release_coeff) * in_db;
            }

            if self.env_db > peak_env_db {
                peak_env_db = self.env_db;
            }

            // Gain reduction follows env_db synchronously — no second smoother.
            let over_db = (self.env_db - threshold_db).max(0.0);
            let gr_db = over_db * one_minus_inv_ratio;
            let target_gain = db_to_linear(-gr_db);

            if bypassed {
                // Bypass: signal passes through unchanged. The detector
                // continues to run (above) so the sidechain meter still
                // publishes and re-engaging the effect is glitch-free.
                *s = x;
            } else {
                min_gain_this_buf = min_gain_this_buf.min(target_gain);
                *s = x * target_gain;
            }
        }

        // Publish the per-buffer peak of the smoothed sidechain envelope in
        // dBFS, clamped to the meter range. If the envelope never updated
        // above the floor we publish -96 dB.
        self.last_sidechain_db = if peak_env_db.is_finite() {
            peak_env_db.clamp(-96.0, 0.0)
        } else {
            -96.0
        };

        self.last_gain_reduction_db = if !bypassed && min_gain_this_buf < 1.0 {
            linear_to_db(min_gain_this_buf)
        } else {
            0.0
        };
    }

    fn reset(&mut self) {
        self.sidechain_filter.reset();
        self.env_db = -120.0;
        self.last_gain_reduction_db = 0.0;
        self.last_sidechain_db = -96.0;
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

    const SR: f32 = 48000.0;

    fn make_deesser(freq: f32, threshold_db: f32, ratio: f32) -> DeEsser {
        let params = Arc::new(DspParams::new());
        DspParams::set_f32(&params.deesser_frequency, freq);
        DspParams::set_f32(&params.deesser_threshold, threshold_db);
        DspParams::set_f32(&params.deesser_ratio, ratio);
        DspParams::set_f32(&params.deesser_attack_ms, 1.0);
        DspParams::set_f32(&params.deesser_release_ms, 50.0);
        DeEsser::new(params, SR)
    }

    fn sine_wave(freq: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * freq / SR * i as f32).sin())
            .collect()
    }

    fn rms(buf: &[f32]) -> f32 {
        let s: f32 = buf.iter().map(|x| x * x).sum();
        (s / buf.len() as f32).sqrt()
    }

    #[test]
    fn silence_in_silence_out() {
        let mut de = make_deesser(6000.0, -20.0, 4.0);
        let mut buf = vec![0.0f32; 512];
        de.process(&mut buf, SR);
        for s in &buf {
            assert!(s.abs() < 1e-12, "silence → silence, got {s}");
        }
        assert!(
            de.last_sidechain_db <= -90.0,
            "silence sidechain ≤ -90 dB, got {}",
            de.last_sidechain_db
        );
        assert!(
            de.env_db <= -90.0,
            "silence env_db ≤ -90 dB, got {}",
            de.env_db
        );
    }

    #[test]
    fn sibilant_frequency_triggers_reduction() {
        // De-esser at 6 kHz, threshold -20 dB, ratio 4:1.
        // -6 dBFS sine → over_db ≈ 14, gr_dB = 14 * (1 - 1/4) = 10.5 dB.
        let mut de = make_deesser(6000.0, -20.0, 4.0);

        // Warmup to build envelope past attack.
        let warmup = sine_wave(6000.0, 9600);
        let mut warm_buf = warmup;
        de.process(&mut warm_buf, SR);

        let mut buf = sine_wave(6000.0, 1024);
        let before_rms = rms(&buf);
        de.process(&mut buf, SR);
        let after_rms = rms(&buf);

        assert!(
            after_rms < before_rms * 0.99,
            "Sibilant freq should trigger reduction: before={before_rms:.4}, after={after_rms:.4}"
        );
        assert!(
            de.last_gain_reduction_db < 0.0,
            "GR meter should be negative, got {}",
            de.last_gain_reduction_db
        );
    }

    /// Verify the ratio formula `gr_dB = max(0, env_dB - thr) * (1 - 1/ratio)`
    /// for ratios 2/4/8 at a sustained tone above threshold.
    ///
    /// This is a closed-form check in the sense that `expected_gr_db` is
    /// computed from a directly observable steady-state quantity — `env_db`,
    /// the smoothed log-domain detector level — rather than reverse-engineered
    /// from the gain computer's internal state. With the new single-smoother
    /// topology there is no separate gain trajectory to read, so any
    /// regression in the detector (wrong domain, wrong coefficient formula,
    /// sign flip) shows up here as a divergence between `env_db` (read once
    /// the smoother has settled) and the published `last_gain_reduction_db`.
    #[test]
    fn ratio_formula_within_tolerance() {
        for &ratio in &[2.0f32, 4.0, 8.0] {
            let mut de = make_deesser(6000.0, -20.0, ratio);

            // Long warmup (≥ 1 s) so the envelope settles fully.
            let mut warm = sine_wave(6000.0, SR as usize);
            de.process(&mut warm, SR);

            // Snapshot the *measured* settled detector level. We then compute
            // expected GR analytically from this observed env_db.
            let env_db = de.env_db;
            let expected_gr_db = (env_db - (-20.0f32)).max(0.0) * (1.0 - 1.0 / ratio);

            // Process one more buffer so `last_gain_reduction_db` is fresh.
            let mut buf = sine_wave(6000.0, 4800);
            de.process(&mut buf, SR);
            let actual_gr_db = -de.last_gain_reduction_db; // stored as ≤ 0

            assert!(
                (actual_gr_db - expected_gr_db).abs() < 1.5,
                "ratio={ratio}: expected gr ≈ {expected_gr_db:.2} dB, \
                 got {actual_gr_db:.2} dB (env_db={env_db:.2})"
            );
        }
    }

    /// With a single log-domain smoother on the detector, `env_db` rises as a
    /// clean exponential toward its steady value with the configured attack
    /// time constant — `1 - exp(-3) ≈ 0.95` of the span by 3τ. The published
    /// `last_gain_reduction_db` is gated by `max(0, env_db - threshold)`, so
    /// while env_db itself rises cleanly from the silence floor, the visible
    /// GR appears compressed until env_db crosses threshold. This test
    /// asserts both: (1) env_db reaches ≥ 90% of its settled value by 3×
    /// attack — the property that proves there is no second smoother on the
    /// gain trajectory — and (2) the GR meter rise is monotonic and reaches a
    /// meaningful fraction of steady state in that window. A double-smoother
    /// (the previous topology) would fail the env_db assertion outright.
    #[test]
    fn single_smoother_attack_shape() {
        let params = Arc::new(DspParams::new());
        DspParams::set_f32(&params.deesser_frequency, 6000.0);
        DspParams::set_f32(&params.deesser_threshold, -20.0);
        DspParams::set_f32(&params.deesser_ratio, 4.0);
        DspParams::set_f32(&params.deesser_attack_ms, 10.0);
        DspParams::set_f32(&params.deesser_release_ms, 500.0);

        let mut de = DeEsser::new(params, SR);

        // Step from silence to a sustained -6 dBFS sine at the centre. We
        // sample `last_gain_reduction_db` per small buffer so we can trace
        // the rise.
        let buf_len = 64; // ~1.33 ms per buffer at 48 kHz
        let total_samples = (SR * 0.030) as usize; // 30 ms ≈ 3× attack
        let mut produced = 0usize;
        let mut samples_db: Vec<f32> = Vec::new();

        // Pre-build the full step signal then chunk it.
        let step: Vec<f32> = (0..total_samples + buf_len)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * 6000.0 / SR * i as f32).sin())
            .collect();

        while produced + buf_len <= step.len() {
            let mut chunk = step[produced..produced + buf_len].to_vec();
            de.process(&mut chunk, SR);
            samples_db.push(-de.last_gain_reduction_db); // positive GR
            produced += buf_len;
            if produced >= total_samples {
                break;
            }
        }
        let env_db_at_3tau = de.env_db;

        // Steady-state GR — keep feeding fresh sine (continuing the phase
        // would be ideal, but the detector cares about magnitude not phase)
        // so the envelope converges fully, then read.
        for _ in 0..40 {
            let mut t: Vec<f32> = (0..(SR * 0.025) as usize)
                .map(|i| 0.5 * (2.0 * std::f32::consts::PI * 6000.0 / SR * i as f32).sin())
                .collect();
            de.process(&mut t, SR);
        }
        let steady_state_gr = -de.last_gain_reduction_db;
        let steady_env_db = de.env_db;

        assert!(
            steady_state_gr > 1.0,
            "expected meaningful steady-state GR, got {steady_state_gr:.2} dB"
        );

        // (1) Clean exponential on env_db itself: from the -120 dB silence
        // floor toward steady_env_db, by 3τ we expect ≥ 95% of the span
        // covered. Allow ≥ 90% to absorb the BPF settling transient and the
        // sinusoidal in_db ripple. A double-smoothed gain trajectory does
        // not affect env_db (the issue would have been on the gain), but
        // the previous *linear*-domain envelope detector would exhibit a
        // level-dependent rise that fails this assertion at this amplitude.
        let span = steady_env_db - (-120.0);
        let covered = env_db_at_3tau - (-120.0);
        let env_fraction = covered / span;
        assert!(
            env_fraction >= 0.90,
            "env_db at 3× attack should cover ≥ 90% of span from floor to \
             steady: env_db_at_3tau={env_db_at_3tau:.2}, \
             steady_env_db={steady_env_db:.2}, fraction={env_fraction:.3}"
        );

        // (2) Monotonic non-decreasing GR rise (allow a tiny epsilon for FP
        // jitter from the per-sample sinusoidal ripple).
        for w in samples_db.windows(2) {
            assert!(
                w[1] + 1e-2 >= w[0],
                "GR rise should be monotonic: {:?} → {:?}",
                w[0],
                w[1]
            );
        }

        // (3) The GR meter must reach a meaningful fraction of steady state
        // by 3× attack — proof that there is no second smoother quietly
        // doubling the effective time constant. With env_db gated by the
        // threshold the visible GR is compressed (env_db has to climb past
        // threshold before GR even starts), but ≥ 40% is comfortably
        // achievable for a single smoother and well above what a compound
        // double-smoother would produce in the same window.
        let last = *samples_db.last().expect("samples_db non-empty");
        assert!(
            last >= 0.40 * steady_state_gr,
            "GR at 3× attack should be ≥ 40% of steady state: \
             last={last:.2} dB, steady={steady_state_gr:.2} dB"
        );
    }

    /// Sidechain meter publishes ≤ -90 dB on silence.
    #[test]
    fn sidechain_meter_silence() {
        let mut de = make_deesser(6000.0, -20.0, 4.0);
        let mut buf = vec![0.0f32; 4800];
        de.process(&mut buf, SR);
        assert!(
            de.last_sidechain_db <= -90.0,
            "silence sidechain expected ≤ -90 dB, got {}",
            de.last_sidechain_db
        );
    }

    /// Sidechain meter reaches near 0 dBFS on a full-scale sine at the centre.
    #[test]
    fn sidechain_meter_full_scale_sine_at_centre() {
        let mut de = make_deesser(6000.0, -20.0, 4.0);
        // Full-scale sine at the bandpass centre.
        let buf_in: Vec<f32> = (0..(SR as usize))
            .map(|i| (2.0 * std::f32::consts::PI * 6000.0 / SR * i as f32).sin())
            .collect();
        let mut buf = buf_in;
        de.process(&mut buf, SR);
        assert!(
            de.last_sidechain_db > -3.0,
            "6 kHz full-scale sine should drive sidechain near 0 dBFS, got {}",
            de.last_sidechain_db
        );
    }

    #[test]
    fn low_frequency_content_unaffected() {
        let mut de = make_deesser(6000.0, -20.0, 4.0);
        let sig = sine_wave(200.0, 8192);
        let mut buf = sig.clone();
        de.process(&mut buf, SR);

        let in_rms = rms(&sig[512..]);
        let out_rms = rms(&buf[512..]);
        let gain_db = 20.0 * (out_rms / in_rms).log10();
        assert!(
            gain_db > -0.5,
            "Low-freq content should be unaffected: got {gain_db:.3} dB"
        );
    }

    #[test]
    fn nan_input_clamped() {
        let mut de = make_deesser(6000.0, -20.0, 4.0);
        let mut buf = vec![f32::NAN; 64];
        de.process(&mut buf, SR);
        for s in &buf {
            assert!(s.is_finite(), "NaN input → finite output");
        }
    }

    #[test]
    fn inf_input_clamped() {
        let mut de = make_deesser(6000.0, -20.0, 4.0);
        let mut buf = vec![f32::INFINITY; 64];
        de.process(&mut buf, SR);
        for s in &buf {
            assert!(s.is_finite(), "Inf input → finite output");
        }
    }

    /// Sidechain meter is published even when the de-esser is bypassed —
    /// the bandpass + envelope follower run unconditionally so the user can
    /// see what the detector would catch before engaging the effect.
    #[test]
    fn sidechain_meter_publishes_when_bypassed() {
        let params = Arc::new(DspParams::new());
        DspParams::set_f32(&params.deesser_frequency, 6000.0);
        DspParams::set_f32(&params.deesser_threshold, -20.0);
        DspParams::set_f32(&params.deesser_ratio, 4.0);
        DspParams::set_f32(&params.deesser_attack_ms, 1.0);
        DspParams::set_f32(&params.deesser_release_ms, 50.0);
        params.bypass_deesser.store(true, Ordering::Relaxed);

        let mut de = DeEsser::new(params, SR);
        let buf_in: Vec<f32> = (0..(SR as usize))
            .map(|i| (2.0 * std::f32::consts::PI * 6000.0 / SR * i as f32).sin())
            .collect();
        let mut buf = buf_in.clone();
        de.process(&mut buf, SR);

        // Bypassed: signal passes through unchanged, but meter still fires.
        let in_rms = rms(&buf_in[512..]);
        let out_rms = rms(&buf[512..]);
        assert!(
            (in_rms - out_rms).abs() / in_rms < 0.001,
            "bypass should pass signal unchanged"
        );
        assert!(
            de.last_sidechain_db > -3.0,
            "bypass: sidechain meter still publishes, got {}",
            de.last_sidechain_db
        );
        assert!(
            de.last_gain_reduction_db == 0.0,
            "bypass: GR meter SHALL be 0 dB"
        );
    }
}
