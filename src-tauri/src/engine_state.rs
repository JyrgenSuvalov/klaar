use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::audio_engine::AudioRing;
use crate::recording_manager::{PlaybackBuffer, RecordingBuffer};

/// Shared state between the audio thread and the Tauri command handlers.
///
/// All fields use atomic types — no mutexes cross the thread boundary.
/// AtomicU32 stores f32 values via bit-casting (f32::to_bits / f32::from_bits).
pub struct AudioEngineState {
    // ── Control (written by UI, read by audio thread) ──────────────────────
    pub processing_enabled: AtomicBool,

    /// Set when the user has installed/updated the Klaar driver and not
    /// yet rebooted. While true, `try_start_engine` early-returns — see
    /// `commands::engine_start_blocked_for_reboot`. Written by:
    ///   - the `set_reboot_pending` IPC (frontend toggles it from
    ///     `state/onboardingPersistedStore.ts` whenever it writes / clears
    ///     the file-backed store), AND
    ///   - the startup seed in `lib.rs::setup` (reads `onboarding-state.json`
    ///     and compares the recorded boot timestamp to `kern.boottime`).
    ///
    /// Read by command-thread callers only — never touched by the audio
    /// callback.
    pub reboot_pending: AtomicBool,

    // ── Meters (written by audio thread, read by UI) ────────────────────────
    /// Input peak level as dBFS (stored as f32 bits in AtomicU32)
    pub input_level_peak: AtomicU32,
    /// Output peak level as dBFS
    pub output_level_peak: AtomicU32,
    /// Gate gain reduction in dB (currently always 0.0)
    pub gate_reduction: AtomicU32,
    /// De-esser gain reduction in dB (currently always 0.0)
    pub deesser_reduction: AtomicU32,
    /// De-esser sidechain envelope peak in dBFS, range [-96, 0].
    /// Published every buffer regardless of bypass — the user needs to see
    /// what the bandpass detector *would* see before engaging the effect.
    pub deesser_sidechain_db: AtomicU32,
    /// Compressor gain reduction in dB (currently always 0.0)
    pub comp_reduction: AtomicU32,
    /// Limiter gain reduction in dB (currently always 0.0)
    pub limiter_reduction: AtomicU32,

    // ── Engine info ─────────────────────────────────────────────────────────
    /// Negotiated sample rate (set when engine starts, read by recording).
    pub sample_rate: AtomicU32,

    // ── Recording (written by command thread, read by audio thread) ─────────
    /// Whether the audio callback should write samples to the recording buffer.
    pub recording_active: AtomicBool,
    /// Pointer to the recording buffer. Set before `recording_active` is set to true.
    /// The audio callback reads this via Relaxed load — it's valid whenever
    /// `recording_active` is true (guaranteed by set_recording using Release/Acquire).
    recording_buffer_ptr: AtomicPtr<RecordingBuffer>,
    /// Holds the Arc to keep the buffer alive while recording_buffer_ptr is set.
    /// Only accessed from the command thread (behind EngineHandle's Mutex).
    recording_buffer_arc: std::sync::Mutex<Option<Arc<RecordingBuffer>>>,

    // ── Playback (written by command thread, read by audio thread) ─────────
    /// Whether the output callback should read from the playback buffer instead of mic ring.
    pub playback_active: AtomicBool,
    /// Pointer to the playback buffer. Set before `playback_active` is set to true.
    playback_buffer_ptr: AtomicPtr<PlaybackBuffer>,
    /// Holds the Arc to keep the buffer alive while playback_buffer_ptr is set.
    /// Only accessed from the command thread (behind EngineHandle's Mutex).
    playback_buffer_arc: std::sync::Mutex<Option<Arc<PlaybackBuffer>>>,

    // ── Monitoring ring buffer (output callback → headphone callback) ──────
    /// Lock-free ring buffer for routing post-DSP samples to the headphone
    /// monitoring AudioUnit. Created at engine start, lives until engine stop.
    /// The output callback pushes; the headphone callback pops.
    monitoring_ring: std::sync::Mutex<Option<Arc<AudioRing>>>,
}

impl AudioEngineState {
    pub fn new() -> Self {
        // Initialise all meters to silence floor (-96 dBFS)
        let silence_bits = (-96.0f32).to_bits();
        Self {
            processing_enabled: AtomicBool::new(true),
            reboot_pending: AtomicBool::new(false),
            input_level_peak: AtomicU32::new(silence_bits),
            output_level_peak: AtomicU32::new(silence_bits),
            gate_reduction: AtomicU32::new(0.0f32.to_bits()),
            deesser_reduction: AtomicU32::new(0.0f32.to_bits()),
            deesser_sidechain_db: AtomicU32::new(silence_bits),
            comp_reduction: AtomicU32::new(0.0f32.to_bits()),
            limiter_reduction: AtomicU32::new(0.0f32.to_bits()),
            sample_rate: AtomicU32::new(48000),
            recording_active: AtomicBool::new(false),
            recording_buffer_ptr: AtomicPtr::new(std::ptr::null_mut()),
            recording_buffer_arc: std::sync::Mutex::new(None),
            playback_active: AtomicBool::new(false),
            playback_buffer_ptr: AtomicPtr::new(std::ptr::null_mut()),
            playback_buffer_arc: std::sync::Mutex::new(None),
            monitoring_ring: std::sync::Mutex::new(None),
        }
    }

    // ── Convenience helpers for f32 ─────────────────────────────────────────

    pub fn set_input_peak(&self, db: f32) {
        self.input_level_peak.store(db.to_bits(), Ordering::Relaxed);
    }

    pub fn set_output_peak(&self, db: f32) {
        self.output_level_peak.store(db.to_bits(), Ordering::Relaxed);
    }

    /// Store the negotiated sample rate (called when the engine starts).
    pub fn set_sample_rate(&self, rate: u32) {
        self.sample_rate.store(rate, Ordering::Relaxed);
    }

    /// Read the current sample rate.
    pub fn get_sample_rate(&self) -> u32 {
        self.sample_rate.load(Ordering::Relaxed)
    }

    // ── Reboot-pending gate ─────────────────────────────────────────────────

    /// Set/clear the post-install reboot-pending flag. Toggled from the
    /// command thread only — see field-level docs.
    #[inline]
    pub fn set_reboot_pending(&self, pending: bool) {
        self.reboot_pending.store(pending, Ordering::Relaxed);
    }

    /// True when the audio engine must NOT start because the user has
    /// installed/updated the driver and not yet rebooted.
    #[inline]
    pub fn is_reboot_pending(&self) -> bool {
        self.reboot_pending.load(Ordering::Relaxed)
    }

    // ── Recording helpers ──────────────────────────────────────────────────

    /// Set the recording buffer and activate recording.
    /// Called from the command thread. Uses Release ordering so the audio
    /// thread sees the buffer pointer before the active flag.
    pub fn set_recording(&self, buffer: Option<Arc<RecordingBuffer>>) {
        // SAFETY: Mutex poisoning indicates a prior panic — no recovery path.
        let mut guard = self.recording_buffer_arc.lock().unwrap();
        if let Some(ref buf) = buffer {
            // Store the raw pointer (from the Arc) into the atomic.
            // Uses Release so the audio thread sees the pointer before the active flag.
            let ptr = Arc::as_ptr(buf) as *mut RecordingBuffer;
            self.recording_buffer_ptr.store(ptr, Ordering::Release);
        }
        *guard = buffer;
        drop(guard);
        // Activate recording — MUST come after pointer is stored
        self.recording_active.store(true, Ordering::Release);
    }

    /// Clear recording state. Called from the command thread.
    pub fn clear_recording(&self) {
        self.recording_active.store(false, Ordering::Release);
        self.recording_buffer_ptr
            .store(std::ptr::null_mut(), Ordering::Release);
        // Drop the Arc
        // SAFETY: Mutex poisoning indicates a prior panic — no recovery path.
        *self.recording_buffer_arc.lock().unwrap() = None;
    }

    /// Get the recording buffer pointer for the audio callback.
    /// Returns None if recording is not active.
    ///
    /// SAFETY: The returned reference is valid as long as recording_active is true,
    /// which is guaranteed by the set/clear protocol (buffer is set before flag,
    /// flag is cleared before buffer is dropped).
    #[inline]
    pub fn get_recording_buffer(&self) -> Option<&RecordingBuffer> {
        if !self.recording_active.load(Ordering::Acquire) {
            return None;
        }
        let ptr = self.recording_buffer_ptr.load(Ordering::Acquire);
        if ptr.is_null() {
            return None;
        }
        // SAFETY: ptr is valid while recording_active is true
        Some(unsafe { &*ptr })
    }

    // ── Playback helpers ──────────────────────────────────────────────────

    /// Set the playback buffer and activate playback.
    /// Called from the command thread. Uses Release ordering so the audio
    /// thread sees the buffer pointer before the active flag.
    pub fn set_playback(&self, buffer: Arc<PlaybackBuffer>) {
        let mut guard = self.playback_buffer_arc.lock().unwrap();
        let ptr = Arc::as_ptr(&buffer) as *mut PlaybackBuffer;
        self.playback_buffer_ptr.store(ptr, Ordering::Release);
        *guard = Some(buffer);
        drop(guard);
        // Activate playback — MUST come after pointer is stored
        self.playback_active.store(true, Ordering::Release);
    }

    /// Clear playback state. Called from the command thread.
    pub fn clear_playback(&self) {
        // Deactivate first — audio thread will stop reading
        self.playback_active.store(false, Ordering::Release);
        self.playback_buffer_ptr
            .store(std::ptr::null_mut(), Ordering::Release);
        // Drop the Arc
        *self.playback_buffer_arc.lock().unwrap() = None;
    }

    /// Get the playback buffer pointer for the audio callback.
    /// Returns None if playback is not active.
    ///
    /// SAFETY: The returned reference is valid as long as playback_active is true,
    /// which is guaranteed by the set/clear protocol.
    #[inline]
    pub fn get_playback_buffer(&self) -> Option<&PlaybackBuffer> {
        if !self.playback_active.load(Ordering::Acquire) {
            return None;
        }
        let ptr = self.playback_buffer_ptr.load(Ordering::Acquire);
        if ptr.is_null() {
            return None;
        }
        // SAFETY: ptr is valid while playback_active is true
        Some(unsafe { &*ptr })
    }

    // ── Monitoring ring buffer helpers ────────────────────────────────────

    /// Set the monitoring ring buffer (called at engine start).
    pub fn set_monitoring_ring(&self, ring: Arc<AudioRing>) {
        *self.monitoring_ring.lock().unwrap() = Some(ring);
    }

    /// Get the monitoring ring buffer for use in playback callbacks.
    pub fn get_monitoring_ring(&self) -> Option<Arc<AudioRing>> {
        self.monitoring_ring.lock().unwrap().clone()
    }

    /// Clear the monitoring ring buffer (called at engine stop).
    pub fn clear_monitoring_ring(&self) {
        *self.monitoring_ring.lock().unwrap() = None;
    }
}

impl Default for AudioEngineState {
    fn default() -> Self {
        Self::new()
    }
}

/// Data transfer object for the `get_meters` IPC command.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeterData {
    /// Input peak in dBFS (range: -96 .. 0)
    pub input_level: f32,
    /// Output peak in dBFS (range: -96 .. 0)
    pub output_level: f32,
    /// Gate gain reduction in dB (currently always 0.0)
    pub gate_reduction: f32,
    /// De-esser gain reduction in dB (currently always 0.0)
    pub de_esser_reduction: f32,
    /// De-esser sidechain envelope peak in dBFS, range [-96, 0].
    pub de_esser_sidechain_db: f32,
    /// Compressor gain reduction in dB (currently always 0.0)
    pub compressor_reduction: f32,
    /// Limiter gain reduction in dB (currently always 0.0)
    pub limiter_reduction: f32,
}
