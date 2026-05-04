/// Property tests and full-chain integration tests for the DSP processing chain.
///
/// Task 8.1: Property tests — no NaN/Inf output for any f32 input across all processors;
///           limiter never exceeds ceiling.
/// Task 8.2: Full-chain integration test with known signals.
#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use proptest::prelude::*;

    use crate::dsp::params::DspParams;
    use crate::dsp::{AudioProcessor, ProcessorChain};
    use crate::dsp::noise_gate::NoiseGate;
    use crate::dsp::parametric_eq::ParametricEq;
    use crate::dsp::deesser::DeEsser;
    use crate::dsp::compressor::Compressor;
    use crate::dsp::limiter::Limiter;
    use crate::dsp::params::FilterType;

    const SR: f32 = 48000.0;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn sine_wave(freq: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq / SR * i as f32).sin())
            .collect()
    }

    fn rms(buf: &[f32]) -> f32 {
        let s: f32 = buf.iter().map(|x| x * x).sum();
        (s / buf.len() as f32).sqrt()
    }

    fn to_db(linear: f32) -> f32 {
        20.0 * linear.log10()
    }

    fn db_to_linear(db: f32) -> f32 {
        10.0f32.powf(db / 20.0)
    }

    // ── Property test strategies ──────────────────────────────────────────────

    /// Any finite f32 (no NaN/Inf) in a plausible audio range
    fn finite_sample_strategy() -> impl Strategy<Value = f32> {
        // Include full f32 range + exact 0.0 + near-zero denormals
        prop_oneof![
            Just(0.0f32),
            Just(1.0f32),
            Just(-1.0f32),
            Just(f32::MIN_POSITIVE),
            Just(-f32::MIN_POSITIVE),
            // Regular audio range
            -10.0f32..10.0f32,
            // Edge cases near limits
            prop::num::f32::NORMAL,
        ]
    }

    /// f32 including NaN, Inf, and -Inf
    fn any_f32_strategy() -> impl Strategy<Value = f32> {
        prop_oneof![
            finite_sample_strategy(),
            Just(f32::NAN),
            Just(f32::INFINITY),
            Just(f32::NEG_INFINITY),
        ]
    }

    // ── 8.1 Property tests ────────────────────────────────────────────────────

    proptest! {
        #[test]
        fn noise_gate_no_nan_inf_output(samples in prop::collection::vec(any_f32_strategy(), 1..512)) {
            let params = Arc::new(DspParams::new());
            DspParams::set_f32(&params.gate_threshold, -40.0);
            let mut gate = NoiseGate::new(params, SR);
            let mut buf = samples;
            gate.process(&mut buf, SR);
            for s in &buf {
                prop_assert!(s.is_finite(), "NoiseGate produced non-finite: {s}");
            }
        }

        #[test]
        fn parametric_eq_no_nan_inf_output(samples in prop::collection::vec(any_f32_strategy(), 1..512)) {
            let params = Arc::new(DspParams::new());
            // Configure a band with non-trivial gain
            use std::sync::atomic::Ordering;
            params.eq_bands[0].gain.store(12.0f32.to_bits(), Ordering::Relaxed);
            params.eq_bands[0].q.store(4.0f32.to_bits(), Ordering::Relaxed);
            let mut eq = ParametricEq::new(params, SR);
            let mut buf = samples;
            eq.process(&mut buf, SR);
            for s in &buf {
                prop_assert!(s.is_finite(), "ParametricEq produced non-finite: {s}");
            }
        }

        #[test]
        fn compressor_no_nan_inf_output(samples in prop::collection::vec(any_f32_strategy(), 1..512)) {
            let params = Arc::new(DspParams::new());
            DspParams::set_f32(&params.comp_threshold, -20.0);
            DspParams::set_f32(&params.comp_ratio, 4.0);
            let mut comp = Compressor::new(params, SR);
            let mut buf = samples;
            comp.process(&mut buf, SR);
            for s in &buf {
                prop_assert!(s.is_finite(), "Compressor produced non-finite: {s}");
            }
        }

        #[test]
        fn deesser_no_nan_inf_output(samples in prop::collection::vec(any_f32_strategy(), 1..512)) {
            let params = Arc::new(DspParams::new());
            DspParams::set_f32(&params.deesser_threshold, -20.0);
            let mut de = DeEsser::new(params, SR);
            let mut buf = samples;
            de.process(&mut buf, SR);
            for s in &buf {
                prop_assert!(s.is_finite(), "DeEsser produced non-finite: {s}");
            }
        }

        #[test]
        fn limiter_no_nan_inf_output(samples in prop::collection::vec(any_f32_strategy(), 1..512)) {
            let params = Arc::new(DspParams::new());
            DspParams::set_f32(&params.limiter_ceiling, -3.0);
            let mut lim = Limiter::new(params, SR);
            let mut buf = samples;
            lim.process(&mut buf, SR);
            for s in &buf {
                prop_assert!(s.is_finite(), "Limiter produced non-finite: {s}");
            }
        }

        #[test]
        fn limiter_never_exceeds_ceiling(samples in prop::collection::vec(finite_sample_strategy(), 1..512)) {
            let ceiling_db = -3.0_f32;
            let ceiling_linear = db_to_linear(ceiling_db);
            let params = Arc::new(DspParams::new());
            DspParams::set_f32(&params.limiter_ceiling, ceiling_db);
            let mut lim = Limiter::new(params, SR);
            let mut buf = samples;
            lim.process(&mut buf, SR);
            for (i, s) in buf.iter().enumerate() {
                prop_assert!(
                    s.abs() <= ceiling_linear + 1e-5,
                    "Limiter sample {i} exceeds ceiling: {:.6} > {:.6}", s.abs(), ceiling_linear
                );
            }
        }

        #[test]
        fn full_chain_no_nan_inf_output(samples in prop::collection::vec(any_f32_strategy(), 1..512)) {
            let params = Arc::new(DspParams::new());
            DspParams::set_f32(&params.limiter_ceiling, -3.0);
            let mut chain = ProcessorChain::new(params, SR);
            let mut buf = samples;
            chain.process(&mut buf, SR);
            for s in &buf {
                prop_assert!(s.is_finite(), "Full chain produced non-finite: {s}");
            }
        }

        #[test]
        fn full_chain_limiter_ceiling_enforced(samples in prop::collection::vec(finite_sample_strategy(), 1..512)) {
            let ceiling_db = -6.0_f32;
            let ceiling_linear = db_to_linear(ceiling_db);
            let params = Arc::new(DspParams::new());
            DspParams::set_f32(&params.limiter_ceiling, ceiling_db);
            // Disable all but limiter to isolate ceiling enforcement
            params.bypass_gate.store(true, std::sync::atomic::Ordering::Relaxed);
            params.bypass_eq.store(true, std::sync::atomic::Ordering::Relaxed);
            params.bypass_deesser.store(true, std::sync::atomic::Ordering::Relaxed);
            params.bypass_compressor.store(true, std::sync::atomic::Ordering::Relaxed);
            let mut chain = ProcessorChain::new(params, SR);
            let mut buf = samples;
            chain.process(&mut buf, SR);
            for (i, s) in buf.iter().enumerate() {
                prop_assert!(
                    s.abs() <= ceiling_linear + 1e-5,
                    "Chain sample {i} exceeds limiter ceiling: {:.6} > {:.6}", s.abs(), ceiling_linear
                );
            }
        }
    }

    // ── 8.2 Full-chain integration test ───────────────────────────────────────

    #[test]
    fn full_chain_integration_known_signal() {
        // Signal: 1 kHz sine at -6 dBFS
        // Chain config:
        //   Gate: threshold -60 dB (open), range -40 dB
        //   EQ: band 3 → +3 dB at 1 kHz (bell, Q=2)
        //   De-esser: threshold 0 dB (inactive — signal is 1 kHz, not sibilant band)
        //   Compressor: threshold -12 dB, ratio 4:1, attack 1ms, release 100ms, makeup 0 dB
        //   Limiter: ceiling -3 dBFS
        //
        // Expected properties:
        //   1. No output sample exceeds -3 dBFS (limiter ceiling)
        //   2. Gate does not attenuate (signal is well above -60 dB threshold)
        //   3. Compressor reduces the level above -12 dBFS

        use std::sync::atomic::Ordering;

        let params = Arc::new(DspParams::new());

        // Gate: wide open
        DspParams::set_f32(&params.gate_threshold, -60.0);
        DspParams::set_f32(&params.gate_range, -40.0);
        DspParams::set_f32(&params.gate_attack, 1.0);
        DspParams::set_f32(&params.gate_release, 100.0);

        // EQ: all bands at 0 gain (transparent), except band 3 at +3 dB
        for i in 0..8 {
            params.eq_bands[i].gain.store(0.0f32.to_bits(), Ordering::Relaxed);
        }
        params.eq_bands[3].filter_type.store(FilterType::Bell.to_f32().to_bits(), Ordering::Relaxed);
        params.eq_bands[3].frequency.store(1000.0f32.to_bits(), Ordering::Relaxed);
        params.eq_bands[3].gain.store(3.0f32.to_bits(), Ordering::Relaxed);
        params.eq_bands[3].q.store(2.0f32.to_bits(), Ordering::Relaxed);

        // De-esser: effectively inactive (threshold 0 dB, signal below sidechain band)
        DspParams::set_f32(&params.deesser_threshold, 0.0);
        DspParams::set_f32(&params.deesser_frequency, 8000.0);

        // Compressor: compress above -12 dB at 4:1
        DspParams::set_f32(&params.comp_threshold, -12.0);
        DspParams::set_f32(&params.comp_ratio, 4.0);
        DspParams::set_f32(&params.comp_attack, 1.0);
        DspParams::set_f32(&params.comp_release, 100.0);
        DspParams::set_f32(&params.comp_makeup, 0.0);

        // Limiter: ceiling -3 dBFS
        DspParams::set_f32(&params.limiter_ceiling, -3.0);
        DspParams::set_f32(&params.limiter_release, 50.0);

        let mut chain = ProcessorChain::new(params, SR);

        // Warmup: 200 ms of signal to reach steady state
        let warmup = sine_wave(1000.0, db_to_linear(-6.0), 9600);
        let mut warm_buf = warmup;
        chain.process(&mut warm_buf, SR);

        // Test buffer: 1024 samples
        let mut buf = sine_wave(1000.0, db_to_linear(-6.0), 1024);
        chain.process(&mut buf, SR);

        let ceiling_linear = db_to_linear(-3.0);

        // Property 1: No sample exceeds limiter ceiling
        for (i, &s) in buf.iter().enumerate() {
            assert!(
                s.abs() <= ceiling_linear + 1e-5,
                "Sample {i} exceeds limiter ceiling: {:.6} > {:.6}", s.abs(), ceiling_linear
            );
        }

        // Property 2: Signal is not silence (gate didn't close)
        let out_rms = rms(&buf);
        assert!(
            out_rms > 0.01,
            "Gate should not have closed on -6 dBFS signal: rms={out_rms}"
        );

        // Property 3: Output is finite everywhere
        for s in &buf {
            assert!(s.is_finite(), "Chain output must be finite");
        }

        // Property 4: Compressor has reduced the peak below what EQ would have produced
        // (EQ adds +3 dB at 1 kHz → -6 + 3 = -3 dB before compressor → compressor acts)
        let out_db = to_db(out_rms);
        let eq_only_db = -6.0 + 3.0; // what EQ alone would produce ≈ -3 dB
        // Compressor + limiter should bring it below that
        assert!(
            out_db < eq_only_db,
            "Compressor should reduce level below EQ output: got {out_db:.2} dB vs {eq_only_db:.2} dB"
        );
    }

    #[test]
    fn bypassed_processor_passes_audio_unmodified() {
        // Configure an aggressive compressor that would definitely change the signal
        // if it were active, then bypass it and verify output == input sample-for-sample.
        let params = Arc::new(DspParams::new());
        DspParams::set_f32(&params.comp_threshold, -60.0); // trigger on everything
        DspParams::set_f32(&params.comp_ratio, 20.0);       // extreme ratio
        DspParams::set_f32(&params.comp_makeup, 0.0);

        // Bypass every processor in the chain
        use std::sync::atomic::Ordering;
        params.bypass_gate.store(true, Ordering::Relaxed);
        params.bypass_eq.store(true, Ordering::Relaxed);
        params.bypass_deesser.store(true, Ordering::Relaxed);
        params.bypass_compressor.store(true, Ordering::Relaxed);
        params.bypass_limiter.store(true, Ordering::Relaxed);

        let mut chain = ProcessorChain::new(params, SR);

        let input = sine_wave(1000.0, 0.5, 256);
        let mut buf = input.clone();
        chain.process(&mut buf, SR);

        // Every sample must be byte-for-byte identical to the input
        for (i, (&expected, &actual)) in input.iter().zip(buf.iter()).enumerate() {
            assert_eq!(
                expected, actual,
                "Sample {i}: fully-bypassed chain must pass audio unmodified (expected {expected}, got {actual})"
            );
        }
    }

    #[test]
    fn full_chain_gate_attenuates_silence() {
        // Gate with high threshold: silence input should be attenuated by range
        let params = Arc::new(DspParams::new());

        // Gate: threshold at -30 dB, range -50 dB (heavy attenuation)
        DspParams::set_f32(&params.gate_threshold, -30.0);
        DspParams::set_f32(&params.gate_range, -50.0);
        DspParams::set_f32(&params.gate_attack, 1.0);
        DspParams::set_f32(&params.gate_release, 1.0);

        // Bypass all other effects — test gate in isolation
        params.bypass_eq.store(true, std::sync::atomic::Ordering::Relaxed);
        params.bypass_deesser.store(true, std::sync::atomic::Ordering::Relaxed);
        params.bypass_compressor.store(true, std::sync::atomic::Ordering::Relaxed);
        params.bypass_limiter.store(true, std::sync::atomic::Ordering::Relaxed);

        let mut chain = ProcessorChain::new(params, SR);

        // Feed silence to close the gate
        let mut warmup = vec![0.0f32; 4800];
        chain.process(&mut warmup, SR);

        // Now measure: silence at -60 dBFS (well below -30 dB threshold)
        let signal = db_to_linear(-60.0);
        let mut buf = vec![signal; 1024];
        chain.process(&mut buf, SR);

        let out_rms = rms(&buf);
        // With range -50 dB, the gate applies -50 dB attenuation to the already quiet signal
        // Output should be very small
        assert!(
            out_rms < db_to_linear(-60.0) * 0.1,
            "Gate should attenuate silence heavily: rms={out_rms:.2e}"
        );
    }
}
