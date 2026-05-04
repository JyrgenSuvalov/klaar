/// Klaar Audio Engine — CoreAudio with DSP chain processing.
///
/// Architecture:
///   - Two AUHAL AudioUnits: one input, one output (pattern from coreaudio-rs examples)
///   - Input AudioUnit's `set_input_callback`: reads mic samples → lock-free ring
///   - Output AudioUnit's `set_render_callback`: drains ring → ProcessorChain → output
///   - Meter values written to AtomicU32 (f32 bits) on the audio thread
///   - ProcessorChain: NoiseGate → ParametricEQ → DeEsser → Compressor → Limiter
///
/// Real-time rules (callbacks): NO allocations, NO mutexes, NO log!, NO syscalls.
use coreaudio::audio_unit::audio_format::LinearPcmFlags;
use coreaudio::audio_unit::macos_helpers::audio_unit_from_device_id;
use coreaudio::audio_unit::render_callback::{self, data};
use coreaudio::audio_unit::{Element, SampleFormat, Scope, StreamFormat};
use coreaudio::sys::kAudioUnitProperty_StreamFormat;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::device_manager::{get_device_id_by_uid, get_device_sample_rate_by_uid};
use crate::engine_state::AudioEngineState;
use crate::dsp::DspParams;
use crate::dsp::spectrum::{create_spectrum_pair, SpectrumReader};

// ────────────────────────────────────────────────────────────────────────────
// Constants
// ────────────────────────────────────────────────────────────────────────────

/// dBFS floor (used for silence and metering floor)
const METER_FLOOR_DB: f32 = -96.0;
/// Linear amplitude at the meter floor
const METER_FLOOR_LINEAR: f32 = 1.584_893e-5_f32; // 10^(-96/20)

// ────────────────────────────────────────────────────────────────────────────
// AudioEngine
// ────────────────────────────────────────────────────────────────────────────

/// Holds two CoreAudio AudioUnits (input + output) with full DSP chain processing.
///
/// `start()` opens both units; `stop()` tears them down cleanly.
/// Dropping a running engine calls `stop()` automatically.
pub struct AudioEngine {
    /// Input AudioUnit (HAHAL, input enabled)
    input_unit: Option<coreaudio::audio_unit::AudioUnit>,
    /// Output AudioUnit (HAL, output enabled)
    output_unit: Option<coreaudio::audio_unit::AudioUnit>,
    state: Arc<AudioEngineState>,
    dsp_params: Arc<DspParams>,
    pub input_device_uid: Option<String>,
    pub output_device_uid: Option<String>,
}

impl AudioEngine {
    pub fn new(state: Arc<AudioEngineState>, dsp_params: Arc<DspParams>) -> Self {
        Self {
            input_unit: None,
            output_unit: None,
            state,
            dsp_params,
            input_device_uid: None,
            output_device_uid: None,
        }
    }

    /// Start passthrough between `input_uid` and `output_uid`.
    ///
    /// Returns a descriptive `Err` string on failure; the error is forwarded to the frontend.
    pub fn start(&mut self, input_uid: &str, output_uid: &str) -> Result<SpectrumReader, String> {
        // Always stop cleanly before (re-)starting
        self.stop();

        // ── Sample rate check ──────────────────────────────────────────────
        let input_sr = get_device_sample_rate_by_uid(input_uid)?;
        let output_sr = get_device_sample_rate_by_uid(output_uid)?;

        if (input_sr - output_sr).abs() > 0.5 {
            return Err(format!(
                "sample_rate_mismatch: input {input_sr:.0} Hz ≠ output {output_sr:.0} Hz. \
                 Set both devices to the same sample rate in Audio MIDI Setup.",
            ));
        }
        let sample_rate = input_sr;

        // ── Device IDs ────────────────────────────────────────────────────
        let input_id = get_device_id_by_uid(input_uid)?;
        let output_id = get_device_id_by_uid(output_uid)?;

        // ── Stream format: mono f32 non-interleaved ────────────────────────
        let stream_format = StreamFormat {
            sample_rate,
            sample_format: SampleFormat::F32,
            flags: LinearPcmFlags::IS_FLOAT
                | LinearPcmFlags::IS_PACKED
                | LinearPcmFlags::IS_NON_INTERLEAVED,
            channels: 1,
        };

        // ── Input AudioUnit ────────────────────────────────────────────────
        let mut input_au = audio_unit_from_device_id(input_id, true).map_err(|e| {
            // -66748 (kAudioHardwareNotRunningError) often means mic permission denied
            let s = e.to_string();
            if s.contains("66748") || s.contains("10863") {
                "microphone_permission_denied".to_string()
            } else {
                format!("Failed to open input device: {e}")
            }
        })?;

        let asbd = stream_format.to_asbd();
        input_au
            .set_property(
                kAudioUnitProperty_StreamFormat,
                Scope::Output,
                Element::Input,
                Some(&asbd),
            )
            .map_err(|e| format!("set input stream format: {e}"))?;

        // ── Output AudioUnit ───────────────────────────────────────────────
        let mut output_au =
            audio_unit_from_device_id(output_id, false).map_err(|e| {
                format!("Failed to open output device: {e}")
            })?;

        let asbd = stream_format.to_asbd();
        output_au
            .set_property(
                kAudioUnitProperty_StreamFormat,
                Scope::Input,
                Element::Output,
                Some(&asbd),
            )
            .map_err(|e| format!("set output stream format: {e}"))?;

        // ── Lock-free ring buffer (SPSC) ───────────────────────────────────
        // Sized for ~100 ms at 48 kHz to absorb any callback jitter
        let ring = Arc::new(AudioRing::new(8192));
        let ring_in = ring.clone();
        let ring_out = ring.clone();

        // ── Monitoring ring buffer (output → headphone playback) ──────────
        // Same capacity as the mic ring. Written by output callback during
        // playback; read by the headphone monitoring AudioUnit.
        let mon_ring = Arc::new(AudioRing::new(8192));
        let mon_ring_out = mon_ring.clone();
        self.state.set_monitoring_ring(mon_ring);

        // Clone Arc<AudioEngineState> for both callbacks
        let state_in = self.state.clone();
        let state_out = self.state.clone();

        // Build the DSP chain — owned exclusively by the output render callback.
        // The chain reads parameters from Arc<DspParams> (bypass + effect params).
        // No mutex needed: all communication is via atomics.
        let mut chain = crate::dsp::ProcessorChain::new(self.dsp_params.clone(), sample_rate as f32);

        // Create spectrum analyzer pair — writer goes into ProcessorChain (post-gate, pre-EQ),
        // reader is returned to the caller for IPC access.
        let (spectrum_writer, spectrum_reader) = create_spectrum_pair(sample_rate as u32);
        chain.set_spectrum_writer(spectrum_writer);

        // ── Register input callback ────────────────────────────────────────
        type InputArgs = render_callback::Args<data::NonInterleaved<f32>>;

        input_au
            .set_input_callback(move |args: InputArgs| {
                let num_frames = args.num_frames;
                let mut data = args.data;

                if let Some(channel) = data.channels_mut().next() {
                    let samples = &channel[..num_frames];
                    // Compute input peak (dBFS) and write to atomic
                    let peak = peak_linear(samples);
                    state_in.set_input_peak(linear_to_db(peak));
                    // Feed ring
                    ring_in.push(samples);
                }
                Ok(())
            })
            .map_err(|e| format!("set_input_callback: {e}"))?;

        // ── Register output render callback ───────────────────────────────
        type OutputArgs = render_callback::Args<data::NonInterleaved<f32>>;

        // Capture processing_enabled from state; chain is owned by this closure.
        let sr = sample_rate as f32;

        output_au
            .set_render_callback(move |args: OutputArgs| {
                let num_frames = args.num_frames;
                let mut data = args.data;

                let processing = state_out.processing_enabled.load(Ordering::Relaxed);
                let playback_active = state_out.playback_active.load(Ordering::Acquire);

                for channel in data.channels_mut() {
                    if playback_active {
                        // Read from playback buffer instead of mic ring
                        if let Some(pb) = state_out.get_playback_buffer() {
                            pb.read(&mut channel[..num_frames]);
                        } else {
                            // Playback flag set but buffer not ready — output silence
                            for s in &mut channel[..num_frames] {
                                *s = 0.0;
                            }
                        }
                    } else {
                        // Normal mode: read from mic ring buffer
                        let written = ring_out.pop(&mut channel[..num_frames], num_frames);
                        // Zero any unfilled frames to prevent stale audio
                        for s in &mut channel[written..num_frames] {
                            *s = 0.0;
                        }
                    }

                    if processing {
                        // Route through DSP chain: Gate → EQ → De-esser → Compressor → Limiter
                        chain.process(&mut channel[..num_frames], sr);

                        // Write gain reduction meters
                        state_out.gate_reduction.store(
                            chain.noise_gate.last_gain_reduction_db.to_bits(),
                            Ordering::Relaxed,
                        );
                        state_out.deesser_reduction.store(
                            chain.deesser.last_gain_reduction_db.to_bits(),
                            Ordering::Relaxed,
                        );
                        state_out.deesser_sidechain_db.store(
                            chain.deesser.last_sidechain_db.to_bits(),
                            Ordering::Relaxed,
                        );
                        state_out.comp_reduction.store(
                            chain.compressor.last_gain_reduction_db.to_bits(),
                            Ordering::Relaxed,
                        );
                        state_out.limiter_reduction.store(
                            chain.limiter.last_gain_reduction_db.to_bits(),
                            Ordering::Relaxed,
                        );
                    } else {
                        // Passthrough: zero out meters. Sidechain meter sits
                        // at the silence floor since the chain didn't run.
                        state_out.gate_reduction.store(0.0f32.to_bits(), Ordering::Relaxed);
                        state_out.deesser_reduction.store(0.0f32.to_bits(), Ordering::Relaxed);
                        state_out.deesser_sidechain_db.store((-96.0f32).to_bits(), Ordering::Relaxed);
                        state_out.comp_reduction.store(0.0f32.to_bits(), Ordering::Relaxed);
                        state_out.limiter_reduction.store(0.0f32.to_bits(), Ordering::Relaxed);
                    }
                }

                // Compute output peak from the first channel after DSP
                if let Some(ch) = data.channels().next() {
                    let peak = peak_linear(&ch[..num_frames]);
                    state_out.set_output_peak(linear_to_db(peak));

                    // Write post-chain samples to recording buffer if active
                    // (zero allocs, zero locks — only atomic reads)
                    if let Some(rec_buf) = state_out.get_recording_buffer() {
                        rec_buf.push(&ch[..num_frames]);
                    }

                    // Write post-chain samples to monitoring ring buffer for
                    // headphone playback (only during active playback)
                    if playback_active {
                        mon_ring_out.push(&ch[..num_frames]);
                    }
                }

                Ok(())
            })
            .map_err(|e| format!("set_render_callback: {e}"))?;

        // ── Initialize & start both units ─────────────────────────────────
        input_au
            .initialize()
            .map_err(|e| format!("initialize input AudioUnit: {e}"))?;
        output_au
            .initialize()
            .map_err(|e| format!("initialize output AudioUnit: {e}"))?;

        input_au.start().map_err(|e| {
            let s = e.to_string();
            if s.contains("66748") || s.contains("10863") {
                "microphone_permission_denied".to_string()
            } else {
                format!("start input AudioUnit: {e}")
            }
        })?;
        output_au
            .start()
            .map_err(|e| format!("start output AudioUnit: {e}"))?;

        self.input_unit = Some(input_au);
        self.output_unit = Some(output_au);
        self.input_device_uid = Some(input_uid.to_string());
        self.output_device_uid = Some(output_uid.to_string());

        // Store the negotiated sample rate so recording can use it
        self.state.set_sample_rate(sample_rate as u32);

        log::info!(
            "AudioEngine started: {} → {} @ {sample_rate:.0} Hz",
            input_uid,
            output_uid,
        );

        Ok(spectrum_reader)
    }

    /// Stop the AudioUnits and reset meter values to silence.
    pub fn stop(&mut self) {
        if let Some(mut au) = self.input_unit.take() {
            let _ = au.stop();
        }
        if let Some(mut au) = self.output_unit.take() {
            let _ = au.stop();
        }

        // Clear playback state — must happen before monitoring ring is dropped
        self.state.clear_playback();
        self.state.clear_monitoring_ring();

        // Reset meters to floor
        self.state.set_input_peak(METER_FLOOR_DB);
        self.state.set_output_peak(METER_FLOOR_DB);

        if self.input_device_uid.is_some() {
            log::info!("AudioEngine stopped");
        }
        self.input_device_uid = None;
        self.output_device_uid = None;
    }

    pub fn is_running(&self) -> bool {
        self.input_unit.is_some()
    }

    /// Sample rate the engine was started with (Hz), or `None` if stopped.
    ///
    /// Returns the value stored in `AudioEngineState` by `start()` after
    /// successful AUHAL negotiation, suitable for comparison against a freshly
    /// queried `get_device_sample_rate_by_uid` to detect mid-stream SR drift
    /// (used by `device_format_listener` to decide whether to restart).
    pub fn current_sample_rate(&self) -> Option<u32> {
        if self.is_running() {
            Some(self.state.get_sample_rate())
        } else {
            None
        }
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

// ────────────────────────────────────────────────────────────────────────────
// DSP helpers — allocation-free, called on audio thread
// ────────────────────────────────────────────────────────────────────────────

/// Peak absolute sample value in a buffer.
#[inline(always)]
fn peak_linear(buf: &[f32]) -> f32 {
    buf.iter().fold(0.0f32, |acc, &s| {
        // Flush denormals to zero; guard against NaN/Inf
        let s = if s.is_finite() { s } else { 0.0 };
        acc.max(s.abs())
    })
}

/// Linear amplitude [0..1] → dBFS, clamped to floor.
#[inline(always)]
fn linear_to_db(linear: f32) -> f32 {
    if linear <= METER_FLOOR_LINEAR {
        METER_FLOOR_DB
    } else {
        (20.0 * linear.log10()).max(METER_FLOOR_DB)
    }
}

// ────────────────────────────────────────────────────────────────────────────
// SPSC lock-free ring buffer
// ────────────────────────────────────────────────────────────────────────────

/// Power-of-two single-producer / single-consumer ring buffer for f32 samples.
///
/// Thread safety guarantee: exactly one thread pushes, one thread pops.
/// Uses Release/Acquire ordering on the shared indices: the writer stores with
/// Release and the reader loads with Acquire, providing the minimal necessary
/// happens-before guarantee for correct SPSC operation.
pub(crate) struct AudioRing {
    buf: Vec<std::cell::UnsafeCell<f32>>,
    mask: usize,
    read: std::sync::atomic::AtomicUsize,
    write: std::sync::atomic::AtomicUsize,
}

// SAFETY: send/sync are safe because we enforce SPSC discipline at call sites.
unsafe impl Send for AudioRing {}
unsafe impl Sync for AudioRing {}

impl AudioRing {
    pub(crate) fn new(capacity: usize) -> Self {
        let cap = capacity.next_power_of_two();
        let buf = (0..cap)
            .map(|_| std::cell::UnsafeCell::new(0.0f32))
            .collect();
        Self {
            buf,
            mask: cap - 1,
            read: std::sync::atomic::AtomicUsize::new(0),
            write: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Push samples from `data`. Returns number of samples actually written.
    #[inline]
    pub(crate) fn push(&self, data: &[f32]) -> usize {
        let write = self.write.load(Ordering::Relaxed);
        let read = self.read.load(Ordering::Acquire);
        let free = self.buf.len() - write.wrapping_sub(read);
        let n = data.len().min(free);
        for (i, &s) in data[..n].iter().enumerate() {
            let idx = write.wrapping_add(i) & self.mask;
            unsafe { *self.buf[idx].get() = s };
        }
        self.write.store(write.wrapping_add(n), Ordering::Release);
        n
    }

    /// Pop up to `max` samples into `dst`. Returns number of samples read.
    #[inline]
    pub(crate) fn pop(&self, dst: &mut [f32], max: usize) -> usize {
        let read = self.read.load(Ordering::Relaxed);
        let write = self.write.load(Ordering::Acquire);
        let available = write.wrapping_sub(read);
        let n = max.min(available).min(dst.len());
        for (i, slot) in dst[..n].iter_mut().enumerate() {
            let idx = read.wrapping_add(i) & self.mask;
            *slot = unsafe { *self.buf[idx].get() };
        }
        self.read.store(read.wrapping_add(n), Ordering::Release);
        n
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    // ── AudioRing ────────────────────────────────────────────────────────────

    #[test]
    fn ring_push_pop_roundtrip() {
        let ring = AudioRing::new(16);
        let input: Vec<f32> = (0..8).map(|i| i as f32 * 0.1).collect();
        let written = ring.push(&input);
        assert_eq!(written, 8);

        let mut output = vec![0.0f32; 8];
        let read = ring.pop(&mut output, 8);
        assert_eq!(read, 8);
        for (a, b) in input.iter().zip(output.iter()) {
            assert_relative_eq!(a, b, epsilon = 1e-6);
        }
    }

    #[test]
    fn ring_overflow_silently_truncates() {
        // Capacity 8 (next power of two ≥ 4 is 4 → use 8 for a clear test)
        let ring = AudioRing::new(8);
        let input: Vec<f32> = vec![1.0f32; 12]; // 12 > 8, overflow
        let written = ring.push(&input);
        // Must not panic; written ≤ capacity
        assert!(written <= 8);
        // Ring is not corrupted — can still pop what was written
        let mut output = vec![0.0f32; written];
        let read = ring.pop(&mut output, written);
        assert_eq!(read, written);
    }

    #[test]
    fn ring_underrun_returns_zero() {
        let ring = AudioRing::new(16);
        // Pop from empty ring
        let mut output = vec![0.0f32; 8];
        let read = ring.pop(&mut output, 8);
        assert_eq!(read, 0, "popping empty ring must return 0");
        // Output buffer must be untouched (all zeros)
        for s in &output {
            assert_relative_eq!(*s, 0.0f32, epsilon = 1e-9);
        }
    }

    #[test]
    fn ring_multiple_cycles() {
        let ring = AudioRing::new(16);

        // Cycle 1: push 6, pop 4
        let data1: Vec<f32> = (0..6).map(|i| i as f32).collect();
        ring.push(&data1);
        let mut out1 = vec![0.0f32; 4];
        let r1 = ring.pop(&mut out1, 4);
        assert_eq!(r1, 4);
        assert_relative_eq!(out1[0], 0.0);
        assert_relative_eq!(out1[3], 3.0);

        // Cycle 2: push 4 more, pop remaining 2 + 4 = 6
        let data2: Vec<f32> = (10..14).map(|i| i as f32).collect();
        ring.push(&data2);
        let mut out2 = vec![0.0f32; 6];
        let r2 = ring.pop(&mut out2, 6);
        assert_eq!(r2, 6);
        // First 2 are leftover from data1 (values 4.0, 5.0)
        assert_relative_eq!(out2[0], 4.0);
        assert_relative_eq!(out2[1], 5.0);
        // Next 4 are data2 (10.0, 11.0, 12.0, 13.0)
        assert_relative_eq!(out2[2], 10.0);
        assert_relative_eq!(out2[5], 13.0);
    }

    // ── linear_to_db ─────────────────────────────────────────────────────────

    #[test]
    fn db_unity_gain_is_zero() {
        assert_relative_eq!(linear_to_db(1.0), 0.0, epsilon = 1e-4);
    }

    #[test]
    fn db_silence_is_floor() {
        assert_relative_eq!(linear_to_db(0.0), METER_FLOOR_DB, epsilon = 1e-4);
    }

    #[test]
    fn db_half_amplitude_is_minus_six() {
        // 20 * log10(0.5) ≈ -6.0206 dBFS
        assert_relative_eq!(linear_to_db(0.5), -6.0206, epsilon = 1e-3);
    }

    #[test]
    fn db_below_floor_is_clamped() {
        // A very small value that would compute far below the floor
        let result = linear_to_db(1e-10);
        assert_relative_eq!(result, METER_FLOOR_DB, epsilon = 1e-4);
    }

}
