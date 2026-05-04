/// Record & playback for listening back to processed audio.
///
/// Architecture:
/// - **Recording** uses a pre-allocated `Vec<f32>` circular buffer. The audio
///   callback writes samples via an atomic write index — zero allocs, zero locks.
/// - **Stop** reads the buffer contents and writes a WAV file via `hound`.
/// - **Playback** opens a separate CoreAudio output unit on the user's headphone
///   device and plays the WAV samples through it.
///
/// All IPC commands operate on the non-real-time thread. Only the atomic flags
/// and write index cross the audio thread boundary.
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use coreaudio::audio_unit::audio_format::LinearPcmFlags;
use coreaudio::audio_unit::macos_helpers::audio_unit_from_device_id;
use coreaudio::audio_unit::render_callback::{self, data};
use coreaudio::audio_unit::{AudioUnit, Element, SampleFormat, Scope, StreamFormat};
use coreaudio::sys::kAudioUnitProperty_StreamFormat;

use uuid::Uuid;

// ────────────────────────────────────────────────────────────────────────────
// Recording buffer — shared between audio thread and command thread
// ────────────────────────────────────────────────────────────────────────────

/// Pre-allocated circular buffer for recording post-chain audio.
///
/// The audio thread writes via `push()` using an atomic write index.
/// The command thread reads via `drain()` after stopping recording.
pub struct RecordingBuffer {
    /// Pre-allocated sample storage
    data: Vec<f32>,
    /// Current write position (wraps around at capacity)
    write_pos: AtomicUsize,
    /// Total samples written (may exceed capacity — used to determine actual length)
    total_written: AtomicUsize,
    /// Capacity in samples
    capacity: usize,
}

impl RecordingBuffer {
    /// Create a new buffer for `max_seconds` at the given sample rate.
    pub fn new(max_seconds: u32, sample_rate: u32) -> Self {
        let capacity = (max_seconds as usize) * (sample_rate as usize);
        Self {
            data: vec![0.0; capacity],
            write_pos: AtomicUsize::new(0),
            total_written: AtomicUsize::new(0),
            capacity,
        }
    }

    /// Write samples from the audio callback. Zero allocations, zero locks.
    ///
    /// SAFETY: Only one thread (the audio callback) should call this.
    /// Multiple concurrent writers would corrupt the buffer.
    #[inline]
    pub fn push(&self, samples: &[f32]) {
        let mut pos = self.write_pos.load(Ordering::Relaxed);
        for &s in samples {
            // SAFETY: we only write within bounds because we mod by capacity
            unsafe {
                *self.data.as_ptr().add(pos % self.capacity).cast_mut() = s;
            }
            pos += 1;
        }
        self.write_pos.store(pos % self.capacity, Ordering::Relaxed);
        self.total_written.fetch_add(samples.len(), Ordering::Relaxed);
    }

    /// Drain the buffer contents in chronological order.
    /// Called after recording stops (from the command thread).
    pub fn drain(&self) -> Vec<f32> {
        let total = self.total_written.load(Ordering::Relaxed);
        if total == 0 {
            return Vec::new();
        }

        let actual_len = total.min(self.capacity);
        let write_pos = self.write_pos.load(Ordering::Relaxed);

        let mut result = Vec::with_capacity(actual_len);

        if total >= self.capacity {
            // Buffer wrapped — read from write_pos to end, then 0 to write_pos
            result.extend_from_slice(&self.data[write_pos..]);
            result.extend_from_slice(&self.data[..write_pos]);
        } else {
            // Buffer didn't wrap — read from 0 to write_pos
            result.extend_from_slice(&self.data[..write_pos]);
        }

        result
    }
}

// SAFETY: The buffer data is only mutated by push() on the audio thread
// via raw pointer writes. drain() reads after recording stops.
unsafe impl Send for RecordingBuffer {}
unsafe impl Sync for RecordingBuffer {}

// ────────────────────────────────────────────────────────────────────────────
// Playback buffer — shared between audio thread and command thread
// ────────────────────────────────────────────────────────────────────────────

/// Pre-loaded sample buffer for playback through the DSP chain.
///
/// The command thread loads WAV samples into this buffer. The audio callback
/// reads from it via an atomic read position that wraps at the end (looping).
/// All operations are lock-free — safe for the audio thread.
pub struct PlaybackBuffer {
    /// Pre-loaded WAV samples (immutable after creation)
    data: Vec<f32>,
    /// Current read position (wraps at `len`)
    read_pos: AtomicUsize,
    /// Number of samples in the buffer
    len: usize,
}

impl PlaybackBuffer {
    /// Create a new playback buffer from WAV samples.
    pub fn new(samples: Vec<f32>) -> Self {
        let len = samples.len();
        Self {
            data: samples,
            read_pos: AtomicUsize::new(0),
            len,
        }
    }

    /// Read samples into a destination slice, advancing the position with wrapping (loop).
    ///
    /// Called by the audio callback — zero allocations, zero locks.
    /// Returns the number of samples written (always == `dst.len()` if buffer is non-empty).
    #[inline]
    pub fn read(&self, dst: &mut [f32]) -> usize {
        if self.len == 0 {
            return 0;
        }

        let mut pos = self.read_pos.load(Ordering::Relaxed);
        for slot in dst.iter_mut() {
            *slot = self.data[pos];
            pos += 1;
            if pos >= self.len {
                pos = 0;
            }
        }
        self.read_pos.store(pos, Ordering::Relaxed);

        dst.len()
    }

    /// Number of samples in the buffer.
    #[inline]
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the buffer is empty.
    #[inline]
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Reset the read position to the start.
    #[allow(dead_code)]
    pub fn reset_position(&self) {
        self.read_pos.store(0, Ordering::Relaxed);
    }
}

// SAFETY: The data Vec is immutable after creation. Only the atomic read_pos is mutated,
// and it's accessed by a single audio thread reader.
unsafe impl Send for PlaybackBuffer {}
unsafe impl Sync for PlaybackBuffer {}

// ────────────────────────────────────────────────────────────────────────────
// RecordingManager — owns recording/playback state
// ────────────────────────────────────────────────────────────────────────────

/// Manages record and playback lifecycle.
///
/// Designed to be wrapped in a Mutex and managed by Tauri — all methods
/// are called from the command thread, never the audio thread.
pub struct RecordingManager {
    /// Directory for WAV files
    recordings_dir: PathBuf,
    /// Current recording buffer (Some when recording is active or buffer was pre-allocated)
    buffer: Option<Arc<RecordingBuffer>>,
    /// Whether recording is logically active (command-thread bookkeeping only;
    /// the audio callback reads `AudioEngineState::recording_active` instead).
    recording_active: bool,
    /// Sample rate for the current/last recording
    sample_rate: u32,
    /// ID of the last completed recording
    last_recording_id: Option<String>,
    /// Headphone monitoring AudioUnit (Some when playback is active)
    playback_unit: Option<AudioUnit>,
}

impl RecordingManager {
    /// Create a new RecordingManager with the given base data directory.
    pub fn new(app_data_dir: &std::path::Path) -> Result<Self, String> {
        let recordings_dir = app_data_dir.join("recordings");
        fs::create_dir_all(&recordings_dir)
            .map_err(|e| format!("Failed to create recordings directory: {e}"))?;

        Ok(Self {
            recordings_dir,
            buffer: None,
            recording_active: false,
            sample_rate: 48000,
            last_recording_id: None,
            playback_unit: None,
        })
    }

    /// Get the shared buffer for the audio callback (if allocated).
    #[allow(dead_code)]
    pub fn recording_buffer(&self) -> Option<Arc<RecordingBuffer>> {
        self.buffer.clone()
    }

    /// Start recording. Allocates the buffer and sets the atomic flag.
    ///
    /// Returns the shared buffer + flag for the audio callback to use.
    /// Start recording. Allocates the buffer and returns it for the audio callback.
    /// The caller (command handler) is responsible for passing the buffer to
    /// `AudioEngineState::set_recording()`, which activates the audio-thread flag.
    pub fn start_recording(
        &mut self,
        max_seconds: u32,
        sample_rate: u32,
    ) -> Result<Arc<RecordingBuffer>, String> {
        if self.recording_active {
            return Err("Recording already active".to_string());
        }

        let max_seconds = max_seconds.clamp(1, 60);
        self.sample_rate = sample_rate;

        let buffer = Arc::new(RecordingBuffer::new(max_seconds, sample_rate));
        self.buffer = Some(buffer.clone());

        self.recording_active = true;
        log::info!("Recording started: max_seconds={max_seconds}, sample_rate={sample_rate}");

        Ok(buffer)
    }

    /// Stop recording and write the buffer to a WAV file.
    ///
    /// Returns the recording ID (UUID).
    pub fn stop_recording(&mut self) -> Result<String, String> {
        if !self.recording_active {
            return Err("No active recording".to_string());
        }

        // Mark recording as inactive on the manager side.
        // The audio-thread flag is cleared by AudioEngineState::clear_recording().
        self.recording_active = false;

        let buffer = self.buffer.as_ref().ok_or("No recording buffer")?;
        let samples = buffer.drain();

        if samples.is_empty() {
            return Err("No samples recorded".to_string());
        }

        let id = Uuid::new_v4().to_string();
        let wav_path = self.recordings_dir.join(format!("{id}.wav"));

        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: self.sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };

        let mut writer = hound::WavWriter::create(&wav_path, spec)
            .map_err(|e| format!("Failed to create WAV file: {e}"))?;

        for &sample in &samples {
            writer
                .write_sample(sample)
                .map_err(|e| format!("Failed to write WAV sample: {e}"))?;
        }

        writer
            .finalize()
            .map_err(|e| format!("Failed to finalize WAV file: {e}"))?;

        let duration_s = samples.len() as f32 / self.sample_rate as f32;
        log::info!(
            "Recording saved: id={id}, duration={duration_s:.1}s, samples={}, path={}",
            samples.len(),
            wav_path.display()
        );

        self.last_recording_id = Some(id.clone());
        self.buffer = None; // Release the buffer memory

        Ok(id)
    }

    /// Play a previously recorded WAV file through the DSP chain.
    ///
    /// Loads the WAV samples into a `PlaybackBuffer`, validates sample rate,
    /// sets the buffer on `AudioEngineState`, and starts the headphone
    /// monitoring AudioUnit that reads processed output from the monitoring
    /// ring buffer.
    pub fn play_recording(
        &mut self,
        recording_id: &str,
        output_device_uid: &str,
        engine_state: &crate::engine_state::AudioEngineState,
    ) -> Result<(), String> {
        // Stop any existing playback first
        self.stop_playback(engine_state)?;

        let wav_path = self.recordings_dir.join(format!("{recording_id}.wav"));
        if !wav_path.exists() {
            return Err(format!("Recording '{recording_id}' not found"));
        }

        // Read WAV file
        let mut reader = hound::WavReader::open(&wav_path)
            .map_err(|e| format!("Failed to open WAV file: {e}"))?;
        let spec = reader.spec();
        let samples: Vec<f32> = reader
            .samples::<f32>()
            .collect::<Result<Vec<f32>, _>>()
            .map_err(|e| format!("Failed to read WAV samples: {e}"))?;

        if samples.is_empty() {
            return Err("Recording is empty".to_string());
        }

        // Validate sample rate matches the engine's negotiated rate
        let engine_sr = engine_state.get_sample_rate();
        if spec.sample_rate != engine_sr {
            return Err(format!(
                "Sample rate mismatch: recording is {} Hz but engine is {} Hz",
                spec.sample_rate, engine_sr
            ));
        }

        // Get the monitoring ring buffer from engine state
        let mon_ring = engine_state
            .get_monitoring_ring()
            .ok_or("No monitoring ring buffer — is the engine running?")?;

        // Load samples into a PlaybackBuffer and set it on engine state
        let playback_buf = Arc::new(PlaybackBuffer::new(samples));
        engine_state.set_playback(playback_buf);

        // Resolve output device
        let output_id = crate::device_manager::get_device_id_by_uid(output_device_uid)?;

        // Open a monitoring AudioUnit for the headphone device
        let mut output_au = audio_unit_from_device_id(output_id, false)
            .map_err(|e| format!("Failed to open playback device: {e}"))?;

        let stream_format = StreamFormat {
            sample_rate: engine_sr as f64,
            sample_format: SampleFormat::F32,
            flags: LinearPcmFlags::IS_FLOAT | LinearPcmFlags::IS_PACKED | LinearPcmFlags::IS_NON_INTERLEAVED,
            channels: 1,
        };

        let asbd = stream_format.to_asbd();
        output_au
            .set_property(
                kAudioUnitProperty_StreamFormat,
                Scope::Input,
                Element::Output,
                Some(&asbd),
            )
            .map_err(|e| format!("Failed to set playback stream format: {e}"))?;

        // Set up render callback — reads from monitoring ring buffer
        // (post-DSP processed samples written by the output callback)
        type OutputArgs = render_callback::Args<data::NonInterleaved<f32>>;

        output_au
            .set_render_callback(move |args: OutputArgs| {
                let num_frames = args.num_frames;
                let mut data = args.data;

                for channel in data.channels_mut() {
                    let read = mon_ring.pop(&mut channel[..num_frames], num_frames);
                    // Zero any unfilled frames (ring underrun)
                    for s in &mut channel[read..num_frames] {
                        *s = 0.0;
                    }
                }

                Ok(())
            })
            .map_err(|e| format!("Failed to set monitoring callback: {e}"))?;

        output_au
            .initialize()
            .map_err(|e| format!("Failed to initialize monitoring AudioUnit: {e}"))?;

        output_au
            .start()
            .map_err(|e| format!("Failed to start monitoring playback: {e}"))?;

        log::info!(
            "Playback started (through DSP chain): recording={recording_id}, device={output_device_uid}"
        );

        self.playback_unit = Some(output_au);

        Ok(())
    }

    /// Stop active playback (idempotent).
    pub fn stop_playback(
        &mut self,
        engine_state: &crate::engine_state::AudioEngineState,
    ) -> Result<(), String> {
        // Clear playback state first — audio thread stops reading from playback buffer
        engine_state.clear_playback();

        if let Some(mut unit) = self.playback_unit.take() {
            unit.stop()
                .map_err(|e| format!("Failed to stop playback: {e}"))?;
            log::info!("Playback stopped");
        }
        Ok(())
    }

    /// Delete a recording's WAV file from disk (idempotent).
    pub fn delete_recording(&self, recording_id: &str) -> Result<(), String> {
        let wav_path = self.recordings_dir.join(format!("{recording_id}.wav"));
        if wav_path.exists() {
            fs::remove_file(&wav_path)
                .map_err(|e| format!("Failed to delete recording: {e}"))?;
            log::info!("Recording deleted: {recording_id}");
        }
        Ok(())
    }

    /// Delete recordings older than 24 hours. Called on app launch.
    pub fn cleanup_old_recordings(&self) -> Result<u32, String> {
        let mut deleted = 0u32;
        let cutoff = std::time::SystemTime::now()
            - std::time::Duration::from_secs(24 * 60 * 60);

        let entries = fs::read_dir(&self.recordings_dir)
            .map_err(|e| format!("Failed to read recordings dir: {e}"))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("wav") {
                continue;
            }
            if let Ok(meta) = fs::metadata(&path) {
                if let Ok(modified) = meta.modified() {
                    if modified < cutoff && fs::remove_file(&path).is_ok() {
                        deleted += 1;
                        log::info!("Cleaned up old recording: {}", path.display());
                    }
                }
            }
        }

        if deleted > 0 {
            log::info!("Cleaned up {deleted} old recording(s)");
        }
        Ok(deleted)
    }

    /// Whether recording is currently active.
    #[allow(dead_code)]
    pub fn is_recording(&self) -> bool {
        self.recording_active
    }

    /// Whether playback is currently active.
    #[allow(dead_code)]
    pub fn is_playing(&self) -> bool {
        self.playback_unit.is_some()
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a buffer for `seconds` at 100 Hz (small for tests).
    fn test_buffer(seconds: u32) -> RecordingBuffer {
        RecordingBuffer::new(seconds, 100)
    }

    #[test]
    fn push_drain_roundtrip_no_wrap() {
        let buf = test_buffer(1); // capacity = 100
        let samples: Vec<f32> = (0..50).map(|i| i as f32).collect();
        buf.push(&samples);

        let out = buf.drain();
        assert_eq!(out.len(), 50);
        assert_eq!(out, samples);
    }

    #[test]
    fn push_drain_with_wraparound() {
        let buf = test_buffer(1); // capacity = 100
        // Write 150 samples — buffer wraps, keeping the last 100
        let samples: Vec<f32> = (0..150).map(|i| i as f32).collect();
        buf.push(&samples);

        let out = buf.drain();
        assert_eq!(out.len(), 100);
        // Should contain samples 50..150 in chronological order
        let expected: Vec<f32> = (50..150).map(|i| i as f32).collect();
        assert_eq!(out, expected);
    }

    #[test]
    fn push_exact_capacity() {
        let buf = test_buffer(1); // capacity = 100
        let samples: Vec<f32> = (0..100).map(|i| i as f32).collect();
        buf.push(&samples);

        let out = buf.drain();
        assert_eq!(out.len(), 100);
        assert_eq!(out, samples);
    }

    #[test]
    fn drain_when_empty_returns_empty() {
        let buf = test_buffer(1);
        let out = buf.drain();
        assert!(out.is_empty());
    }

    #[test]
    fn push_single_sample_at_a_time() {
        let buf = test_buffer(1); // capacity = 100
        for i in 0..30 {
            buf.push(&[i as f32]);
        }

        let out = buf.drain();
        assert_eq!(out.len(), 30);
        let expected: Vec<f32> = (0..30).map(|i| i as f32).collect();
        assert_eq!(out, expected);
    }

    #[test]
    fn multiple_pushes_then_drain() {
        let buf = test_buffer(1); // capacity = 100
        buf.push(&[1.0, 2.0, 3.0]);
        buf.push(&[4.0, 5.0]);
        buf.push(&[6.0]);

        let out = buf.drain();
        assert_eq!(out, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn wraparound_with_multiple_pushes() {
        let buf = test_buffer(1); // capacity = 100
        // Fill to 90
        let first: Vec<f32> = (0..90).map(|i| i as f32).collect();
        buf.push(&first);
        // Push 20 more — wraps around, overwriting first 10
        let second: Vec<f32> = (90..110).map(|i| i as f32).collect();
        buf.push(&second);

        let out = buf.drain();
        assert_eq!(out.len(), 100);
        // Should contain samples 10..110
        let expected: Vec<f32> = (10..110).map(|i| i as f32).collect();
        assert_eq!(out, expected);
    }

    #[test]
    fn total_written_tracks_correctly() {
        let buf = test_buffer(1);
        buf.push(&[1.0; 25]);
        buf.push(&[2.0; 25]);
        assert_eq!(buf.total_written.load(Ordering::Relaxed), 50);

        buf.push(&[3.0; 100]);
        assert_eq!(buf.total_written.load(Ordering::Relaxed), 150);
    }

    // ── PlaybackBuffer ──────────────────────────────────────────────────────

    #[test]
    fn playback_read_basic() {
        let samples: Vec<f32> = (0..10).map(|i| i as f32).collect();
        let pb = PlaybackBuffer::new(samples.clone());
        let mut dst = vec![0.0f32; 5];
        let n = pb.read(&mut dst);
        assert_eq!(n, 5);
        assert_eq!(dst, vec![0.0, 1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn playback_read_wraps_around() {
        let samples: Vec<f32> = (0..4).map(|i| i as f32).collect();
        let pb = PlaybackBuffer::new(samples);
        let mut dst = vec![0.0f32; 6];
        let n = pb.read(&mut dst);
        assert_eq!(n, 6);
        // Should wrap: 0, 1, 2, 3, 0, 1
        assert_eq!(dst, vec![0.0, 1.0, 2.0, 3.0, 0.0, 1.0]);
    }

    #[test]
    fn playback_read_sequential_calls_continue() {
        let samples: Vec<f32> = (0..6).map(|i| i as f32).collect();
        let pb = PlaybackBuffer::new(samples);

        let mut dst1 = vec![0.0f32; 3];
        pb.read(&mut dst1);
        assert_eq!(dst1, vec![0.0, 1.0, 2.0]);

        let mut dst2 = vec![0.0f32; 3];
        pb.read(&mut dst2);
        assert_eq!(dst2, vec![3.0, 4.0, 5.0]);

        // Next call should wrap
        let mut dst3 = vec![0.0f32; 2];
        pb.read(&mut dst3);
        assert_eq!(dst3, vec![0.0, 1.0]);
    }

    #[test]
    fn playback_empty_buffer_returns_zero() {
        let pb = PlaybackBuffer::new(vec![]);
        let mut dst = vec![0.0f32; 5];
        let n = pb.read(&mut dst);
        assert_eq!(n, 0);
    }

    #[test]
    fn playback_reset_position() {
        let samples: Vec<f32> = (0..4).map(|i| i as f32).collect();
        let pb = PlaybackBuffer::new(samples);

        let mut dst = vec![0.0f32; 3];
        pb.read(&mut dst);
        assert_eq!(dst, vec![0.0, 1.0, 2.0]);

        pb.reset_position();
        pb.read(&mut dst);
        assert_eq!(dst, vec![0.0, 1.0, 2.0]);
    }
}
