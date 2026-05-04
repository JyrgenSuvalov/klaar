/// 8-band Parametric EQ processor.
///
/// Each band has 4 pre-allocated BiquadFilter stages. For standard filters
/// (bell, HP, LP, shelves) only stage 0 is active. For 48 dB/oct filters
/// (HighPass48, LowPass48) all 4 stages run as a Butterworth cascade with
/// per-stage Q values scaled proportionally by the user's Q setting.
///
/// Parameters are read from Arc<DspParams> each buffer and passed to
/// BiquadFilter::update().
use std::sync::Arc;
use crate::dsp::params::DspParams;
use crate::dsp::biquad::{BiquadFilter, BUTTERWORTH_8TH_ORDER_QS, BUTTERWORTH_REF_Q};
use crate::dsp::AudioProcessor;

pub struct ParametricEq {
    params: Arc<DspParams>,
    /// 8 bands × 4 stages per band. For single-stage filters, only [band][0] is used.
    bands: [[BiquadFilter; 4]; 8],
}

impl ParametricEq {
    pub fn new(params: Arc<DspParams>, _sample_rate: f32) -> Self {
        Self {
            params,
            bands: std::array::from_fn(|_| std::array::from_fn(|_| BiquadFilter::new())),
        }
    }

    /// Compute the per-stage Q values for 48 dB/oct Butterworth cascade.
    ///
    /// Stages 0–2 use fixed Butterworth Q values for clean slope rolloff.
    /// Stage 3 scales its Butterworth Q proportionally to the user's Q:
    ///   stage_3_q = BUTTERWORTH_Q[3] × (user_q / BUTTERWORTH_REF_Q)
    ///
    /// At user_q ≈ 0.707 (Butterworth reference), all 4 stages form a
    /// true 8th-order Butterworth — maximally flat passband, 48 dB/oct.
    /// Above 0.707, stage 3's Q increases, adding a resonant peak at cutoff.
    /// Below 0.707, the response becomes more overdamped (softer knee).
    #[inline]
    fn stage_qs(user_q: f32) -> [f32; 4] {
        let resonance_scale = user_q / BUTTERWORTH_REF_Q;
        [
            BUTTERWORTH_8TH_ORDER_QS[0],
            BUTTERWORTH_8TH_ORDER_QS[1],
            BUTTERWORTH_8TH_ORDER_QS[2],
            BUTTERWORTH_8TH_ORDER_QS[3] * resonance_scale,
        ]
    }
}

impl AudioProcessor for ParametricEq {
    fn process(&mut self, buffer: &mut [f32], sample_rate: f32) {
        for (i, band_stages) in self.bands.iter_mut().enumerate() {
            let band = &self.params.eq_bands[i];

            if !band.get_enabled() {
                continue; // Skip disabled bands entirely
            }

            let filter_type = band.get_filter_type();
            let freq = band.get_frequency();
            let gain = band.get_gain();
            let q = band.get_q();
            let stage_count = filter_type.stage_count();
            let base_type = filter_type.base_type();

            if stage_count == 1 {
                // Single biquad stage (bell, HP, LP, shelves)
                band_stages[0].update(base_type, freq, gain, q, sample_rate);
                band_stages[0].process_buffer(buffer);
            } else {
                // 48 dB/oct: 3 fixed Butterworth stages + 1 user-Q resonance stage
                let qs = Self::stage_qs(q);
                for (stage_idx, stage_q) in qs.iter().enumerate() {
                    band_stages[stage_idx].update(base_type, freq, gain, *stage_q, sample_rate);
                    band_stages[stage_idx].process_buffer(buffer);
                }
            }
        }
    }

    fn reset(&mut self) {
        for band_stages in self.bands.iter_mut() {
            for stage in band_stages.iter_mut() {
                stage.reset();
            }
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::params::{FilterType, DspParams};
    use std::sync::atomic::Ordering;

    const SR: f32 = 48000.0;

    fn sine_wave(freq: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq / SR * i as f32).sin())
            .collect()
    }

    fn rms(buf: &[f32]) -> f32 {
        let s: f32 = buf.iter().map(|x| x * x).sum();
        (s / buf.len() as f32).sqrt()
    }

    fn to_db(linear: f32) -> f32 {
        20.0 * linear.log10()
    }

    #[test]
    fn all_bands_flat_is_passthrough() {
        // Default params: all bands at 0 dB gain → passthrough
        let params = Arc::new(DspParams::new());
        let mut eq = ParametricEq::new(params, SR);

        let sig = sine_wave(1000.0, 4096);
        let mut buf = sig.clone();
        eq.process(&mut buf, SR);

        // All zeros in, gain should be ~0 dB after settlement
        let in_rms = rms(&sig[512..]);
        let out_rms = rms(&buf[512..]);
        let gain_db = to_db(out_rms / in_rms);
        assert!(
            gain_db.abs() < 0.1,
            "Flat EQ should be transparent: got {gain_db:.3} dB"
        );
    }

    #[test]
    fn disabled_band_has_no_effect() {
        let params = Arc::new(DspParams::new());
        // Disable band 0, set it to +12 dB at 1 kHz
        params.eq_bands[0].enabled.store(0.0f32.to_bits(), Ordering::Relaxed);
        params.eq_bands[0].frequency.store(1000.0f32.to_bits(), Ordering::Relaxed);
        params.eq_bands[0].gain.store(12.0f32.to_bits(), Ordering::Relaxed);
        params.eq_bands[0].q.store(4.0f32.to_bits(), Ordering::Relaxed);

        let mut eq = ParametricEq::new(params, SR);
        let sig = sine_wave(1000.0, 4096);
        let mut buf = sig.clone();
        eq.process(&mut buf, SR);

        let in_rms = rms(&sig[512..]);
        let out_rms = rms(&buf[512..]);
        let gain_db = to_db(out_rms / in_rms);
        assert!(
            gain_db.abs() < 0.1,
            "Disabled band must not affect signal: got {gain_db:.3} dB"
        );
    }

    #[test]
    fn enabled_band_boosts_at_centre_frequency() {
        let params = Arc::new(DspParams::new());
        // Disable all other bands, enable band 3 at +6 dB / 1 kHz / Q 4
        for i in 0..8 {
            params.eq_bands[i].enabled.store(0.0f32.to_bits(), Ordering::Relaxed);
        }
        params.eq_bands[3].enabled.store(1.0f32.to_bits(), Ordering::Relaxed);
        params.eq_bands[3].filter_type.store(FilterType::Bell.to_f32().to_bits(), Ordering::Relaxed);
        params.eq_bands[3].frequency.store(1000.0f32.to_bits(), Ordering::Relaxed);
        params.eq_bands[3].gain.store(6.0f32.to_bits(), Ordering::Relaxed);
        params.eq_bands[3].q.store(4.0f32.to_bits(), Ordering::Relaxed);

        let mut eq = ParametricEq::new(params, SR);
        let sig = sine_wave(1000.0, 8192);
        let mut buf = sig.clone();
        eq.process(&mut buf, SR);

        let gain_db = to_db(rms(&buf[512..]) / rms(&sig[512..]));
        assert!(
            (gain_db - 6.0).abs() < 0.5,
            "Band +6 dB at 1 kHz: got {gain_db:.3} dB"
        );
    }

    #[test]
    fn multi_band_response_is_additive() {
        // Two bands both boosting the same 1 kHz signal: +3 dB + +3 dB ≈ +6 dB
        let params = Arc::new(DspParams::new());
        for i in 0..8 {
            params.eq_bands[i].enabled.store(0.0f32.to_bits(), Ordering::Relaxed);
        }
        for i in 0..2 {
            params.eq_bands[i].enabled.store(1.0f32.to_bits(), Ordering::Relaxed);
            params.eq_bands[i].filter_type.store(FilterType::Bell.to_f32().to_bits(), Ordering::Relaxed);
            params.eq_bands[i].frequency.store(1000.0f32.to_bits(), Ordering::Relaxed);
            params.eq_bands[i].gain.store(3.0f32.to_bits(), Ordering::Relaxed);
            params.eq_bands[i].q.store(4.0f32.to_bits(), Ordering::Relaxed);
        }

        let mut eq = ParametricEq::new(params, SR);
        let sig = sine_wave(1000.0, 8192);
        let mut buf = sig.clone();
        eq.process(&mut buf, SR);

        let gain_db = to_db(rms(&buf[512..]) / rms(&sig[512..]));
        // Allow generous tolerance for combined phase/amplitude interaction
        assert!(
            (gain_db - 6.0).abs() < 1.5,
            "Two +3 dB bands should give ~+6 dB: got {gain_db:.3} dB"
        );
    }

    #[test]
    fn silence_in_silence_out() {
        let params = Arc::new(DspParams::new());
        let mut eq = ParametricEq::new(params, SR);
        let mut buf = vec![0.0f32; 512];
        eq.process(&mut buf, SR);
        for s in &buf {
            assert!(s.abs() < 1e-12, "silence → silence, got {s}");
        }
    }

    // ── 48 dB/oct filter tests ──────────────────────────────────────────────

    #[test]
    fn highpass48_attenuates_more_steeply_than_highpass12() {
        // Compare HP12 vs HP48 at 1 kHz cutoff, measuring attenuation at 100 Hz.
        // HP12 = 12 dB/oct = ~20 dB at 1 decade below.
        // HP48 = 48 dB/oct = ~80 dB at 1 decade below.
        let params_12 = Arc::new(DspParams::new());
        let params_48 = Arc::new(DspParams::new());

        for p in [&params_12, &params_48] {
            for i in 0..8 {
                p.eq_bands[i].enabled.store(0.0f32.to_bits(), Ordering::Relaxed);
            }
            p.eq_bands[0].enabled.store(1.0f32.to_bits(), Ordering::Relaxed);
            p.eq_bands[0].frequency.store(1000.0f32.to_bits(), Ordering::Relaxed);
            p.eq_bands[0].gain.store(0.0f32.to_bits(), Ordering::Relaxed);
            p.eq_bands[0].q.store(0.707f32.to_bits(), Ordering::Relaxed);
        }

        params_12.eq_bands[0].filter_type.store(FilterType::HighPass.to_f32().to_bits(), Ordering::Relaxed);
        params_48.eq_bands[0].filter_type.store(FilterType::HighPass48.to_f32().to_bits(), Ordering::Relaxed);

        let sig = sine_wave(100.0, 16384);

        let mut eq12 = ParametricEq::new(params_12, SR);
        let mut buf12 = sig.clone();
        eq12.process(&mut buf12, SR);

        let mut eq48 = ParametricEq::new(params_48, SR);
        let mut buf48 = sig.clone();
        eq48.process(&mut buf48, SR);

        let atten_12 = to_db(rms(&buf12[2048..]) / rms(&sig[2048..]));
        let atten_48 = to_db(rms(&buf48[2048..]) / rms(&sig[2048..]));

        // HP48 should attenuate significantly more than HP12
        assert!(
            atten_48 < atten_12 - 20.0,
            "HP48 should attenuate much more than HP12 at 100 Hz: HP12={atten_12:.1} dB, HP48={atten_48:.1} dB"
        );
    }

    #[test]
    fn lowpass48_attenuates_more_steeply_than_lowpass12() {
        let params_12 = Arc::new(DspParams::new());
        let params_48 = Arc::new(DspParams::new());

        for p in [&params_12, &params_48] {
            for i in 0..8 {
                p.eq_bands[i].enabled.store(0.0f32.to_bits(), Ordering::Relaxed);
            }
            p.eq_bands[0].enabled.store(1.0f32.to_bits(), Ordering::Relaxed);
            p.eq_bands[0].frequency.store(1000.0f32.to_bits(), Ordering::Relaxed);
            p.eq_bands[0].gain.store(0.0f32.to_bits(), Ordering::Relaxed);
            p.eq_bands[0].q.store(0.707f32.to_bits(), Ordering::Relaxed);
        }

        params_12.eq_bands[0].filter_type.store(FilterType::LowPass.to_f32().to_bits(), Ordering::Relaxed);
        params_48.eq_bands[0].filter_type.store(FilterType::LowPass48.to_f32().to_bits(), Ordering::Relaxed);

        let sig = sine_wave(10000.0, 16384);

        let mut eq12 = ParametricEq::new(params_12, SR);
        let mut buf12 = sig.clone();
        eq12.process(&mut buf12, SR);

        let mut eq48 = ParametricEq::new(params_48, SR);
        let mut buf48 = sig.clone();
        eq48.process(&mut buf48, SR);

        let atten_12 = to_db(rms(&buf12[2048..]) / rms(&sig[2048..]));
        let atten_48 = to_db(rms(&buf48[2048..]) / rms(&sig[2048..]));

        assert!(
            atten_48 < atten_12 - 20.0,
            "LP48 should attenuate much more than LP12 at 10 kHz: LP12={atten_12:.1} dB, LP48={atten_48:.1} dB"
        );
    }

    #[test]
    fn highpass48_passes_above_cutoff() {
        let params = Arc::new(DspParams::new());
        for i in 0..8 {
            params.eq_bands[i].enabled.store(0.0f32.to_bits(), Ordering::Relaxed);
        }
        params.eq_bands[0].enabled.store(1.0f32.to_bits(), Ordering::Relaxed);
        params.eq_bands[0].filter_type.store(FilterType::HighPass48.to_f32().to_bits(), Ordering::Relaxed);
        params.eq_bands[0].frequency.store(200.0f32.to_bits(), Ordering::Relaxed);
        params.eq_bands[0].q.store(0.707f32.to_bits(), Ordering::Relaxed);

        let mut eq = ParametricEq::new(params, SR);
        let sig = sine_wave(8000.0, 8192);
        let mut buf = sig.clone();
        eq.process(&mut buf, SR);

        let gain_db = to_db(rms(&buf[1024..]) / rms(&sig[1024..]));
        assert!(gain_db > -1.0, "HP48 should pass 8 kHz near unity: got {gain_db:.2} dB");
    }

    #[test]
    fn lowpass48_passes_below_cutoff() {
        let params = Arc::new(DspParams::new());
        for i in 0..8 {
            params.eq_bands[i].enabled.store(0.0f32.to_bits(), Ordering::Relaxed);
        }
        params.eq_bands[0].enabled.store(1.0f32.to_bits(), Ordering::Relaxed);
        params.eq_bands[0].filter_type.store(FilterType::LowPass48.to_f32().to_bits(), Ordering::Relaxed);
        params.eq_bands[0].frequency.store(8000.0f32.to_bits(), Ordering::Relaxed);
        params.eq_bands[0].q.store(0.707f32.to_bits(), Ordering::Relaxed);

        let mut eq = ParametricEq::new(params, SR);
        let sig = sine_wave(100.0, 8192);
        let mut buf = sig.clone();
        eq.process(&mut buf, SR);

        let gain_db = to_db(rms(&buf[1024..]) / rms(&sig[1024..]));
        assert!(gain_db > -1.0, "LP48 should pass 100 Hz near unity: got {gain_db:.2} dB");
    }
}
