/// DSP module for Klaar.
///
/// Fixed processing chain: Noise Gate → Parametric EQ → De-esser → Compressor → Limiter
///
/// All processors are zero-allocation and lock-free on the audio thread.
/// Parameters are communicated via `Arc<DspParams>` (AtomicU32 / AtomicBool fields).
/// Bypass flags live in `DspParams` so the audio callback and command handlers
/// both access them without any mutex — pure atomic reads/writes.
pub mod biquad;
#[cfg(test)] pub mod chain_tests;
pub mod compressor;
pub mod deesser;
pub mod limiter;
pub mod noise_gate;
pub mod params;
pub mod parametric_eq;
pub mod spectrum;

pub use params::DspParams;

use std::sync::atomic::Ordering;
use std::sync::Arc;

use compressor::Compressor;
use deesser::DeEsser;
use limiter::Limiter;
use noise_gate::NoiseGate;
use parametric_eq::ParametricEq;
use spectrum::SpectrumWriter;

// ────────────────────────────────────────────────────────────────────────────
// Mute stage (post-limiter)
// ────────────────────────────────────────────────────────────────────────────

/// Time constant for the mute-on (un-→mute) ramp. ~2 ms gives instant
/// cough-suppression with no audible click.
pub const TAU_MUTE_ON: f32 = 0.002;

/// Time constant for the mute-off (mute-→un) ramp. ~10 ms gives a gentle,
/// pop-free re-entry.
pub const TAU_MUTE_OFF: f32 = 0.010;

/// Epsilon below which `mute_current` snaps exactly to its target so the
/// settled state is bit-identical to a no-op multiply (×1.0) or hard mute
/// (×0.0).
///
/// The design doc cites `1e-6`, but f32 precision near 1.0 has a ULP of
/// ≈1.19e-7, and the per-sample increment of the one-pole stalls below that
/// ULP once the residual reaches ~3e-5. We use `1e-4` instead: still −80 dB
/// (well below audibility, and below the f32 noise floor of any realistic
/// downstream op), but comfortably above the f32 stall floor on the mute-off
/// ramp so the settled state is exact, not asymptotic. The tighter `1e-6`
/// would never trigger in practice — values would plateau at ~1−1.5e-5 on
/// mute-off and the spec's "snap to exact target" requirement would silently
/// fail. See `mute_current_snaps_to_exact_target` test.
const MUTE_SNAP_EPS: f32 = 1e-4;

/// One-pole coefficient for time constant `tau` (seconds) at `sample_rate` (Hz).
///
/// Derivation: a one-pole IIR with pole `a` reaches 63 % of step in `τ` when
/// `a = 1 - exp(-1/(τ·fs))`. The exp() is precomputed in
/// `ProcessorChain::cache_mute_coeffs` whenever the sample rate changes, so
/// the per-sample hot path is one branch + one mul + one add.
#[inline(always)]
fn mute_coeff(tau: f32, sample_rate: f32) -> f32 {
    1.0 - (-1.0 / (tau * sample_rate)).exp()
}

// ────────────────────────────────────────────────────────────────────────────
// AudioProcessor trait
// ────────────────────────────────────────────────────────────────────────────

/// Common interface for all DSP effect processors.
///
/// Implementations MUST be:
/// - Zero-allocation (no heap alloc on the audio thread)
/// - Lock-free (no mutex, no channel send)
/// - Bounded in execution time
pub trait AudioProcessor: Send {
    /// Process `buffer` in-place at the given sample rate.
    fn process(&mut self, buffer: &mut [f32], sample_rate: f32);
    /// Reset all internal state (delay lines, envelope followers, etc.).
    /// Called when the audio engine restarts after a device change.
    #[allow(dead_code)]
    fn reset(&mut self);
}

// ────────────────────────────────────────────────────────────────────────────
// ProcessorChain
// ────────────────────────────────────────────────────────────────────────────

/// Fixed-order DSP processing chain.
///
/// Order: Noise Gate → Parametric EQ → De-esser → Compressor → Limiter
///
/// Bypass flags live in `Arc<DspParams>` — written by the UI thread via IPC,
/// read by the audio thread via `Ordering::Relaxed` loads. No locking.
pub struct ProcessorChain {
    params: Arc<DspParams>,
    pub noise_gate: NoiseGate,
    pub eq: ParametricEq,
    pub deesser: DeEsser,
    pub compressor: Compressor,
    pub limiter: Limiter,
    /// Spectrum analyzer writer — taps audio post-gate, pre-EQ.
    /// `None` until set via `set_spectrum_writer()`.
    spectrum_writer: Option<SpectrumWriter>,

    // ── Mute stage (audio-thread-private state) ─────────────────────────────
    /// Current smoothed mute gain. Owned exclusively by the audio thread —
    /// no atomic, no lock. Initialised to 1.0 (unmuted).
    mute_current: f32,
    /// Sample rate the mute coefficients were last computed for. Recomputed
    /// in-place on the audio thread when this differs from the buffer's SR.
    mute_coeff_sr: f32,
    /// Cached one-pole coefficient for the 2 ms mute-on ramp.
    mute_coeff_on: f32,
    /// Cached one-pole coefficient for the 10 ms mute-off ramp.
    mute_coeff_off: f32,
}

impl ProcessorChain {
    pub fn new(params: Arc<DspParams>, sample_rate: f32) -> Self {
        Self {
            noise_gate: NoiseGate::new(params.clone(), sample_rate),
            eq: ParametricEq::new(params.clone(), sample_rate),
            deesser: DeEsser::new(params.clone(), sample_rate),
            compressor: Compressor::new(params.clone(), sample_rate),
            limiter: Limiter::new(params.clone(), sample_rate),
            spectrum_writer: None,
            params,
            // Mute stage starts unmuted and settled — first sample is a no-op.
            mute_current: 1.0,
            mute_coeff_sr: sample_rate,
            mute_coeff_on: mute_coeff(TAU_MUTE_ON, sample_rate),
            mute_coeff_off: mute_coeff(TAU_MUTE_OFF, sample_rate),
        }
    }

    /// Recompute the mute coefficients if the sample rate has changed.
    /// Called on the audio thread before the per-sample loop — the branch is
    /// only taken when `sample_rate` differs from the cached value (typically
    /// on engine restart after a device change), so it does not cost on the
    /// hot path.
    #[inline(always)]
    fn cache_mute_coeffs(&mut self, sample_rate: f32) {
        if sample_rate != self.mute_coeff_sr {
            self.mute_coeff_sr = sample_rate;
            self.mute_coeff_on = mute_coeff(TAU_MUTE_ON, sample_rate);
            self.mute_coeff_off = mute_coeff(TAU_MUTE_OFF, sample_rate);
        }
    }

    /// Apply the smoothed mute gain to every sample in the buffer in-place.
    ///
    /// Real-time safety: no allocation, no lock, no log, no syscall. Reads
    /// `mute_target` once per sample with `Relaxed` ordering, then runs a
    /// branch-free one-pole smoothing step + a snap-to-target check. When
    /// the mute is settled to 1.0 the multiply is mathematically a no-op
    /// (×1.0), giving bit-identical output to the same chain without the
    /// stage. See `mute-control` capability spec.
    #[inline(always)]
    fn apply_mute(&mut self, buffer: &mut [f32]) {
        // Snapshot the atomic for the buffer. The intent flag changes at most
        // a few times per second from the UI thread; per-sample reads are also
        // fine and remain lock-free, but a per-buffer snapshot avoids
        // hammering the cache line.
        let target = DspParams::get_f32(&self.params.mute_target);
        let mut current = self.mute_current;
        for s in buffer.iter_mut() {
            // Asymmetric τ: mute-on (target < current) is the fast 2 ms ramp;
            // mute-off (target > current) is the gentler 10 ms ramp.
            let coeff = if target < current {
                self.mute_coeff_on
            } else {
                self.mute_coeff_off
            };
            current += coeff * (target - current);
            // Epsilon snap so the settled state is exact, avoiding asymptotic
            // residuals that would prevent a true bit-identical no-op.
            if (current - target).abs() < MUTE_SNAP_EPS {
                current = target;
            }
            *s *= current;
        }
        self.mute_current = current;
    }

    /// Set the spectrum writer for post-gate, pre-EQ frequency analysis.
    pub fn set_spectrum_writer(&mut self, writer: SpectrumWriter) {
        self.spectrum_writer = Some(writer);
    }

    /// Process `buffer` through the full chain in fixed order.
    ///
    /// Bypass flags are read from `DspParams` atomics — no allocation, no locking.
    #[inline]
    pub fn process(&mut self, buffer: &mut [f32], sample_rate: f32) {
        let p = &self.params;

        if !p.bypass_gate.load(Ordering::Relaxed) {
            self.noise_gate.process(buffer, sample_rate);
        }

        // Spectrum tap: post-gate, pre-EQ — shows what the user is shaping
        if let Some(ref writer) = self.spectrum_writer {
            writer.push_samples(buffer);
        }

        if !p.bypass_eq.load(Ordering::Relaxed) {
            self.eq.process(buffer, sample_rate);
        }
        // De-esser runs unconditionally. The processor handles bypass
        // internally so the bandpass + envelope follower can keep the
        // sidechain meter live (and gain state hot) even when bypassed.
        self.deesser.process(buffer, sample_rate);
        if !p.bypass_compressor.load(Ordering::Relaxed) {
            self.compressor.process(buffer, sample_rate);
        }
        if !p.bypass_limiter.load(Ordering::Relaxed) {
            self.limiter.process(buffer, sample_rate);
        }

        // Final smoothed mute gain — runs unconditionally as the very last
        // operation on the output buffer. Cannot be bypassed (the multiply
        // is a no-op when settled to unmuted = 1.0).
        self.cache_mute_coeffs(sample_rate);
        self.apply_mute(buffer);
    }

    /// Reset all processors in the chain.
    /// Called when the audio engine restarts after a device change.
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.noise_gate.reset();
        self.eq.reset();
        self.deesser.reset();
        self.compressor.reset();
        self.limiter.reset();
        // Mute snaps back to the unmuted start state so the next stream
        // begins as a no-op multiply. The intent flag in `DspParams` is
        // intentionally untouched — engine restart does not change intent.
        self.mute_current = 1.0;
    }
}

#[cfg(test)]
mod mute_tests {
    use super::*;

    /// Build a chain with all DSP bypassed so the only operation that touches
    /// the buffer is the post-limiter mute stage.
    fn bypassed_chain(sr: f32) -> ProcessorChain {
        let params = Arc::new(DspParams::new());
        params.bypass_gate.store(true, Ordering::Relaxed);
        params.bypass_eq.store(true, Ordering::Relaxed);
        params.bypass_deesser.store(true, Ordering::Relaxed);
        params.bypass_compressor.store(true, Ordering::Relaxed);
        params.bypass_limiter.store(true, Ordering::Relaxed);
        ProcessorChain::new(params, sr)
    }

    /// 1.6 — mute-on ramp settles to < 1 % of full level and is monotonically
    /// non-increasing. With τ=2 ms a one-pole takes ~10 ms (5 τ) to reach
    /// −40 dB; we measure at 12 ms so the < 0.01 assertion is comfortably
    /// satisfied without flake.
    #[test]
    fn mute_on_ramp_reaches_silence_and_is_monotonic() {
        let sr = 48_000.0_f32;
        let mut chain = bypassed_chain(sr);
        chain.params.set_muted(true);

        let n = (sr * 0.012) as usize; // 12 ms (~6 τ)
        let mut buf = vec![1.0_f32; n];
        chain.process(&mut buf, sr);

        // Final sample is well below 1 % of full level.
        assert!(
            buf[n - 1].abs() < 0.01,
            "expected output < 0.01 after 5 ms mute-on ramp, got {}",
            buf[n - 1]
        );

        // Monotonically non-increasing in absolute value during the ramp.
        for i in 1..n {
            assert!(
                buf[i].abs() <= buf[i - 1].abs() + 1e-7,
                "ramp not monotonic at i={i}: {} > {}",
                buf[i],
                buf[i - 1]
            );
        }
    }

    /// 1.7 — mute-off ramp reaches near-unity within ~25 ms (τ=10 ms gives
    /// >99 % settling in 2.5 τ ≈ 25 ms) and is monotonically non-decreasing.
    #[test]
    fn mute_off_ramp_reaches_unity_and_is_monotonic() {
        let sr = 48_000.0_f32;
        let mut chain = bypassed_chain(sr);
        // Start fully muted so the unmute ramps from 0 → 1.
        chain.params.set_muted(true);
        chain.mute_current = 0.0;
        chain.params.set_muted(false);

        let n = (sr * 0.060) as usize; // 60 ms (~6 τ)
        let mut buf = vec![1.0_f32; n];
        chain.process(&mut buf, sr);

        assert!(
            buf[n - 1] > 0.99,
            "expected output > 0.99 after 25 ms mute-off ramp, got {}",
            buf[n - 1]
        );

        for i in 1..n {
            assert!(
                buf[i] >= buf[i - 1] - 1e-7,
                "ramp not monotonic at i={i}: {} < {}",
                buf[i],
                buf[i - 1]
            );
        }
    }

    /// 1.8 — `mute_current` snaps to exact 0.0 / 1.0 once it crosses the
    /// epsilon settle threshold. No asymptotic residual.
    #[test]
    fn mute_current_snaps_to_exact_target() {
        let sr = 48_000.0_f32;
        let mut chain = bypassed_chain(sr);

        // Settle to 0.0 (mute on, 50 ms is ample).
        chain.params.set_muted(true);
        let mut buf = vec![1.0_f32; (sr * 0.050) as usize];
        chain.process(&mut buf, sr);
        assert_eq!(
            chain.mute_current, 0.0,
            "mute_current must snap to exact 0.0 after settling"
        );

        // Settle back to 1.0 (mute off). Snap requires (1.0-current) < 1e-6,
        // which at τ=10 ms takes ~140 ms (≈14 τ); 200 ms is comfortable.
        chain.params.set_muted(false);
        let mut buf = vec![1.0_f32; (sr * 0.200) as usize];
        chain.process(&mut buf, sr);
        assert_eq!(
            chain.mute_current, 1.0,
            "mute_current must snap to exact 1.0 after settling"
        );
    }

    /// 1.9 — when settled to unmuted (`mute_current == 1.0`), the mute stage
    /// is bit-identical to a no-op: a known signal through the full chain
    /// matches the same chain run before the mute stage existed (verified by
    /// checking against the pre-mute output captured by snapshotting
    /// `mute_current = 1.0` then asserting buffer values are unchanged by the
    /// final apply_mute pass).
    #[test]
    fn settled_unmuted_mute_stage_is_no_op() {
        let sr = 48_000.0_f32;
        let mut chain = bypassed_chain(sr);
        // Already settled to 1.0 by construction.
        assert_eq!(chain.mute_current, 1.0);

        // A non-trivial known signal so any spurious gain shows up.
        let mut buf: Vec<f32> = (0..256)
            .map(|i| (i as f32 / 256.0 * std::f32::consts::TAU).sin() * 0.7)
            .collect();
        let original = buf.clone();

        chain.process(&mut buf, sr);

        // With every DSP bypassed and the mute stage settled at 1.0, the
        // chain must be a literal pass-through.
        for (i, (a, b)) in original.iter().zip(buf.iter()).enumerate() {
            assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "mute stage altered sample {i}: {a} → {b}"
            );
        }
    }

    /// Sample-rate change recomputes coefficients. Sanity: starting at 48k
    /// then processing at 96k reaches silence faster (in samples) only if
    /// coeffs are recomputed for the new SR.
    #[test]
    fn coeffs_recompute_on_sample_rate_change() {
        let mut chain = bypassed_chain(48_000.0);
        let initial_on = chain.mute_coeff_on;
        let mut buf = vec![1.0_f32; 64];
        chain.process(&mut buf, 96_000.0);
        assert_ne!(
            chain.mute_coeff_on, initial_on,
            "mute coefficients must be recomputed when sample rate changes"
        );
        assert_eq!(chain.mute_coeff_sr, 96_000.0);
    }
}
