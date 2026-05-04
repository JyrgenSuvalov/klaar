//! Lock-free positional ring buffer for audio sample transfer.
//!
//! Shared-memory positional buffer indexed by absolute frame number (the
//! CoreAudio `AudioTimeStamp::mSampleTime` semantic). The writer places samples
//! at `(sample_time as usize) & frame_mask`; the reader retrieves samples from
//! the same frame address. Writer and reader agree on the absolute frame
//! position via CoreAudio's safety-offset contract, so there is no producer /
//! consumer race in the FIFO sense and no read/write cursors are required.
//!
//! Writes go to `outputTime.mSampleTime % size`, reads come from
//! `inputTime.mSampleTime % size`. Latency between WriteMix and ReadInput is
//! determined entirely by `outputTime - inputTime` (both computed by CoreAudio
//! from the device's safety offsets, which are 0), which converges latency
//! to ~0.
//!
//! - Underrun: a slot that has not been populated by any `write_at` since the
//!   last `reset()` contains `0.0f32` samples. This is achieved by `reset()`
//!   zero-filling the entire underlying storage.
//! - "Overrun": `write_at(T + capacity_frames, data)` deterministically
//!   overwrites the slot at `write_at(T, ...)` — both sample-times map to the
//!   same frame index under the wrap. At steady state this is correct: the
//!   slot is always being refilled before the next read.

use std::cell::UnsafeCell;
use std::sync::atomic::{compiler_fence, Ordering};

use crate::constants::CHANNEL_COUNT;

/// Lock-free positional ring buffer for interleaved `f32` audio samples.
///
/// Thread safety contract:
/// - Exactly one thread may call `write_at()` (the producer / WriteMix thread).
/// - Exactly one thread may call `read_at()` (the consumer / ReadInput thread).
/// - `reset()` and `resize()` must only be called when no IO is active.
///
/// Capacity is expressed in **frames** and is always a power of two so that
/// position masking uses bitwise AND instead of modulo. Each frame holds
/// `CHANNEL_COUNT` interleaved `f32` samples.
pub struct RingBuffer {
    /// Sample storage — `capacity_frames * CHANNEL_COUNT` interleaved `f32`s.
    /// Allocated once, resized only during config changes with IO stopped.
    ///
    /// Wrapped in `UnsafeCell` because `write_at()` mutates the contents
    /// through a shared `&self` reference. Without the `UnsafeCell`, deriving a
    /// `*mut f32` from `&Vec<f32>` and writing through it would violate Rust's
    /// aliasing rules (UB even though the positional-addressing access pattern
    /// prevents actual data races).
    buffer: UnsafeCell<Vec<f32>>,
    /// Bitmask for wrapping frame positions: `capacity_frames - 1`.
    frame_mask: usize,
    /// Capacity in frames (power of two).
    capacity_frames: usize,
}

impl RingBuffer {
    /// Create a new positional ring buffer with the given capacity in frames.
    ///
    /// `capacity_frames` is rounded up to the next power of two if not already
    /// one. The underlying storage holds `capacity_frames * CHANNEL_COUNT`
    /// `f32` samples, zero-initialised so that reads of never-written slots
    /// return silence.
    ///
    /// Panics if `capacity_frames` is 0.
    pub fn new(capacity_frames: usize) -> Self {
        assert!(capacity_frames > 0, "Ring buffer capacity must be > 0");
        let capacity_frames = capacity_frames.next_power_of_two();
        let sample_count = capacity_frames * CHANNEL_COUNT as usize;
        Self {
            buffer: UnsafeCell::new(vec![0.0f32; sample_count]),
            frame_mask: capacity_frames - 1,
            capacity_frames,
        }
    }

    /// Write interleaved samples into the slot keyed by absolute `sample_time`.
    ///
    /// `sample_time` is a frame count (matches `AudioTimeStamp::mSampleTime`).
    /// The frame slot is `(sample_time as usize) & frame_mask`. `data.len()`
    /// must be a multiple of `CHANNEL_COUNT`; callers pass
    /// `inIOBufferFrameSize * CHANNEL_COUNT` interleaved samples.
    ///
    /// If the write straddles the end of the underlying buffer, it splits into
    /// two `copy_nonoverlapping` calls.
    ///
    /// # Real-time safety
    /// No allocations, no locks, no syscalls. O(n) copy only.
    pub fn write_at(&self, sample_time: u64, data: &[f32]) {
        let frame_index = (sample_time as usize) & self.frame_mask;
        let start_sample = frame_index * CHANNEL_COUNT as usize;
        let sample_count = self.capacity_frames * CHANNEL_COUNT as usize;
        let len = data.len();

        // If the caller ever passes more than one full buffer's worth of data,
        // silently clamp — the trailing samples would alias across the wrap.
        // In practice `len == io_buffer_frame_size * CHANNEL_COUNT` which is
        // always much less than `sample_count`, but this guard costs nothing.
        let len = len.min(sample_count);

        // SAFETY: The SPSC / positional contract guarantees there is no
        // concurrent mutation of the same frame slot. The underlying `Vec<f32>`
        // is never reallocated while IO is active. Mutation through
        // `UnsafeCell::get()` is legal because the buffer is wrapped in
        // `UnsafeCell`.
        let buf_ptr = unsafe { (*self.buffer.get()).as_mut_ptr() };
        let first_chunk = (sample_count - start_sample).min(len);

        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), buf_ptr.add(start_sample), first_chunk);

            if first_chunk < len {
                std::ptr::copy_nonoverlapping(
                    data.as_ptr().add(first_chunk),
                    buf_ptr,
                    len - first_chunk,
                );
            }
        }

        // A Release fence after the stores ensures any subsequent Acquire fence
        // in the reader observes these writes. There are no atomic positions to
        // update — writer and reader agree on absolute frame positions via the
        // `sample_time` argument passed by CoreAudio.
        compiler_fence(Ordering::Release);
    }

    /// Read interleaved samples from the slot keyed by absolute `sample_time`.
    ///
    /// Symmetric to `write_at`. If no `write_at` has populated this slot since
    /// the last `reset()`, `out` is filled with zeros (because `reset()`
    /// zero-fills the underlying storage).
    ///
    /// `out.len()` must be a multiple of `CHANNEL_COUNT`; callers pass
    /// `inIOBufferFrameSize * CHANNEL_COUNT` samples.
    ///
    /// # Real-time safety
    /// No allocations, no locks, no syscalls. O(n) copy only.
    pub fn read_at(&self, sample_time: u64, out: &mut [f32]) {
        // Acquire fence before the loads ensures we observe any Release stores
        // the writer has performed.
        compiler_fence(Ordering::Acquire);

        let frame_index = (sample_time as usize) & self.frame_mask;
        let start_sample = frame_index * CHANNEL_COUNT as usize;
        let sample_count = self.capacity_frames * CHANNEL_COUNT as usize;
        let len = out.len().min(sample_count);

        // SAFETY: Reading via `UnsafeCell::get()` — the writer only mutates
        // slots it owns under the positional contract; our read addresses do
        // not overlap with a concurrent write of the same slot at steady
        // state. The underlying `Vec<f32>` is not reallocated while IO is
        // active.
        let buf_ptr = unsafe { (*self.buffer.get()).as_ptr() };
        let first_chunk = (sample_count - start_sample).min(len);

        unsafe {
            std::ptr::copy_nonoverlapping(buf_ptr.add(start_sample), out.as_mut_ptr(), first_chunk);

            if first_chunk < len {
                std::ptr::copy_nonoverlapping(
                    buf_ptr,
                    out.as_mut_ptr().add(first_chunk),
                    len - first_chunk,
                );
            }
        }
    }

    /// Zero-fill the entire underlying storage.
    ///
    /// With positional indexing there are no read/write cursors to reset —
    /// silence at a given `sample_time` must be represented by an actual zero
    /// in storage. A subsequent `read_at(T, out)` for any slot that has not
    /// been populated by a `write_at` since this call will return zeros.
    ///
    /// # Real-time safety
    /// **Not RT-safe.** This is a full-buffer memset (at default sizing:
    /// 131_072 `f32`s = 512 KiB). Call only during `StartIO` first-client
    /// transition or `PerformDeviceConfigurationChange`, never from the RT
    /// thread.
    ///
    /// # Safety contract
    /// Must only be called when IO is not active (no concurrent `read_at` /
    /// `write_at` calls).
    pub fn reset(&self) {
        // SAFETY: Caller guarantees no concurrent IO. `UnsafeCell::get()` plus
        // the contract means we have exclusive access here.
        let buf = unsafe { &mut *self.buffer.get() };
        buf.fill(0.0);
    }

    /// Resize the ring buffer to a new capacity in frames.
    ///
    /// `capacity_frames` is rounded up to the next power of two. All existing
    /// data is discarded and storage is zero-filled.
    ///
    /// # Real-time safety
    /// **Not RT-safe.** Reallocates the underlying `Vec<f32>` and zero-fills
    /// it. Call only during `PerformDeviceConfigurationChange` with IO
    /// stopped.
    ///
    /// # Safety contract
    /// Must only be called when IO is not active.
    pub fn resize(&mut self, capacity_frames: usize) {
        assert!(capacity_frames > 0, "Ring buffer capacity must be > 0");
        let capacity_frames = capacity_frames.next_power_of_two();
        let sample_count = capacity_frames * CHANNEL_COUNT as usize;
        // &mut self gives exclusive access, so UnsafeCell::get_mut() is safe.
        let buf = self.buffer.get_mut();
        buf.resize(sample_count, 0.0);
        buf.fill(0.0);
        self.frame_mask = capacity_frames - 1;
        self.capacity_frames = capacity_frames;
    }

    /// Capacity in frames (power of two). Test-only helper.
    #[cfg(test)]
    pub fn capacity_frames(&self) -> usize {
        self.capacity_frames
    }
}

// SAFETY: RingBuffer is safe to share between threads because:
// - The positional API means writer and reader address absolute frame slots
//   agreed upon externally (via CoreAudio `mSampleTime`), not producer /
//   consumer cursors. There are no atomics to race on.
// - The `Vec<f32>` buffer is wrapped in `UnsafeCell`, so interior mutation
//   through `&self` is legal under Rust's aliasing rules.
// - The buffer is never reallocated while shared (`resize` takes `&mut self`).
// - At steady state CoreAudio's safety-offset contract ensures writer and
//   reader operate on different frame slots at any instant. Compiler fences
//   with Acquire/Release ordering on storage ensure store-store / load-load
//   ordering where needed.
//
// UnsafeCell is !Sync by default, so we must manually assert Sync for
// RingBuffer.
unsafe impl Sync for RingBuffer {}
unsafe impl Send for RingBuffer {}
