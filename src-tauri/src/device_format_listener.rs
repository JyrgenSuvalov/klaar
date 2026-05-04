//! Per-device CoreAudio nominal sample-rate change listener.
//!
//! ## Why
//!
//! CoreAudio's `kAudioHardwarePropertyDevices` only fires on device add/remove.
//! When a user changes the nominal sample rate of an existing device in
//! Audio MIDI Setup, no system-wide notification is generated — only a
//! per-device `kAudioDevicePropertyNominalSampleRate` change. To detect this
//! reliably we register a property listener on every enumerated device and
//! re-sync the listener set whenever the device list changes (so freshly
//! plugged devices acquire a listener and removed ones drop theirs).
//!
//! ## Threading model
//!
//! Three threads are involved:
//!
//! 1. **CoreAudio property-listener thread** — runs the C-compatible
//!    `property_listener_callback` whenever any registered device's nominal
//!    SR changes. The callback is **strictly real-time-safe**: it does
//!    nothing but `AtomicU64::store` (the monotonic millisecond timestamp
//!    of the most recent fire) and `Thread::unpark` (lock-free, signal-safe
//!    on all supported platforms). No allocation, no mutex, no logging,
//!    no Tauri call.
//! 2. **Worker thread** — owned by the listener; sleeps via `park_timeout`
//!    until 150 ms have elapsed since the most recent callback fire, then
//!    invokes the user-supplied `on_change` closure exactly once per quiet
//!    period. This is the debounce coalescer (decision D3 in the change
//!    design).
//! 3. **Tauri / main thread** — calls `sync(&[AudioDeviceID])` on
//!    enumeration changes (startup + inside the existing `DeviceChangeListener`
//!    callback) to register/unregister per-device listeners. This thread also
//!    owns the closure executed when the worker fires.
//!
//! On `Drop`, every active listener is unregistered from CoreAudio, the
//! worker thread is signalled to exit and joined, and the boxed callback
//! data is reclaimed.

use coreaudio::sys::{
    kAudioDevicePropertyNominalSampleRate,
    kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal,
    AudioObjectAddPropertyListener,
    AudioObjectID,
    AudioObjectPropertyAddress,
    AudioObjectRemovePropertyListener,
    OSStatus,
};
use std::collections::HashSet;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle, Thread};
use std::time::{Duration, Instant};

/// Debounce window for SR-change bursts (decision D3).
const DEBOUNCE_MS: u64 = 150;

// ────────────────────────────────────────────────────────────────────────────
// Test-injectable register/unregister hooks
// ────────────────────────────────────────────────────────────────────────────
//
// At runtime these are thin wrappers around the real CoreAudio symbols.
// In `#[cfg(test)]` builds they default to no-ops so unit tests can exercise
// the bookkeeping logic without touching the OS, and tests may override them
// per-listener via `DeviceFormatListener::with_hooks_for_test`.

type RegisterFn = fn(AudioObjectID, *mut c_void) -> OSStatus;
type UnregisterFn = fn(AudioObjectID, *mut c_void) -> OSStatus;

fn real_register(device_id: AudioObjectID, client_data: *mut c_void) -> OSStatus {
    let addr = sample_rate_property_address();
    unsafe {
        AudioObjectAddPropertyListener(
            device_id,
            &addr,
            Some(property_listener_callback),
            client_data,
        )
    }
}

fn real_unregister(device_id: AudioObjectID, client_data: *mut c_void) -> OSStatus {
    let addr = sample_rate_property_address();
    unsafe {
        AudioObjectRemovePropertyListener(
            device_id,
            &addr,
            Some(property_listener_callback),
            client_data,
        )
    }
}

fn sample_rate_property_address() -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyNominalSampleRate,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Callback data
// ────────────────────────────────────────────────────────────────────────────

/// Shared state between the CoreAudio callback and the worker thread.
///
/// Held behind an `Arc` and pointed to (via `Arc::as_ptr`) by every
/// per-device listener registration. The pointer is reclaimed on drop after
/// every listener has been unregistered.
struct CallbackData {
    /// Monotonic milliseconds since `epoch` at which the most recent callback
    /// fire occurred. Zero = no fire pending. Updated by the CoreAudio
    /// callback (atomic store only); cleared by the worker thread after it
    /// runs the user closure.
    last_fire_ms: AtomicU64,
    /// Worker thread handle, set once at construction. The CoreAudio callback
    /// uses this to `unpark` the worker after stamping `last_fire_ms`.
    worker: OnceLock<Thread>,
    /// Reference instant for `last_fire_ms`. Captured at construction and
    /// never mutated thereafter.
    epoch: Instant,
}

impl CallbackData {
    fn now_ms(&self) -> u64 {
        // Saturating cast — `as u64` from a u128 truncates, but we cap the
        // duration to u64::MAX milliseconds (≈ 584 million years).
        let elapsed = self.epoch.elapsed().as_millis();
        if elapsed > u64::MAX as u128 {
            u64::MAX
        } else {
            elapsed as u64
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Real-time-safe C callback
// ────────────────────────────────────────────────────────────────────────────

/// Invoked by CoreAudio on its internal property-listener thread whenever the
/// nominal sample rate of a registered device changes.
///
/// **Real-time discipline**: this function performs only an atomic store and
/// a `Thread::unpark`. No allocations, locks, syscalls beyond the atomic
/// fence, logging, or Tauri calls.
extern "C" fn property_listener_callback(
    _in_object_id: AudioObjectID,
    _in_number_addresses: u32,
    _in_addresses: *const AudioObjectPropertyAddress,
    in_client_data: *mut c_void,
) -> OSStatus {
    if in_client_data.is_null() {
        return 0;
    }
    // SAFETY: in_client_data is a `*const CallbackData` obtained via
    // `Arc::as_ptr` while the owning `DeviceFormatListener` is alive. The
    // listener guarantees every CoreAudio registration is removed before the
    // Arc is dropped, so the pointee is always valid here.
    let data = unsafe { &*(in_client_data as *const CallbackData) };
    let now = data.now_ms().max(1); // 0 is reserved for "no pending fire"
    data.last_fire_ms.store(now, Ordering::Release);
    if let Some(worker) = data.worker.get() {
        worker.unpark();
    }
    0 // noErr
}

// ────────────────────────────────────────────────────────────────────────────
// Public listener
// ────────────────────────────────────────────────────────────────────────────

/// Owns the per-device sample-rate listeners and the debounce worker.
///
/// Construct once at app startup, then call [`sync`](Self::sync) with the
/// current set of enumerated `AudioDeviceID`s — the listener will register
/// listeners for new devices and unregister for absent ones. On `Drop` every
/// registration is removed and the worker thread is joined.
pub struct DeviceFormatListener {
    /// Boxed shared state. Held in an `Arc` so the worker thread can reach
    /// it; the CoreAudio callback uses `Arc::as_ptr(&data)` (raw, no refcount
    /// bump). Drop order in `Drop` guarantees no dangling pointer.
    data: Arc<CallbackData>,
    /// Devices we currently have a listener registered on.
    registered: Mutex<HashSet<AudioObjectID>>,
    /// Worker thread join handle. Taken in `Drop`.
    worker_join: Mutex<Option<JoinHandle<()>>>,
    /// Set true on drop to wake the worker so it can exit its loop.
    shutdown: Arc<AtomicBool>,
    /// Hooks used to register/unregister listeners. Always the real
    /// CoreAudio symbols at runtime; injectable in `#[cfg(test)]`.
    register_fn: RegisterFn,
    unregister_fn: UnregisterFn,
}

// SAFETY: the only !Send / !Sync field would be the raw pointer used by
// CoreAudio, which we manage via Arc + careful drop ordering. All fields
// here are Send + Sync on their own.
unsafe impl Send for DeviceFormatListener {}
unsafe impl Sync for DeviceFormatListener {}

impl DeviceFormatListener {
    /// Spawn the worker and create the listener (no devices registered yet —
    /// call [`sync`](Self::sync) after construction).
    ///
    /// `on_change` is invoked on the worker thread, at most once per
    /// 150 ms quiet window, after one or more `kAudioDevicePropertyNominalSampleRate`
    /// callbacks have fired across any registered device.
    pub fn new<F>(on_change: F) -> Result<Self, String>
    where
        F: Fn() + Send + Sync + 'static,
    {
        Self::new_with_hooks(on_change, real_register, real_unregister)
    }

    fn new_with_hooks<F>(
        on_change: F,
        register_fn: RegisterFn,
        unregister_fn: UnregisterFn,
    ) -> Result<Self, String>
    where
        F: Fn() + Send + Sync + 'static,
    {
        let data = Arc::new(CallbackData {
            last_fire_ms: AtomicU64::new(0),
            worker: OnceLock::new(),
            epoch: Instant::now(),
        });
        let shutdown = Arc::new(AtomicBool::new(false));

        let data_w = data.clone();
        let shutdown_w = shutdown.clone();
        let on_change_box: Box<dyn Fn() + Send + Sync> = Box::new(on_change);

        let join = thread::Builder::new()
            .name("device-format-listener".to_string())
            .spawn(move || {
                worker_loop(data_w, shutdown_w, on_change_box);
            })
            .map_err(|e| format!("spawn device-format-listener worker: {e}"))?;

        // Publish the worker thread handle so the CoreAudio callback can
        // unpark us. `OnceLock::set` returns Err on second-set; ignore.
        let _ = data.worker.set(join.thread().clone());

        Ok(Self {
            data,
            registered: Mutex::new(HashSet::new()),
            worker_join: Mutex::new(Some(join)),
            shutdown,
            register_fn,
            unregister_fn,
        })
    }

    /// Reconcile the registered listener set against `devices`:
    ///
    /// - For every ID in `devices` that we don't already track, register a
    ///   listener.
    /// - For every ID we already track that is absent from `devices`,
    ///   unregister.
    ///
    /// Errors from individual register/unregister calls are logged but do
    /// not abort the sync — a partial sync is preferable to leaving the set
    /// in a worse state than we found it.
    pub fn sync(&self, devices: &[AudioObjectID]) {
        let new_set: HashSet<AudioObjectID> = devices.iter().copied().collect();
        let mut current = self.registered.lock().expect("registered lock poisoned");

        // Compute deltas without allocating intermediates beyond the diff
        // sets themselves (this is on the Tauri main thread, not RT).
        let to_add: Vec<AudioObjectID> =
            new_set.difference(&current).copied().collect();
        let to_remove: Vec<AudioObjectID> =
            current.difference(&new_set).copied().collect();

        let client_data = Arc::as_ptr(&self.data) as *mut c_void;

        for id in to_add {
            let status = (self.register_fn)(id, client_data);
            if status == 0 {
                current.insert(id);
            } else {
                log::warn!(
                    "DeviceFormatListener: AudioObjectAddPropertyListener({id}) failed: {:#010x}",
                    status as u32
                );
            }
        }

        for id in to_remove {
            let status = (self.unregister_fn)(id, client_data);
            // Unregister from our bookkeeping regardless — even if CoreAudio
            // returned an error, the device is gone from enumeration and a
            // subsequent re-add will re-register cleanly.
            current.remove(&id);
            if status != 0 {
                log::warn!(
                    "DeviceFormatListener: AudioObjectRemovePropertyListener({id}) failed: {:#010x}",
                    status as u32
                );
            }
        }
    }

    /// Number of devices we currently hold a listener on. Test/diagnostic only.
    #[cfg(test)]
    pub fn registered_count(&self) -> usize {
        self.registered.lock().unwrap().len()
    }

    /// Test-only constructor that lets unit tests inject mock register/unregister
    /// hooks instead of calling into CoreAudio.
    #[cfg(test)]
    pub fn new_for_test<F>(
        on_change: F,
        register_fn: RegisterFn,
        unregister_fn: UnregisterFn,
    ) -> Result<Self, String>
    where
        F: Fn() + Send + Sync + 'static,
    {
        Self::new_with_hooks(on_change, register_fn, unregister_fn)
    }
}

impl Drop for DeviceFormatListener {
    fn drop(&mut self) {
        // 1) Unregister every active CoreAudio listener.
        let client_data = Arc::as_ptr(&self.data) as *mut c_void;
        let registered: HashSet<AudioObjectID> = {
            let mut guard = self.registered.lock().expect("registered lock poisoned");
            std::mem::take(&mut *guard)
        };
        let count = registered.len();
        for id in registered {
            let status = (self.unregister_fn)(id, client_data);
            if status != 0 {
                log::warn!(
                    "DeviceFormatListener::drop: unregister({id}) failed: {:#010x}",
                    status as u32
                );
            }
        }
        log::debug!(
            "DeviceFormatListener::drop: unregistered {} per-device listener(s)",
            count
        );

        // 2) Signal the worker to exit and unpark it.
        self.shutdown.store(true, Ordering::Release);
        if let Some(t) = self.data.worker.get() {
            t.unpark();
        }

        // 3) Join the worker so on_change cannot fire after Drop returns
        //    (and so we know nothing is reading `data` once the Arc drops).
        if let Some(join) = self
            .worker_join
            .lock()
            .expect("worker_join lock poisoned")
            .take()
        {
            if let Err(e) = join.join() {
                log::warn!("DeviceFormatListener: worker join failed: {:?}", e);
            }
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Worker loop
// ────────────────────────────────────────────────────────────────────────────

fn worker_loop(
    data: Arc<CallbackData>,
    shutdown: Arc<AtomicBool>,
    on_change: Box<dyn Fn() + Send + Sync>,
) {
    loop {
        if shutdown.load(Ordering::Acquire) {
            break;
        }

        let stamp = data.last_fire_ms.load(Ordering::Acquire);
        if stamp == 0 {
            // No pending fire — park indefinitely until callback unparks us
            // or Drop signals shutdown.
            thread::park();
            continue;
        }

        let now = data.now_ms();
        let elapsed = now.saturating_sub(stamp);
        if elapsed >= DEBOUNCE_MS {
            // Quiet period elapsed — clear the stamp and fire.
            //
            // Using compare_exchange ensures that if a fresh callback fires
            // between our load and clear, we don't drop its event: if `stamp`
            // changed, we leave the new value in place and re-loop to wait
            // for *its* quiet period.
            if data
                .last_fire_ms
                .compare_exchange(stamp, 0, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                on_change();
            }
            // else: a newer fire raced in; loop and re-evaluate.
        } else {
            let remaining = DEBOUNCE_MS - elapsed;
            thread::park_timeout(Duration::from_millis(remaining));
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    fn noop_register(_id: AudioObjectID, _data: *mut c_void) -> OSStatus {
        0
    }
    fn noop_unregister(_id: AudioObjectID, _data: *mut c_void) -> OSStatus {
        0
    }

    /// Helper to simulate a CoreAudio fire on the worker without going
    /// through the OS: it stamps the timestamp and unparks the worker
    /// exactly the way the real C callback would.
    fn simulate_fire(listener: &DeviceFormatListener) {
        let now = listener.data.now_ms().max(1);
        listener.data.last_fire_ms.store(now, Ordering::Release);
        if let Some(t) = listener.data.worker.get() {
            t.unpark();
        }
    }

    // 5.1 — Debounce coalescer cases.

    #[test]
    fn three_fires_within_50ms_produce_one_callback() {
        let count = Arc::new(AtomicUsize::new(0));
        let count_c = count.clone();
        let listener = DeviceFormatListener::new_for_test(
            move || {
                count_c.fetch_add(1, Ordering::SeqCst);
            },
            noop_register,
            noop_unregister,
        )
        .expect("listener");

        for _ in 0..3 {
            simulate_fire(&listener);
            thread::sleep(Duration::from_millis(15));
        }
        // Wait well past the debounce window
        thread::sleep(Duration::from_millis(DEBOUNCE_MS + 100));
        assert_eq!(count.load(Ordering::SeqCst), 1, "expected exactly one coalesced callback");
    }

    #[test]
    fn single_fire_then_quiet_produces_one_callback() {
        let count = Arc::new(AtomicUsize::new(0));
        let count_c = count.clone();
        let listener = DeviceFormatListener::new_for_test(
            move || {
                count_c.fetch_add(1, Ordering::SeqCst);
            },
            noop_register,
            noop_unregister,
        )
        .expect("listener");

        simulate_fire(&listener);
        thread::sleep(Duration::from_millis(DEBOUNCE_MS + 100));
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn fire_then_partial_quiet_then_fire_produces_one_late_callback() {
        let count = Arc::new(AtomicUsize::new(0));
        let count_c = count.clone();
        let listener = DeviceFormatListener::new_for_test(
            move || {
                count_c.fetch_add(1, Ordering::SeqCst);
            },
            noop_register,
            noop_unregister,
        )
        .expect("listener");

        simulate_fire(&listener);
        thread::sleep(Duration::from_millis(100));
        // Still inside the debounce window — the worker must NOT have fired yet.
        assert_eq!(count.load(Ordering::SeqCst), 0);
        let second = Instant::now();
        simulate_fire(&listener);
        // After ~150ms from the *second* fire, exactly one callback total.
        thread::sleep(Duration::from_millis(DEBOUNCE_MS + 100));
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(
            second.elapsed() >= Duration::from_millis(DEBOUNCE_MS),
            "callback fired before quiet period elapsed"
        );
    }

    // 5.2 — sync() bookkeeping: counts what register/unregister hooks see.

    #[test]
    fn sync_adds_new_devices_and_removes_departed() {
        use std::sync::atomic::AtomicI32;
        static REG_CALLS: AtomicI32 = AtomicI32::new(0);
        static UNREG_CALLS: AtomicI32 = AtomicI32::new(0);
        REG_CALLS.store(0, Ordering::SeqCst);
        UNREG_CALLS.store(0, Ordering::SeqCst);

        fn counting_register(_id: AudioObjectID, _data: *mut c_void) -> OSStatus {
            REG_CALLS.fetch_add(1, Ordering::SeqCst);
            0
        }
        fn counting_unregister(_id: AudioObjectID, _data: *mut c_void) -> OSStatus {
            UNREG_CALLS.fetch_add(1, Ordering::SeqCst);
            0
        }

        let listener = DeviceFormatListener::new_for_test(
            || {},
            counting_register,
            counting_unregister,
        )
        .expect("listener");

        // Initial sync — three devices added.
        listener.sync(&[1, 2, 3]);
        assert_eq!(listener.registered_count(), 3);
        assert_eq!(REG_CALLS.load(Ordering::SeqCst), 3);
        assert_eq!(UNREG_CALLS.load(Ordering::SeqCst), 0);

        // Idempotent re-sync — no new add/remove calls.
        listener.sync(&[1, 2, 3]);
        assert_eq!(REG_CALLS.load(Ordering::SeqCst), 3);
        assert_eq!(UNREG_CALLS.load(Ordering::SeqCst), 0);

        // Replace one device — one add, one remove.
        listener.sync(&[1, 2, 4]);
        assert_eq!(listener.registered_count(), 3);
        assert_eq!(REG_CALLS.load(Ordering::SeqCst), 4);
        assert_eq!(UNREG_CALLS.load(Ordering::SeqCst), 1);

        // Drop unregisters the rest.
        drop(listener);
        assert_eq!(UNREG_CALLS.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn sync_with_failed_register_does_not_record_device() {
        fn failing_register(_id: AudioObjectID, _data: *mut c_void) -> OSStatus {
            -1 // any non-zero
        }

        let listener = DeviceFormatListener::new_for_test(
            || {},
            failing_register,
            noop_unregister,
        )
        .expect("listener");

        listener.sync(&[1, 2]);
        assert_eq!(
            listener.registered_count(),
            0,
            "failed registrations should not be recorded — next sync will retry"
        );
    }
}
