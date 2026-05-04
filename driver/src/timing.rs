//! Virtual clock for CoreAudio `GetZeroTimeStamp`.
//!
//! Uses `mach_absolute_time()` as the time source. The clock is anchored
//! when the first IO client starts, producing monotonically advancing
//! `(sample_time, host_time, seed)` tuples aligned to `zero_timestamp_period`.
//!
//! # Real-time safety
//!
//! `get_zero_timestamp()` is called on CoreAudio's real-time IO thread.
//! It uses only atomic loads, `mach_absolute_time()`, and arithmetic.
//! No allocations, no locks, no syscalls beyond the Mach time call.

use std::sync::atomic::{AtomicU64, Ordering};

/// Mach timebase ratio (nanoseconds per tick = numer/denom).
/// Cached at first use to avoid repeated `mach_timebase_info()` calls.
struct TimebaseInfo {
    numer: u64,
    denom: u64,
}

impl TimebaseInfo {
    fn query() -> Self {
        let mut info = mach2::mach_time::mach_timebase_info_data_t { numer: 0, denom: 0 };
        // SAFETY: mach_timebase_info is always safe to call and fills the struct.
        unsafe {
            mach2::mach_time::mach_timebase_info(&mut info);
        }
        Self {
            numer: info.numer as u64,
            denom: info.denom as u64,
        }
    }
}

/// Virtual clock for producing `GetZeroTimeStamp` results.
///
/// The clock anchors at a host time (from `mach_absolute_time()`) and computes
/// timestamps relative to that anchor. Timestamps advance by exactly
/// `zero_timestamp_period` frames per period.
///
/// `VirtualClock` is automatically `Send + Sync` because all fields are either
/// atomics or immutable after construction (`TimebaseInfo` never changes).
pub struct VirtualClock {
    /// Host time at which the clock was anchored (first StartIO)
    anchor_host_time: AtomicU64,
    /// Clock seed — increments on configuration changes (sample rate)
    seed: AtomicU64,
    /// Cached Mach-ticks-per-sample-frame, stored as `f64::to_bits`.
    /// Recomputed only when the anchor is set or the clock is reset, so
    /// `get_zero_timestamp` on the RT thread avoids a divide per cycle.
    ticks_per_frame_bits: AtomicU64,
    /// Cached timebase info (immutable after construction)
    timebase: TimebaseInfo,
}

impl Default for VirtualClock {
    fn default() -> Self {
        Self::new()
    }
}

impl VirtualClock {
    /// Create a new virtual clock. Queries `mach_timebase_info` once.
    pub fn new() -> Self {
        Self {
            anchor_host_time: AtomicU64::new(0),
            seed: AtomicU64::new(1),
            ticks_per_frame_bits: AtomicU64::new(0),
            timebase: TimebaseInfo::query(),
        }
    }

    /// Recompute `ticks_per_frame` for the given sample rate and store it.
    ///
    /// Called by `anchor` and `reset`, i.e. only on non-RT threads during
    /// `StartIO` or configuration change flow.
    fn update_ticks_per_frame(&self, sample_rate: f64) {
        // ticks_per_sec   = denom * 1_000_000_000 / numer
        // ticks_per_frame = ticks_per_sec / sample_rate
        //                 = (denom * 1_000_000_000) / (numer * sample_rate)
        let ticks_per_frame = (self.timebase.denom as f64 * 1_000_000_000.0)
            / (self.timebase.numer as f64 * sample_rate);
        self.ticks_per_frame_bits
            .store(ticks_per_frame.to_bits(), Ordering::Release);
    }

    /// Anchor the clock at the given host time for the given sample rate.
    /// Called on first `StartIO` (0→1 transition).
    pub fn anchor(&self, host_time: u64, sample_rate: f64) {
        self.update_ticks_per_frame(sample_rate);
        self.anchor_host_time.store(host_time, Ordering::Release);
    }

    /// Get the current `mach_absolute_time()`.
    pub fn now() -> u64 {
        // SAFETY: mach_absolute_time() is always safe and real-time safe.
        unsafe { mach2::mach_time::mach_absolute_time() }
    }

    /// Compute the zero timestamp for the current time.
    ///
    /// Returns `(sample_time, host_time, seed)`:
    /// - `sample_time`: the sample count at the most recent period boundary,
    ///   always a multiple of `period`
    /// - `host_time`: the host tick corresponding to that sample time
    /// - `seed`: clock identity; changes when the clock is reset
    ///
    /// # Real-time safety
    /// Uses only atomic loads, `mach_absolute_time()`, and arithmetic. The
    /// `ticks_per_frame` value is cached — updated only on anchor/reset — so
    /// this path has no floating-point division on the RT thread.
    pub fn get_zero_timestamp(&self, period: u32) -> (f64, u64, u64) {
        let anchor = self.anchor_host_time.load(Ordering::Acquire);
        let seed = self.seed.load(Ordering::Acquire);
        let ticks_per_frame =
            f64::from_bits(self.ticks_per_frame_bits.load(Ordering::Acquire));
        let now = Self::now();

        if anchor == 0 || now < anchor || ticks_per_frame <= 0.0 {
            // Clock not yet anchored, time went backwards (shouldn't happen),
            // or ticks_per_frame not yet initialized
            return (0.0, anchor, seed);
        }

        // Elapsed host ticks since anchor
        let elapsed_ticks = now - anchor;

        // Convert to elapsed frames
        let elapsed_frames = elapsed_ticks as f64 / ticks_per_frame;

        // Quantize to period boundaries
        let period_f64 = period as f64;
        let completed_periods = (elapsed_frames / period_f64).floor();
        let sample_time = completed_periods * period_f64;

        // Compute the host time corresponding to this sample_time
        let host_time = anchor + (sample_time * ticks_per_frame) as u64;

        (sample_time, host_time, seed)
    }

    /// Increment the clock seed. Called during configuration changes
    /// (e.g., sample rate change) to signal that the clock has been reset.
    pub fn increment_seed(&self) {
        self.seed.fetch_add(1, Ordering::AcqRel);
    }

    /// Re-anchor the clock for a new sample rate and increment the seed.
    /// Used during configuration changes to reset timing.
    pub fn reset(&self, sample_rate: f64) {
        let now = Self::now();
        self.anchor(now, sample_rate);
        self.increment_seed();
    }

    /// Get the current seed value.
    pub fn seed(&self) -> u64 {
        self.seed.load(Ordering::Acquire)
    }
}
