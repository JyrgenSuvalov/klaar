/// Centralised atomic parameter storage for all DSP effects.
///
/// All fields are `AtomicU32` storing `f32` bits. This struct is `Arc`-shared
/// between the audio thread (reads) and IPC command handlers (writes).
///
/// Parameter ranges:
///   Noise Gate:   threshold (-80..0 dB), attack (0.1..100 ms), release (1..1000 ms), range (-80..0 dB)
///   EQ Band × 8: enabled (0/1), filter_type (0..4), freq (20..20000 Hz), gain (-24..+24 dB), q (0.1..18.0)
///   De-esser:     freq (2000..12000 Hz), threshold (-40..0 dB), ratio (1..10),
///                 attack (0.1..10 ms), release (10..500 ms), listen (bool)
///   Compressor:   threshold (-60..0 dB), ratio (1..20), attack (0.1..100 ms), release (1..1000 ms), makeup (0..30 dB)
///   Limiter:      ceiling (-20..0 dBFS), release (1..500 ms)
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

// ────────────────────────────────────────────────────────────────────────────
// Filter type enum (stored as f32 bits)
// ────────────────────────────────────────────────────────────────────────────

/// Biquad filter type variants.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterType {
    Bell = 0,
    HighPass = 1,
    LowPass = 2,
    HighShelf = 3,
    LowShelf = 4,
    /// RBJ BPF — passes a band around the centre, rejects low and high frequencies.
    /// Used internally by the de-esser sidechain; not exposed to UI.
    BandPass = 5,
    /// 48 dB/oct high-pass (8th-order Butterworth — 4 cascaded biquad stages).
    HighPass48 = 6,
    /// 48 dB/oct low-pass (8th-order Butterworth — 4 cascaded biquad stages).
    LowPass48 = 7,
}

impl FilterType {
    pub fn from_f32(v: f32) -> Self {
        match v as u32 {
            1 => FilterType::HighPass,
            2 => FilterType::LowPass,
            3 => FilterType::HighShelf,
            4 => FilterType::LowShelf,
            5 => FilterType::BandPass,
            6 => FilterType::HighPass48,
            7 => FilterType::LowPass48,
            _ => FilterType::Bell,
        }
    }

    pub fn to_f32(self) -> f32 {
        self as u32 as f32
    }

    /// Returns the number of cascaded biquad stages needed for this filter type.
    pub fn stage_count(self) -> usize {
        match self {
            FilterType::HighPass48 | FilterType::LowPass48 => 4,
            _ => 1,
        }
    }

    /// Returns the base filter type used for coefficient computation.
    /// HighPass48 uses HighPass coefficients per stage; LowPass48 uses LowPass.
    pub fn base_type(self) -> FilterType {
        match self {
            FilterType::HighPass48 => FilterType::HighPass,
            FilterType::LowPass48 => FilterType::LowPass,
            _ => self,
        }
    }

    /// Returns true if this is a pass filter (HP, LP, HPx4, LPx4).
    #[allow(dead_code)]
    pub fn is_pass(self) -> bool {
        matches!(
            self,
            FilterType::HighPass | FilterType::LowPass | FilterType::HighPass48 | FilterType::LowPass48
        )
    }
}

// ────────────────────────────────────────────────────────────────────────────
// EQ band parameter group
// ────────────────────────────────────────────────────────────────────────────

pub struct EqBandParams {
    pub enabled: AtomicU32,     // 0.0 = disabled, 1.0 = enabled
    pub filter_type: AtomicU32, // FilterType as f32 bits
    pub frequency: AtomicU32,   // Hz
    pub gain: AtomicU32,        // dB
    pub q: AtomicU32,           // Q factor
}

impl EqBandParams {
    fn new(enabled: bool, filter_type: FilterType, freq: f32, gain: f32, q: f32) -> Self {
        Self {
            enabled: AtomicU32::new((if enabled { 1.0f32 } else { 0.0f32 }).to_bits()),
            filter_type: AtomicU32::new(filter_type.to_f32().to_bits()),
            frequency: AtomicU32::new(freq.to_bits()),
            gain: AtomicU32::new(gain.to_bits()),
            q: AtomicU32::new(q.to_bits()),
        }
    }

    pub fn get_enabled(&self) -> bool {
        f32::from_bits(self.enabled.load(Ordering::Relaxed)) != 0.0
    }

    pub fn get_filter_type(&self) -> FilterType {
        FilterType::from_f32(f32::from_bits(self.filter_type.load(Ordering::Relaxed)))
    }

    pub fn get_frequency(&self) -> f32 {
        f32::from_bits(self.frequency.load(Ordering::Relaxed))
    }

    pub fn get_gain(&self) -> f32 {
        f32::from_bits(self.gain.load(Ordering::Relaxed))
    }

    pub fn get_q(&self) -> f32 {
        f32::from_bits(self.q.load(Ordering::Relaxed))
    }
}

// ────────────────────────────────────────────────────────────────────────────
// DspParams
// ────────────────────────────────────────────────────────────────────────────

pub struct DspParams {
    // ── Per-effect bypass flags (written by UI, read by audio thread) ────────
    pub bypass_gate:       AtomicBool,
    pub bypass_eq:         AtomicBool,
    pub bypass_deesser:    AtomicBool,
    pub bypass_compressor: AtomicBool,
    pub bypass_limiter:    AtomicBool,

    // ── Noise Gate ──────────────────────────────────────────────────────────
    pub gate_threshold: AtomicU32, // dB, -80..0, default -80 (transparent)
    pub gate_attack: AtomicU32,    // ms, 0.1..100, default 10
    pub gate_release: AtomicU32,   // ms, 1..1000, default 100
    pub gate_range: AtomicU32,     // dB, -80..0, default -60

    // ── Parametric EQ (8 bands) ─────────────────────────────────────────────
    pub eq_bands: [EqBandParams; 8],

    // ── De-esser ────────────────────────────────────────────────────────────
    pub deesser_frequency:  AtomicU32, // Hz, 2000..12000, default 6000
    pub deesser_threshold:  AtomicU32, // dB, -40..0, default -20 dB (audible on first use)
    pub deesser_ratio:      AtomicU32, // 1.0..10.0, default 4.0
    pub deesser_attack_ms:  AtomicU32, // ms, 0.1..10, default 1.0
    pub deesser_release_ms: AtomicU32, // ms, 10..500, default 50.0

    // ── Compressor ──────────────────────────────────────────────────────────
    pub comp_threshold: AtomicU32, // dB, -60..0, default 0 (transparent)
    pub comp_ratio: AtomicU32,     // 1..20, default 1.0 (transparent)
    pub comp_attack: AtomicU32,    // ms, 0.1..100, default 10
    pub comp_release: AtomicU32,   // ms, 1..1000, default 100
    pub comp_makeup: AtomicU32,    // dB, 0..30, default 0

    // ── Limiter ─────────────────────────────────────────────────────────────
    pub limiter_ceiling: AtomicU32, // dBFS, -20..0, default 0 (transparent)
    pub limiter_release: AtomicU32, // ms, 1..500, default 100

    // ── Global mute (post-limiter, smoothed in audio thread) ────────────────
    /// Mute intent flag, exposed to the audio thread. `1.0` = unmuted (pass),
    /// `0.0` = muted (silence). The audio thread one-pole-smooths a private
    /// `mute_current` toward this target with asymmetric τ; see
    /// `ProcessorChain::process` and the `mute-control` capability spec.
    ///
    /// NOT persisted in profiles — `get_all_params` / `set_all_params` ignore
    /// it. Always initialises to `1.0` (unmuted) at process start.
    pub mute_target: AtomicU32,
}

impl DspParams {
    pub fn new() -> Self {
        Self {
            // Bypass flags: all effects active by default
            bypass_gate:       AtomicBool::new(false),
            bypass_eq:         AtomicBool::new(false),
            bypass_deesser:    AtomicBool::new(false),
            bypass_compressor: AtomicBool::new(false),
            bypass_limiter:    AtomicBool::new(false),

            // Gate: threshold at -80 dB → effectively always open (transparent)
            gate_threshold: AtomicU32::new((-80.0f32).to_bits()),
            gate_attack: AtomicU32::new(10.0f32.to_bits()),
            gate_release: AtomicU32::new(100.0f32.to_bits()),
            gate_range: AtomicU32::new((-60.0f32).to_bits()),

            // EQ: 8 flat bands (bell, 0 dB gain, enabled)
            eq_bands: [
                EqBandParams::new(true, FilterType::Bell, 80.0, 0.0, 1.0),
                EqBandParams::new(true, FilterType::Bell, 200.0, 0.0, 1.0),
                EqBandParams::new(true, FilterType::Bell, 500.0, 0.0, 1.0),
                EqBandParams::new(true, FilterType::Bell, 1000.0, 0.0, 1.0),
                EqBandParams::new(true, FilterType::Bell, 2500.0, 0.0, 1.0),
                EqBandParams::new(true, FilterType::Bell, 5000.0, 0.0, 1.0),
                EqBandParams::new(true, FilterType::Bell, 10000.0, 0.0, 1.0),
                EqBandParams::new(true, FilterType::Bell, 16000.0, 0.0, 1.0),
            ],

            // De-esser: threshold at -20 dB → audible on first use with sibilance
            deesser_frequency:  AtomicU32::new(6000.0f32.to_bits()),
            deesser_threshold:  AtomicU32::new((-20.0f32).to_bits()),
            deesser_ratio:      AtomicU32::new(4.0f32.to_bits()),
            deesser_attack_ms:  AtomicU32::new(1.0f32.to_bits()),
            deesser_release_ms: AtomicU32::new(50.0f32.to_bits()),

            // Compressor: ratio 1:1 → no compression (transparent)
            comp_threshold: AtomicU32::new(0.0f32.to_bits()),
            comp_ratio: AtomicU32::new(1.0f32.to_bits()),
            comp_attack: AtomicU32::new(10.0f32.to_bits()),
            comp_release: AtomicU32::new(100.0f32.to_bits()),
            comp_makeup: AtomicU32::new(0.0f32.to_bits()),

            // Limiter: ceiling at 0 dBFS → full headroom (transparent)
            limiter_ceiling: AtomicU32::new(0.0f32.to_bits()),
            limiter_release: AtomicU32::new(100.0f32.to_bits()),

            // Mute: 1.0 (unmuted). Always launches unmuted; never persisted.
            mute_target: AtomicU32::new(1.0f32.to_bits()),
        }
    }

    // ── Mute helpers ────────────────────────────────────────────────────────

    /// Read the current mute intent. `true` = muted.
    #[inline]
    pub fn is_muted(&self) -> bool {
        Self::get_f32(&self.mute_target) < 0.5
    }

    /// Set the mute intent. `true` = muted (target 0.0), `false` = unmuted (1.0).
    #[inline]
    pub fn set_muted(&self, muted: bool) {
        Self::set_f32(&self.mute_target, if muted { 0.0 } else { 1.0 });
    }

    /// Flip the mute intent and return the new boolean state (`true` = muted).
    #[inline]
    pub fn toggle_muted(&self) -> bool {
        let new_muted = !self.is_muted();
        self.set_muted(new_muted);
        new_muted
    }

    // ── Convenience helpers ─────────────────────────────────────────────────

    #[inline]
    pub fn get_f32(atomic: &AtomicU32) -> f32 {
        f32::from_bits(atomic.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_f32(atomic: &AtomicU32, value: f32) {
        atomic.store(value.to_bits(), Ordering::Relaxed);
    }

    // Gate getters
    pub fn gate_threshold(&self) -> f32 { Self::get_f32(&self.gate_threshold) }
    pub fn gate_attack_ms(&self) -> f32 { Self::get_f32(&self.gate_attack) }
    pub fn gate_release_ms(&self) -> f32 { Self::get_f32(&self.gate_release) }
    pub fn gate_range_db(&self) -> f32 { Self::get_f32(&self.gate_range) }

    // De-esser getters
    pub fn deesser_frequency(&self) -> f32 { Self::get_f32(&self.deesser_frequency) }
    pub fn deesser_threshold(&self) -> f32 { Self::get_f32(&self.deesser_threshold) }
    pub fn deesser_ratio(&self) -> f32 { Self::get_f32(&self.deesser_ratio) }
    pub fn deesser_attack_ms(&self) -> f32 { Self::get_f32(&self.deesser_attack_ms) }
    pub fn deesser_release_ms(&self) -> f32 { Self::get_f32(&self.deesser_release_ms) }

    // Compressor getters
    pub fn comp_threshold(&self) -> f32 { Self::get_f32(&self.comp_threshold) }
    pub fn comp_ratio(&self) -> f32 { Self::get_f32(&self.comp_ratio) }
    pub fn comp_attack_ms(&self) -> f32 { Self::get_f32(&self.comp_attack) }
    pub fn comp_release_ms(&self) -> f32 { Self::get_f32(&self.comp_release) }
    pub fn comp_makeup_db(&self) -> f32 { Self::get_f32(&self.comp_makeup) }

    // Limiter getters
    pub fn limiter_ceiling_db(&self) -> f32 { Self::get_f32(&self.limiter_ceiling) }
    pub fn limiter_release_ms(&self) -> f32 { Self::get_f32(&self.limiter_release) }

    // ── Batch parameter read/write (for profile save/load) ─────────────────

    /// Snapshot all current atomic values into a serialisable struct.
    pub fn get_all_params(&self) -> crate::profile_manager::AllDspParams {
        use crate::profile_manager::*;

        AllDspParams {
            noise_gate: NoiseGateParams {
                threshold: self.gate_threshold(),
                attack: self.gate_attack_ms(),
                release: self.gate_release_ms(),
                range: self.gate_range_db(),
                bypassed: self.bypass_gate.load(Ordering::Relaxed),
            },
            eq: EqParams {
                bands: self.eq_bands.iter().map(|b| EqBandData {
                    enabled: b.get_enabled(),
                    filter_type: match b.get_filter_type() {
                        FilterType::Bell      => FilterTypeSerde::Bell,
                        FilterType::HighPass  => FilterTypeSerde::HighPass,
                        FilterType::LowPass   => FilterTypeSerde::LowPass,
                        FilterType::HighShelf => FilterTypeSerde::HighShelf,
                        FilterType::LowShelf  => FilterTypeSerde::LowShelf,
                        FilterType::HighPass48 => FilterTypeSerde::HighPass48,
                        FilterType::LowPass48  => FilterTypeSerde::LowPass48,
                        // BandPass is internal (de-esser sidechain); never written to profiles.
                        FilterType::BandPass => FilterTypeSerde::Bell,
                    },
                    frequency: b.get_frequency(),
                    gain: b.get_gain(),
                    q: b.get_q(),
                }).collect(),
                bypassed: self.bypass_eq.load(Ordering::Relaxed),
            },
            de_esser: DeEsserParams {
                frequency: self.deesser_frequency(),
                threshold: self.deesser_threshold(),
                ratio: self.deesser_ratio(),
                attack_ms: self.deesser_attack_ms(),
                release_ms: self.deesser_release_ms(),
                bypassed: self.bypass_deesser.load(Ordering::Relaxed),
            },
            compressor: CompressorParams {
                threshold: self.comp_threshold(),
                ratio: self.comp_ratio(),
                attack: self.comp_attack_ms(),
                release: self.comp_release_ms(),
                makeup_gain: self.comp_makeup_db(),
                bypassed: self.bypass_compressor.load(Ordering::Relaxed),
            },
            limiter: LimiterParams {
                ceiling: self.limiter_ceiling_db(),
                release: self.limiter_release_ms(),
                bypassed: self.bypass_limiter.load(Ordering::Relaxed),
            },
        }
    }

    /// Write all parameter values from a profile snapshot to the atomics.
    ///
    /// All values are clamped to their valid ranges before being written.  This
    /// guards against hand-edited profiles (or future schema changes) sending
    /// out-of-range values to the audio thread — e.g. a negative frequency or
    /// zero Q would produce NaN biquad coefficients.
    pub fn set_all_params(&self, params: &crate::profile_manager::AllDspParams) {
        use crate::profile_manager::FilterTypeSerde;
        // Noise Gate
        Self::set_f32(&self.gate_threshold, params.noise_gate.threshold.clamp(-80.0, 0.0));
        Self::set_f32(&self.gate_attack,    params.noise_gate.attack.clamp(0.1, 100.0));
        Self::set_f32(&self.gate_release,   params.noise_gate.release.clamp(1.0, 1000.0));
        Self::set_f32(&self.gate_range,     params.noise_gate.range.clamp(-80.0, 0.0));
        self.bypass_gate.store(params.noise_gate.bypassed, Ordering::Relaxed);

        // EQ bands
        for (i, band) in params.eq.bands.iter().enumerate() {
            if i >= 8 { break; }
            let b = &self.eq_bands[i];
            b.enabled.store(
                (if band.enabled { 1.0f32 } else { 0.0f32 }).to_bits(),
                Ordering::Relaxed,
            );
            let ft = match band.filter_type {
                FilterTypeSerde::HighPass   => FilterType::HighPass,
                FilterTypeSerde::LowPass    => FilterType::LowPass,
                FilterTypeSerde::HighShelf  => FilterType::HighShelf,
                FilterTypeSerde::LowShelf   => FilterType::LowShelf,
                FilterTypeSerde::HighPass48 => FilterType::HighPass48,
                FilterTypeSerde::LowPass48  => FilterType::LowPass48,
                // Bell, Unknown (hand-edited / forward-compat) → Bell
                FilterTypeSerde::Bell | FilterTypeSerde::Unknown => FilterType::Bell,
            };
            b.filter_type.store(ft.to_f32().to_bits(), Ordering::Relaxed);
            b.frequency.store(band.frequency.clamp(20.0, 20000.0).to_bits(), Ordering::Relaxed);
            b.gain.store(band.gain.clamp(-24.0, 24.0).to_bits(), Ordering::Relaxed);
            b.q.store(band.q.clamp(0.1, 18.0).to_bits(), Ordering::Relaxed);
        }
        self.bypass_eq.store(params.eq.bypassed, Ordering::Relaxed);

        // De-esser
        Self::set_f32(&self.deesser_frequency,  params.de_esser.frequency.clamp(2000.0, 12000.0));
        Self::set_f32(&self.deesser_threshold,  params.de_esser.threshold.clamp(-40.0, 0.0));
        Self::set_f32(&self.deesser_ratio,      params.de_esser.ratio.clamp(1.0, 10.0));
        Self::set_f32(&self.deesser_attack_ms,  params.de_esser.attack_ms.clamp(0.1, 10.0));
        Self::set_f32(&self.deesser_release_ms, params.de_esser.release_ms.clamp(10.0, 500.0));
        self.bypass_deesser.store(params.de_esser.bypassed, Ordering::Relaxed);

        // Compressor
        Self::set_f32(&self.comp_threshold, params.compressor.threshold.clamp(-60.0, 0.0));
        Self::set_f32(&self.comp_ratio,     params.compressor.ratio.clamp(1.0, 20.0));
        Self::set_f32(&self.comp_attack,    params.compressor.attack.clamp(0.1, 100.0));
        Self::set_f32(&self.comp_release,   params.compressor.release.clamp(1.0, 1000.0));
        Self::set_f32(&self.comp_makeup,    params.compressor.makeup_gain.clamp(0.0, 30.0));
        self.bypass_compressor.store(params.compressor.bypassed, Ordering::Relaxed);

        // Limiter
        Self::set_f32(&self.limiter_ceiling, params.limiter.ceiling.clamp(-20.0, 0.0));
        Self::set_f32(&self.limiter_release, params.limiter.release.clamp(1.0, 500.0));
        self.bypass_limiter.store(params.limiter.bypassed, Ordering::Relaxed);
    }
}

impl Default for DspParams {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile_manager::*;

    #[test]
    fn get_all_params_returns_defaults() {
        let p = DspParams::new();
        let all = p.get_all_params();

        assert_eq!(all.noise_gate.threshold, -80.0);
        assert_eq!(all.noise_gate.attack, 10.0);
        assert!(!all.noise_gate.bypassed);
        assert_eq!(all.compressor.ratio, 1.0);
        assert_eq!(all.limiter.ceiling, 0.0);
        assert_eq!(all.eq.bands.len(), 8);
        assert_eq!(all.eq.bands[0].frequency, 80.0);
    }

    #[test]
    fn set_all_params_updates_atomics() {
        let p = DspParams::new();
        let params = AllDspParams {
            noise_gate: NoiseGateParams {
                threshold: -30.0,
                attack: 5.0,
                release: 50.0,
                range: -40.0,
                bypassed: true,
            },
            compressor: CompressorParams {
                threshold: -20.0,
                ratio: 4.0,
                attack: 2.0,
                release: 200.0,
                makeup_gain: 6.0,
                bypassed: false,
            },
            limiter: LimiterParams {
                ceiling: -1.0,
                release: 50.0,
                bypassed: true,
            },
            ..Default::default()
        };

        p.set_all_params(&params);

        assert_eq!(p.gate_threshold(), -30.0);
        assert_eq!(p.gate_attack_ms(), 5.0);
        assert!(p.bypass_gate.load(Ordering::Relaxed));
        assert_eq!(p.comp_threshold(), -20.0);
        assert_eq!(p.comp_ratio(), 4.0);
        assert_eq!(p.comp_makeup_db(), 6.0);
        assert_eq!(p.limiter_ceiling_db(), -1.0);
        assert!(p.bypass_limiter.load(Ordering::Relaxed));
    }

    #[test]
    fn roundtrip_get_set_get() {
        let p = DspParams::new();
        let params = AllDspParams {
            noise_gate: NoiseGateParams {
                threshold: -25.0,
                attack: 3.0,
                release: 75.0,
                range: -50.0,
                bypassed: true,
            },
            eq: EqParams {
                bands: vec![
                    EqBandData { enabled: false, filter_type: FilterTypeSerde::HighPass, frequency: 100.0, gain: 0.0, q: 0.7 },
                    EqBandData { enabled: true,  filter_type: FilterTypeSerde::Bell,     frequency: 250.0, gain: 3.0, q: 2.0 },
                    EqBandData::default(),
                    EqBandData::default(),
                    EqBandData::default(),
                    EqBandData::default(),
                    EqBandData::default(),
                    EqBandData { enabled: true,  filter_type: FilterTypeSerde::LowPass,  frequency: 18000.0, gain: 0.0, q: 0.7 },
                ],
                bypassed: true,
            },
            de_esser: DeEsserParams {
                frequency: 7000.0,
                threshold: -15.0,
                ratio: 6.0,
                attack_ms: 2.0,
                release_ms: 80.0,
                bypassed: false,
            },
            compressor: CompressorParams {
                threshold: -18.0,
                ratio: 3.0,
                attack: 5.0,
                release: 150.0,
                makeup_gain: 4.0,
                bypassed: false,
            },
            limiter: LimiterParams {
                ceiling: -0.5,
                release: 30.0,
                bypassed: false,
            },
        };

        p.set_all_params(&params);
        let readback = p.get_all_params();

        assert_eq!(readback.noise_gate, params.noise_gate);
        assert_eq!(readback.de_esser, params.de_esser);
        assert_eq!(readback.compressor, params.compressor);
        assert_eq!(readback.limiter, params.limiter);
        assert_eq!(readback.eq.bypassed, params.eq.bypassed);
        assert_eq!(readback.eq.bands.len(), 8);
        assert_eq!(readback.eq.bands[0].filter_type, FilterTypeSerde::HighPass);
        assert!(!readback.eq.bands[0].enabled);
        assert_eq!(readback.eq.bands[1].gain, 3.0);
        assert_eq!(readback.eq.bands[7].filter_type, FilterTypeSerde::LowPass);
    }
}
