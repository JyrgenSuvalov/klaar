/// Noise Gate processor.
///
/// Algorithm:
///   1. Envelope follower on absolute sample values (exponential attack/release)
///   2. Compare envelope (dB) to threshold
///   3. Target gain = 1.0 (open) if above threshold, else 10^(range_db/20) (closed)
///   4. Smooth gain change with its own attack/release coefficients → apply to signal
///
/// All operations are per-sample. No allocation on the audio thread.
use std::sync::Arc;
use crate::dsp::params::DspParams;
use crate::dsp::AudioProcessor;
use crate::dsp::biquad::sanitise;

pub struct NoiseGate {
    params: Arc<DspParams>,
    sample_rate: f32,

    /// Envelope follower state (linear amplitude)
    envelope: f32,
    /// Smoothed gain state (0.0..1.0)
    gain: f32,

    /// Cached gain reduction for metering (dB, ≤ 0)
    pub last_gain_reduction_db: f32,
}

impl NoiseGate {
    pub fn new(params: Arc<DspParams>, sample_rate: f32) -> Self {
        Self {
            params,
            sample_rate,
            envelope: 0.0,
            gain: 1.0, // Start fully open — closes only when envelope falls below threshold
            last_gain_reduction_db: 0.0,
        }
    }

    /// Compute exponential envelope coefficient from time in milliseconds.
    #[inline]
    fn time_coeff(time_ms: f32, sr: f32) -> f32 {
        let time_s = (time_ms * 0.001).max(1e-6);
        (-1.0 / (time_s * sr)).exp()
    }
}

impl AudioProcessor for NoiseGate {
    fn process(&mut self, buffer: &mut [f32], sample_rate: f32) {
        self.sample_rate = sample_rate;

        // Read parameters once per buffer (atomic load, not per-sample)
        let threshold_db = self.params.gate_threshold();
        let attack_coeff = Self::time_coeff(self.params.gate_attack_ms(), sample_rate);
        let release_coeff = Self::time_coeff(self.params.gate_release_ms(), sample_rate);
        let range_db = self.params.gate_range_db().min(0.0); // ensure ≤ 0

        let threshold_linear = db_to_linear(threshold_db);
        let range_linear = db_to_linear(range_db); // target gain when gate is closed

        // Gain attack/release smoothing coefficients (using same times)
        // For the gain signal we want attack to open fast and release to close slow
        // (opposite nomenclature to the envelope)
        let gain_attack_coeff = attack_coeff;
        let gain_release_coeff = release_coeff;

        let mut min_gain: f32 = 1.0; // track minimum gain this buffer for metering

        for s in buffer.iter_mut() {
            let x = sanitise(*s);

            // ── Envelope follower ──────────────────────────────────────────
            let abs_x = x.abs();
            if abs_x > self.envelope {
                self.envelope = attack_coeff * self.envelope + (1.0 - attack_coeff) * abs_x;
            } else {
                self.envelope = release_coeff * self.envelope + (1.0 - release_coeff) * abs_x;
            }

            // ── Target gain from gate state ────────────────────────────────
            let target_gain = if self.envelope >= threshold_linear {
                1.0f32 // gate open
            } else {
                range_linear // gate closed → attenuate by range
            };

            // ── Smooth gain ────────────────────────────────────────────────
            if target_gain > self.gain {
                // Opening: use attack coefficient
                self.gain = gain_attack_coeff * self.gain + (1.0 - gain_attack_coeff) * target_gain;
            } else {
                // Closing: use release coefficient
                self.gain = gain_release_coeff * self.gain + (1.0 - gain_release_coeff) * target_gain;
            }

            min_gain = min_gain.min(self.gain);
            *s = x * self.gain;
        }

        // Metering: gain reduction in dB (≤ 0)
        self.last_gain_reduction_db = if min_gain < 1.0 {
            linear_to_db(min_gain) // already ≤ 0
        } else {
            0.0
        };
    }

    fn reset(&mut self) {
        self.envelope = 0.0;
        self.gain = 1.0;
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
    if linear <= 0.0 {
        -96.0
    } else {
        20.0 * linear.log10()
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    const SR: f32 = 48000.0;

    fn make_gate(threshold_db: f32, range_db: f32, attack_ms: f32, release_ms: f32) -> NoiseGate {
        let params = Arc::new(DspParams::new());
        DspParams::set_f32(&params.gate_threshold, threshold_db);
        DspParams::set_f32(&params.gate_range, range_db);
        DspParams::set_f32(&params.gate_attack, attack_ms);
        DspParams::set_f32(&params.gate_release, release_ms);
        NoiseGate::new(params, SR)
    }

    fn rms(buf: &[f32]) -> f32 {
        let s: f32 = buf.iter().map(|x| x * x).sum();
        (s / buf.len() as f32).sqrt()
    }

    #[test]
    fn silence_in_silence_out() {
        let mut gate = make_gate(-40.0, -60.0, 1.0, 100.0);
        let mut buf = vec![0.0f32; 512];
        gate.process(&mut buf, SR);
        for s in &buf {
            assert!(s.abs() < 1e-12, "silence → silence, got {s}");
        }
    }

    #[test]
    fn signal_above_threshold_passes() {
        // Gate threshold at -40 dB; signal at 0 dBFS (amplitude 1.0)
        // After settling, signal should pass at near unity gain
        let mut gate = make_gate(-40.0, -60.0, 1.0, 500.0);

        // Pre-fill with signal to open the gate
        let mut warmup = vec![1.0f32; 4800]; // 100 ms at 48 kHz
        gate.process(&mut warmup, SR);

        let mut buf = vec![1.0f32; 512];
        gate.process(&mut buf, SR);

        let out_rms = rms(&buf);
        // Expect close to unity (0 dBFS)
        assert!(out_rms > 0.99, "Above threshold: expected ~1.0, got {out_rms}");
    }

    #[test]
    fn signal_below_threshold_attenuated_by_range() {
        // Gate threshold at -20 dB; signal at -60 dBFS (very quiet)
        // Range = -40 dB → closed gate applies -40 dB attenuation
        let signal_linear = db_to_linear(-60.0); // well below threshold
        let range_db = -40.0_f32;

        let mut gate = make_gate(-20.0, range_db, 1.0, 1.0);

        // Process several buffers to let envelope stabilise at closed state
        let mut buf: Vec<f32> = vec![signal_linear; 9600];
        gate.process(&mut buf, SR);

        // Second buffer: gate should now be fully closed
        let mut buf2: Vec<f32> = vec![signal_linear; 1024];
        gate.process(&mut buf2, SR);

        let expected_gain = db_to_linear(range_db);
        let expected_output = signal_linear * expected_gain;
        let actual_rms = rms(&buf2);

        // Allow ±1 dB tolerance for smoothing
        let expected_db = 20.0 * (expected_output + 1e-30).log10();
        let actual_db = 20.0 * (actual_rms + 1e-30).log10();
        assert_abs_diff_eq!(actual_db, expected_db, epsilon = 1.0);
    }

    #[test]
    fn nan_input_clamped() {
        let mut gate = make_gate(-40.0, -60.0, 1.0, 100.0);
        let mut buf = vec![f32::NAN; 64];
        gate.process(&mut buf, SR);
        for s in &buf {
            assert!(s.is_finite(), "NaN input → finite output, got {s}");
        }
    }

    #[test]
    fn inf_input_clamped() {
        let mut gate = make_gate(-40.0, -60.0, 1.0, 100.0);
        let mut buf = vec![f32::INFINITY; 64];
        gate.process(&mut buf, SR);
        for s in &buf {
            assert!(s.is_finite(), "Inf input → finite output, got {s}");
        }
    }

    #[test]
    fn attack_smoothing_no_instant_open() {
        // With a very slow attack (100 ms), the gate should NOT re-open immediately
        // after having been closed by a long period of silence.
        let signal = db_to_linear(-10.0); // above -40 dB threshold

        // Step 1: drive the gate closed with silence (fast release = 1 ms to ensure closure)
        let mut gate = make_gate(-40.0, -60.0, 100.0, 1.0);
        let mut silence = vec![0.0f32; 9600]; // 200 ms of silence
        gate.process(&mut silence, SR);

        // Step 2: gate is now closed — feed signal above threshold for one short buffer (~5.3 ms)
        let mut buf = vec![signal; 256];
        gate.process(&mut buf, SR);

        // With 100 ms attack, after only ~5.3 ms the gate should still be well below unity.
        // First 16 samples should be below full amplitude (gate is still opening).
        let first_rms = rms(&buf[..16]);
        assert!(first_rms < signal * 0.99, "Attack should not be instant: first_rms={first_rms:.4}");
    }

    #[test]
    fn gain_reduction_metering_updates() {
        // When signal is below threshold, gain reduction should be non-zero
        let mut gate = make_gate(-20.0, -40.0, 1.0, 1.0);
        // Process quiet signal to close the gate
        let mut buf = vec![db_to_linear(-60.0); 9600];
        gate.process(&mut buf, SR);
        let mut buf2 = vec![db_to_linear(-60.0); 512];
        gate.process(&mut buf2, SR);
        // Gate reduction should be non-zero (gate is closed)
        assert!(
            gate.last_gain_reduction_db < 0.0,
            "Gate reduction should be negative dB when closed, got {}",
            gate.last_gain_reduction_db
        );
    }
}
