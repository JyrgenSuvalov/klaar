#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

mod constants;
mod device;
mod plugin;
pub mod ring_buffer;
mod stream;
pub mod timing;
#[cfg(test)]
mod tests;
mod types;

use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};

use coreaudio_sys::*;

use crate::types::*;

// ---------------------------------------------------------------------------
// Debug logging — writes to /tmp/klaar-driver.log
// Only compiled when the `dev-logging` cargo feature is enabled. Shipped
// builds leave it off: synchronous disk I/O on every HAL property callback
// slows coreaudiod enumeration to the point of tripping install_driver's
// poll deadline. Must be removed entirely from any build that exercises the
// IO path.
// ---------------------------------------------------------------------------

#[cfg(feature = "dev-logging")]
mod debug_log {
    use coreaudio_sys::{kAudioHardwareNoError, AudioObjectPropertyAddress, OSStatus};
    use std::io::Write;

    fn fourcc(v: u32) -> [u8; 4] {
        v.to_be_bytes()
    }

    pub fn log_msg(msg: &str) {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/klaar-driver.log")
        {
            let _ = writeln!(f, "{msg}");
        }
    }

    pub fn log_property(
        method: &str,
        object_id: u32,
        addr: &AudioObjectPropertyAddress,
        result: OSStatus,
    ) {
        let sel = fourcc(addr.mSelector);
        let scope = fourcc(addr.mScope);
        let sel_str = std::str::from_utf8(&sel).unwrap_or("????");
        let scope_str = std::str::from_utf8(&scope).unwrap_or("????");
        let status_str = if result == kAudioHardwareNoError as OSStatus {
            "OK".to_string()
        } else {
            let fc = fourcc(result as u32);
            format!(
                "ERR {} ('{}')",
                result,
                std::str::from_utf8(&fc).unwrap_or("????")
            )
        };
        log_msg(&format!(
            "{method}: obj={object_id} sel='{}' scope='{}' elem={} => {status_str}",
            sel_str, scope_str, addr.mElement
        ));
    }
}

// Logging macros are gated on the `dev-logging` cargo feature rather than
// `debug_assertions`. Shipped release builds leave the feature off, so the
// macros expand to nothing — no synchronous disk I/O on the CoreAudio HAL
// callback path. Turn them back on for debugging with:
//     cargo build --release --features dev-logging
// or via the `KLAAR_DRIVER_DEV_LOGGING=1` env var recognised by
// `scripts/build-driver.sh`.

#[cfg(feature = "dev-logging")]
macro_rules! driver_log {
    ($($arg:tt)*) => { debug_log::log_msg(&format!($($arg)*)) };
}

#[cfg(not(feature = "dev-logging"))]
macro_rules! driver_log {
    ($($arg:tt)*) => {};
}

#[cfg(feature = "dev-logging")]
macro_rules! driver_log_property {
    ($method:expr, $object_id:expr, $addr:expr, $result:expr) => {
        debug_log::log_property($method, $object_id, $addr, $result)
    };
}

#[cfg(not(feature = "dev-logging"))]
macro_rules! driver_log_property {
    ($method:expr, $object_id:expr, $addr:expr, $result:expr) => {};
}

// ---------------------------------------------------------------------------
// Driver state — single static instance, never deallocated
// ---------------------------------------------------------------------------

pub struct DriverState {
    /// Host interface pointer, set during Initialize
    pub host: AtomicPtr<AudioServerPlugInHostInterface>,

    /// COM reference count (formal — driver is static, never freed)
    pub ref_count: AtomicU32,

    /// Current sample rate (f64 bits stored as u64 for atomic access)
    pub sample_rate_bits: std::sync::atomic::AtomicU64,

    /// Current buffer frame size
    pub buffer_frame_size: AtomicU32,

    /// Whether IO is currently running
    pub io_is_running: AtomicBool,

    /// Number of IO clients
    pub io_client_count: AtomicU32,

    /// Virtual clock for GetZeroTimeStamp
    pub clock: UnsafeCell<Option<timing::VirtualClock>>,

    /// Ring buffer for audio loopback (output → input)
    ///
    /// # Safety
    ///
    /// Uses `UnsafeCell` because:
    /// - IO operations (`read`/`write`) access via atomic positions (lock-free, safe)
    /// - Resize only occurs during `PerformDeviceConfigurationChange`, which
    ///   CoreAudio guarantees does not overlap with IO
    /// - Initialization uses `Once` to ensure single init
    pub ring_buffer: UnsafeCell<Option<ring_buffer::RingBuffer>>,

    /// Ensures ring buffer and clock are initialized exactly once
    pub init_once: std::sync::Once,

    /// Pending sample rate for configuration change flow (f64 bits as u64)
    pub pending_sample_rate_bits: std::sync::atomic::AtomicU64,

    /// Pending buffer frame size for configuration change flow
    pub pending_buffer_frame_size: AtomicU32,
}

// SAFETY: DriverState is safe to share between threads because:
// - All scalar fields use atomic types with appropriate ordering
// - ring_buffer uses UnsafeCell but is only mutated during init (Once) and
//   PerformDeviceConfigurationChange (CoreAudio guarantees exclusive access)
// - clock uses UnsafeCell but follows the same init/config-change pattern
// - The SPSC ring buffer internals use atomics for read/write positions
unsafe impl Sync for DriverState {}

impl DriverState {
    const fn new() -> Self {
        Self {
            host: AtomicPtr::new(ptr::null_mut()),
            ref_count: AtomicU32::new(1),
            sample_rate_bits: std::sync::atomic::AtomicU64::new(
                constants::DEFAULT_SAMPLE_RATE.to_bits(),
            ),
            buffer_frame_size: AtomicU32::new(constants::DEFAULT_BUFFER_FRAME_SIZE),
            io_is_running: AtomicBool::new(false),
            io_client_count: AtomicU32::new(0),
            clock: UnsafeCell::new(None),
            ring_buffer: UnsafeCell::new(None),
            init_once: std::sync::Once::new(),
            pending_sample_rate_bits: std::sync::atomic::AtomicU64::new(0),
            pending_buffer_frame_size: AtomicU32::new(0),
        }
    }

    pub fn sample_rate(&self) -> f64 {
        f64::from_bits(self.sample_rate_bits.load(Ordering::Acquire))
    }

    /// Initialize the ring buffer and virtual clock. Called via `Once` during `driver_Initialize`.
    ///
    /// # Safety
    /// Must only be called once (enforced by `init_once`).
    unsafe fn init_audio(&self) {
        // Capacity is sized as a multiple of the zero-timestamp period rather
        // than the max buffer frame size (capacity = multiplier × period).
        // This keeps headroom proportional to CoreAudio's IO scheduling
        // cadence and avoids over-allocating for the worst-case 8192-frame
        // buffer that applications almost never request in practice.
        // Capacity is now expressed in *frames* (matching CoreAudio's
        // `mSampleTime` semantic and the positional RingBuffer API). Total
        // allocation is unchanged: `capacity_frames × CHANNEL_COUNT` f32s.
        let capacity_frames = constants::RING_BUFFER_CAPACITY_MULTIPLIER
            * constants::ZERO_TIMESTAMP_PERIOD as usize;
        let rb = ring_buffer::RingBuffer::new(capacity_frames);
        (*self.ring_buffer.get()) = Some(rb);

        let clock = timing::VirtualClock::new();
        (*self.clock.get()) = Some(clock);
    }

    /// Get a reference to the ring buffer.
    ///
    /// # Safety
    /// The ring buffer must have been initialized via `init_audio`. CoreAudio
    /// never calls IO entry points before `Initialize`, and `init_once` ensures
    /// this runs before any caller reaches `StartIO`/`DoIOOperation`. We use
    /// `unwrap_unchecked` to avoid emitting a `format!` panic path on the
    /// real-time IO thread (panicking inside `coreaudiod` would crash the
    /// audio subsystem for every app on the system).
    pub unsafe fn get_ring_buffer(&self) -> &ring_buffer::RingBuffer {
        (*self.ring_buffer.get()).as_ref().unwrap_unchecked()
    }

    /// Get a reference to the virtual clock.
    ///
    /// # Safety
    /// The clock must have been initialized via `init_audio`. See
    /// `get_ring_buffer` for the rationale behind `unwrap_unchecked`.
    pub unsafe fn get_clock(&self) -> &timing::VirtualClock {
        (*self.clock.get()).as_ref().unwrap_unchecked()
    }
}

static DRIVER_STATE: DriverState = DriverState::new();

/// Test-only helper: zero-fill the ring buffer so each test starts from a
/// known-clean state. Must only be called from `#[cfg(test)]` code and only
/// when no IO is active (always true for in-process unit tests, which never
/// call `driver_StartIO`).
///
/// Exposed as a narrow helper rather than making `DRIVER_STATE` or
/// `get_ring_buffer` `pub(crate)` because tests should not be able to poke at
/// the ring buffer directly.
#[cfg(test)]
pub(crate) fn test_reset_ring_buffer() {
    // SAFETY: `driver_Initialize` is called before any test touches the ring
    // buffer (see `tests::ensure_initialised`). `reset()` is safe because no
    // driver IO thread exists in a test process.
    unsafe {
        DRIVER_STATE.get_ring_buffer().reset();
    }
}

// ---------------------------------------------------------------------------
// Vtable — static, immutable, contains all 23 function pointers + _reserved
//
// Safety: The vtable is fully initialized at compile time and never mutated.
// The *mut c_void in _reserved is always null. coreaudiod loads one copy of
// this plugin per process. The Sync requirement on statics is satisfied by
// the fact that all access is read-only after initialization.
// ---------------------------------------------------------------------------

#[repr(C)]
struct DriverInterfaceWrapper {
    vtable: AudioServerPlugInDriverInterface,
}
unsafe impl Sync for DriverInterfaceWrapper {}
unsafe impl Send for DriverInterfaceWrapper {}

static DRIVER_VTABLE: DriverInterfaceWrapper = DriverInterfaceWrapper {
    vtable: AudioServerPlugInDriverInterface {
        _reserved: ptr::null_mut(),
        QueryInterface: Some(driver_QueryInterface),
        AddRef: Some(driver_AddRef),
        Release: Some(driver_Release),
        Initialize: Some(driver_Initialize),
        CreateDevice: Some(driver_CreateDevice),
        DestroyDevice: Some(driver_DestroyDevice),
        AddDeviceClient: Some(driver_AddDeviceClient),
        RemoveDeviceClient: Some(driver_RemoveDeviceClient),
        PerformDeviceConfigurationChange: Some(driver_PerformDeviceConfigurationChange),
        AbortDeviceConfigurationChange: Some(driver_AbortDeviceConfigurationChange),
        HasProperty: Some(driver_HasProperty),
        IsPropertySettable: Some(driver_IsPropertySettable),
        GetPropertyDataSize: Some(driver_GetPropertyDataSize),
        GetPropertyData: Some(driver_GetPropertyData),
        SetPropertyData: Some(driver_SetPropertyData),
        StartIO: Some(driver_StartIO),
        StopIO: Some(driver_StopIO),
        GetZeroTimeStamp: Some(driver_GetZeroTimeStamp),
        WillDoIOOperation: Some(driver_WillDoIOOperation),
        BeginIOOperation: Some(driver_BeginIOOperation),
        DoIOOperation: Some(driver_DoIOOperation),
        EndIOOperation: Some(driver_EndIOOperation),
    },
};

// Double pointer for COM: coreaudiod expects AudioServerPlugInDriverInterface**
// Since DriverInterfaceWrapper is #[repr(C)] and vtable is its first field,
// a pointer to DRIVER_VTABLE is also a pointer to the vtable field.
// We store a pointer-to-that-pointer in this static.
#[repr(C)]
struct VtablePointerWrapper {
    ptr: *const AudioServerPlugInDriverInterface,
}
unsafe impl Sync for VtablePointerWrapper {}
unsafe impl Send for VtablePointerWrapper {}

static DRIVER_VTABLE_PTR: VtablePointerWrapper = VtablePointerWrapper {
    ptr: &DRIVER_VTABLE.vtable as *const AudioServerPlugInDriverInterface,
};

// ---------------------------------------------------------------------------
// Factory function — the single exported entry point
// ---------------------------------------------------------------------------

/// Called by coreaudiod to create the driver instance.
/// Returns AudioServerPlugInDriverInterface** (COM-style double pointer).
///
/// # Safety
///
/// Must only be called by CoreAudio's plugin host (`coreaudiod`).
/// `requested_type_uuid` must be a valid `CFUUIDRef` or null.
#[no_mangle]
pub unsafe extern "C" fn KlaarDriver_Create(
    _allocator: CFAllocatorRef,
    requested_type_uuid: CFUUIDRef,
) -> *mut c_void {
    driver_log!("=== KlaarDriver_Create called ===");

    if requested_type_uuid.is_null() {
        driver_log!("  requested_type_uuid is null, returning null");
        return ptr::null_mut();
    }

    let requested_bytes = CFUUIDGetUUIDBytes(requested_type_uuid);

    if !uuid_bytes_equal(&requested_bytes, &KAUDIO_SERVER_PLUGIN_TYPE_UUID_BYTES) {
        driver_log!("  UUID mismatch, returning null");
        return ptr::null_mut();
    }

    driver_log!("  UUID matched, returning vtable pointer");
    // Return pointer to our pointer-to-vtable (double indirection required by COM)
    &DRIVER_VTABLE_PTR.ptr as *const *const AudioServerPlugInDriverInterface as *mut c_void
}

// ---------------------------------------------------------------------------
// COM IUnknown methods
// ---------------------------------------------------------------------------

unsafe extern "C" fn driver_QueryInterface(
    _driver: *mut c_void,
    in_uuid: REFIID, // REFIID = CFUUIDBytes, passed by value
    out_interface: *mut LPVOID,
) -> HRESULT {
    if out_interface.is_null() {
        return E_NOINTERFACE;
    }

    if uuid_bytes_equal(&in_uuid, &KAUDIO_SERVER_PLUGIN_DRIVER_INTERFACE_UUID_BYTES)
        || uuid_bytes_equal(&in_uuid, &IUNKNOWN_UUID_BYTES)
    {
        DRIVER_STATE.ref_count.fetch_add(1, Ordering::AcqRel);
        *out_interface =
            &DRIVER_VTABLE_PTR.ptr as *const *const AudioServerPlugInDriverInterface as LPVOID;
        return S_OK;
    }

    *out_interface = ptr::null_mut();
    E_NOINTERFACE
}

unsafe extern "C" fn driver_AddRef(_driver: *mut c_void) -> u32 {
    DRIVER_STATE.ref_count.fetch_add(1, Ordering::AcqRel) + 1
}

unsafe extern "C" fn driver_Release(_driver: *mut c_void) -> u32 {
    // Static driver — never deallocated. Clamp at 1 to prevent underflow.
    let prev = DRIVER_STATE.ref_count.fetch_sub(1, Ordering::AcqRel);
    if prev <= 1 {
        // Restore to 1 — static driver must stay alive
        DRIVER_STATE.ref_count.store(1, Ordering::Release);
        return 0;
    }
    prev - 1
}

// ---------------------------------------------------------------------------
// Initialize
// ---------------------------------------------------------------------------

unsafe extern "C" fn driver_Initialize(
    _driver: AudioServerPlugInDriverRef,
    host: AudioServerPlugInHostRef,
) -> OSStatus {
    driver_log!("=== driver_Initialize called ===");
    DRIVER_STATE
        .host
        .store(host as *mut AudioServerPlugInHostInterface, Ordering::Release);

    // Initialize ring buffer and virtual clock exactly once
    DRIVER_STATE.init_once.call_once(|| {
        // SAFETY: Called exactly once via Once. No concurrent access possible.
        unsafe { DRIVER_STATE.init_audio() };
    });

    driver_log!("  host stored, audio initialized, returning NoError");
    kAudioHardwareNoError as OSStatus
}

// ---------------------------------------------------------------------------
// Device lifecycle — not supported, we have a fixed device
// ---------------------------------------------------------------------------

unsafe extern "C" fn driver_CreateDevice(
    _driver: AudioServerPlugInDriverRef,
    _description: CFDictionaryRef,
    _client_info: *const AudioServerPlugInClientInfo,
    _out_device_id: *mut AudioObjectID,
) -> OSStatus {
    kAudioHardwareUnsupportedOperationError as OSStatus
}

unsafe extern "C" fn driver_DestroyDevice(
    _driver: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
) -> OSStatus {
    kAudioHardwareUnsupportedOperationError as OSStatus
}

// ---------------------------------------------------------------------------
// Client management — no-op (the driver does not track per-client state)
// ---------------------------------------------------------------------------

unsafe extern "C" fn driver_AddDeviceClient(
    _driver: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _client_info: *const AudioServerPlugInClientInfo,
) -> OSStatus {
    kAudioHardwareNoError as OSStatus
}

unsafe extern "C" fn driver_RemoveDeviceClient(
    _driver: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _client_info: *const AudioServerPlugInClientInfo,
) -> OSStatus {
    kAudioHardwareNoError as OSStatus
}

// ---------------------------------------------------------------------------
// Configuration change — no-op (format is fixed at 48 kHz / stereo)
// ---------------------------------------------------------------------------

unsafe extern "C" fn driver_PerformDeviceConfigurationChange(
    _driver: AudioServerPlugInDriverRef,
    device_id: AudioObjectID,
    change_action: u64,
    _change_info: *mut c_void,
) -> OSStatus {
    driver_log!("PerformDeviceConfigurationChange: action={change_action}");

    match change_action {
        constants::ACTION_SET_SAMPLE_RATE => {
            let new_rate_bits = DRIVER_STATE
                .pending_sample_rate_bits
                .load(Ordering::Acquire);
            let new_rate = f64::from_bits(new_rate_bits);

            // Apply the sample rate change
            DRIVER_STATE
                .sample_rate_bits
                .store(new_rate_bits, Ordering::Release);

            // Reset clock (re-anchor + increment seed) since sample rate changed
            let clock = DRIVER_STATE.get_clock();
            clock.reset(new_rate);

            // Resize ring buffer for new sample rate
            // Capacity doesn't depend on sample rate (it's based on max buffer size),
            // but we reset it to clear any stale data
            let rb = DRIVER_STATE.get_ring_buffer();
            rb.reset();

            driver_log!("  Sample rate changed to {new_rate}");

            // Notify host that properties changed
            notify_properties_changed(device_id, &[
                kAudioDevicePropertyNominalSampleRate,
            ]);
        }

        constants::ACTION_SET_BUFFER_SIZE => {
            let new_size = DRIVER_STATE
                .pending_buffer_frame_size
                .load(Ordering::Acquire);

            DRIVER_STATE
                .buffer_frame_size
                .store(new_size, Ordering::Release);

            driver_log!("  Buffer frame size changed to {new_size}");

            // Notify host that properties changed
            notify_properties_changed(device_id, &[
                kAudioDevicePropertyBufferFrameSize,
            ]);
        }

        _ => {
            driver_log!("  Unknown action {change_action}");
        }
    }

    kAudioHardwareNoError as OSStatus
}

/// Notify the host that device properties have changed.
///
/// # Safety
/// Must only be called when the host pointer is valid.
unsafe fn notify_properties_changed(device_id: AudioObjectID, selectors: &[u32]) {
    let host = DRIVER_STATE.host.load(Ordering::Acquire);
    if host.is_null() {
        return;
    }

    let host_ref = &*host;
    if let Some(notify_fn) = host_ref.PropertiesChanged {
        // Config changes touch at most a handful of selectors — use a fixed
        // stack array and avoid the heap allocation entirely. Selectors beyond
        // the array's capacity are silently dropped (none of our call sites
        // send more than one selector today).
        const MAX_SELECTORS: usize = 4;
        let mut addresses: [AudioObjectPropertyAddress; MAX_SELECTORS] =
            [AudioObjectPropertyAddress {
                mSelector: 0,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain,
            }; MAX_SELECTORS];
        let count = selectors.len().min(MAX_SELECTORS);
        for (slot, &sel) in addresses.iter_mut().zip(selectors.iter()).take(count) {
            slot.mSelector = sel;
        }

        notify_fn(host, device_id, count as u32, addresses.as_ptr());
    }
}

unsafe extern "C" fn driver_AbortDeviceConfigurationChange(
    _driver: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    change_action: u64,
    _change_info: *mut c_void,
) -> OSStatus {
    driver_log!("AbortDeviceConfigurationChange: action={change_action}");

    // Clear the stale pending value for the aborted action so a subsequent
    // SetPropertyData → Request cycle starts from a clean slate. Not strictly
    // necessary (every SetPropertyData writes a fresh pending value before
    // requesting the change) but leaves the state predictable.
    match change_action {
        constants::ACTION_SET_SAMPLE_RATE => {
            DRIVER_STATE
                .pending_sample_rate_bits
                .store(0, Ordering::Release);
        }
        constants::ACTION_SET_BUFFER_SIZE => {
            DRIVER_STATE
                .pending_buffer_frame_size
                .store(0, Ordering::Release);
        }
        _ => {}
    }

    kAudioHardwareNoError as OSStatus
}

// ---------------------------------------------------------------------------
// Property system — dispatches to plugin.rs, device.rs, stream.rs
// ---------------------------------------------------------------------------

unsafe extern "C" fn driver_HasProperty(
    _driver: AudioServerPlugInDriverRef,
    object_id: AudioObjectID,
    _client_pid: pid_t,
    address: *const AudioObjectPropertyAddress,
) -> Boolean {
    if address.is_null() {
        return 0;
    }
    let addr = &*address;

    let result = match object_id {
        constants::PLUGIN_OBJECT_ID => plugin::has_property(addr),
        constants::DEVICE_OBJECT_ID => device::has_property(addr),
        constants::INPUT_STREAM_OBJECT_ID | constants::OUTPUT_STREAM_OBJECT_ID => {
            stream::has_property(object_id, addr)
        }
        _ => 0,
    };

    driver_log!("HasProperty: obj={object_id} sel={:#010x} => {result}", addr.mSelector);

    result
}

unsafe extern "C" fn driver_IsPropertySettable(
    _driver: AudioServerPlugInDriverRef,
    object_id: AudioObjectID,
    _client_pid: pid_t,
    address: *const AudioObjectPropertyAddress,
    out_is_settable: *mut Boolean,
) -> OSStatus {
    if address.is_null() || out_is_settable.is_null() {
        return kAudioHardwareBadObjectError as OSStatus;
    }
    let addr = &*address;

    match object_id {
        constants::PLUGIN_OBJECT_ID => {
            *out_is_settable = plugin::is_property_settable(addr);
            kAudioHardwareNoError as OSStatus
        }
        constants::DEVICE_OBJECT_ID => {
            *out_is_settable = device::is_property_settable(addr);
            kAudioHardwareNoError as OSStatus
        }
        constants::INPUT_STREAM_OBJECT_ID | constants::OUTPUT_STREAM_OBJECT_ID => {
            *out_is_settable = stream::is_property_settable(object_id, addr);
            kAudioHardwareNoError as OSStatus
        }
        _ => kAudioHardwareBadObjectError as OSStatus,
    }
}

unsafe extern "C" fn driver_GetPropertyDataSize(
    _driver: AudioServerPlugInDriverRef,
    object_id: AudioObjectID,
    _client_pid: pid_t,
    address: *const AudioObjectPropertyAddress,
    _qualifier_data_size: u32,
    _qualifier_data: *const c_void,
    out_data_size: *mut u32,
) -> OSStatus {
    if address.is_null() || out_data_size.is_null() {
        return kAudioHardwareBadObjectError as OSStatus;
    }
    let addr = &*address;

    let result = match object_id {
        constants::PLUGIN_OBJECT_ID => plugin::get_property_data_size(addr, out_data_size),
        constants::DEVICE_OBJECT_ID => device::get_property_data_size(addr, out_data_size),
        constants::INPUT_STREAM_OBJECT_ID | constants::OUTPUT_STREAM_OBJECT_ID => {
            stream::get_property_data_size(object_id, addr, out_data_size)
        }
        _ => kAudioHardwareBadObjectError as OSStatus,
    };

    driver_log_property!("GetPropertyDataSize", object_id, addr, result);
    result
}

unsafe extern "C" fn driver_GetPropertyData(
    _driver: AudioServerPlugInDriverRef,
    object_id: AudioObjectID,
    _client_pid: pid_t,
    address: *const AudioObjectPropertyAddress,
    _qualifier_data_size: u32,
    _qualifier_data: *const c_void,
    in_data_size: u32,
    out_data_size: *mut u32,
    out_data: *mut c_void,
) -> OSStatus {
    if address.is_null() || out_data_size.is_null() || out_data.is_null() {
        return kAudioHardwareBadObjectError as OSStatus;
    }
    let addr = &*address;

    let result = match object_id {
        constants::PLUGIN_OBJECT_ID => {
            plugin::get_property_data(addr, in_data_size, out_data_size, out_data)
        }
        constants::DEVICE_OBJECT_ID => {
            device::get_property_data(addr, in_data_size, out_data_size, out_data)
        }
        constants::INPUT_STREAM_OBJECT_ID | constants::OUTPUT_STREAM_OBJECT_ID => {
            stream::get_property_data(object_id, addr, in_data_size, out_data_size, out_data)
        }
        _ => kAudioHardwareBadObjectError as OSStatus,
    };

    driver_log_property!("GetPropertyData", object_id, addr, result);
    result
}

unsafe extern "C" fn driver_SetPropertyData(
    _driver: AudioServerPlugInDriverRef,
    object_id: AudioObjectID,
    _client_pid: pid_t,
    address: *const AudioObjectPropertyAddress,
    _qualifier_data_size: u32,
    _qualifier_data: *const c_void,
    in_data_size: u32,
    in_data: *const c_void,
) -> OSStatus {
    if address.is_null() || in_data.is_null() {
        return kAudioHardwareBadObjectError as OSStatus;
    }
    let addr = &*address;

    match object_id {
        constants::DEVICE_OBJECT_ID => {
            device::set_property_data(addr, in_data_size, in_data)
        }
        constants::PLUGIN_OBJECT_ID
        | constants::INPUT_STREAM_OBJECT_ID
        | constants::OUTPUT_STREAM_OBJECT_ID => {
            // No settable properties on these objects
            kAudioHardwareUnknownPropertyError as OSStatus
        }
        _ => kAudioHardwareBadObjectError as OSStatus,
    }
}

// ---------------------------------------------------------------------------
// IO operations
// ---------------------------------------------------------------------------

unsafe extern "C" fn driver_StartIO(
    _driver: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _client_id: u32,
) -> OSStatus {
    driver_log!("StartIO called, client_id={_client_id}");

    // Atomically increment io_client_count. Use compare_exchange loop to
    // detect the 0→1 transition (first client) where we need to anchor
    // the clock and reset the ring buffer.
    loop {
        let current = DRIVER_STATE.io_client_count.load(Ordering::Acquire);
        let new = current + 1;

        match DRIVER_STATE.io_client_count.compare_exchange(
            current,
            new,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(prev) => {
                if prev == 0 {
                    // First client: anchor clock and reset ring buffer
                    let clock = DRIVER_STATE.get_clock();
                    clock.anchor(timing::VirtualClock::now(), DRIVER_STATE.sample_rate());

                    let rb = DRIVER_STATE.get_ring_buffer();
                    rb.reset();

                    DRIVER_STATE.io_is_running.store(true, Ordering::Release);
                    driver_log!("  First client, clock anchored, ring buffer reset");
                } else {
                    driver_log!("  Client count: {prev} -> {new}");
                }
                break;
            }
            Err(_) => {
                // CAS failed — another thread modified the count, retry
                continue;
            }
        }
    }

    kAudioHardwareNoError as OSStatus
}

unsafe extern "C" fn driver_StopIO(
    _driver: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _client_id: u32,
) -> OSStatus {
    driver_log!("StopIO called, client_id={_client_id}");

    // CAS loop: atomically decrement while guarding against underflow. Using
    // `fetch_sub` would briefly expose `u32::MAX` if the count is already 0,
    // which a racing StartIO could wrap back to 0 and spuriously re-anchor.
    loop {
        let current = DRIVER_STATE.io_client_count.load(Ordering::Acquire);
        if current == 0 {
            driver_log!("  WARNING: StopIO called with io_client_count=0");
            break;
        }
        match DRIVER_STATE.io_client_count.compare_exchange(
            current,
            current - 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(prev) => {
                if prev == 1 {
                    DRIVER_STATE.io_is_running.store(false, Ordering::Release);
                    driver_log!("  Last client stopped, IO stopped");
                } else {
                    driver_log!("  Client count: {prev} -> {}", prev - 1);
                }
                break;
            }
            Err(_) => continue,
        }
    }

    kAudioHardwareNoError as OSStatus
}

unsafe extern "C" fn driver_GetZeroTimeStamp(
    _driver: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _client_id: u32,
    out_sample_time: *mut f64,
    out_host_time: *mut u64,
    out_seed: *mut u64,
) -> OSStatus {
    // Real-time safe: only atomic loads, mach_absolute_time(), and arithmetic
    let clock = DRIVER_STATE.get_clock();
    let period = constants::ZERO_TIMESTAMP_PERIOD;

    let (sample_time, host_time, seed) = clock.get_zero_timestamp(period);

    if !out_sample_time.is_null() {
        *out_sample_time = sample_time;
    }
    if !out_host_time.is_null() {
        *out_host_time = host_time;
    }
    if !out_seed.is_null() {
        *out_seed = seed;
    }

    kAudioHardwareNoError as OSStatus
}

unsafe extern "C" fn driver_WillDoIOOperation(
    _driver: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _client_id: u32,
    operation_id: u32,
    out_will_do: *mut Boolean,
    out_will_do_in_place: *mut Boolean,
) -> OSStatus {
    if !out_will_do.is_null() && !out_will_do_in_place.is_null() {
        let will_do = matches!(
            operation_id,
            kAudioServerPlugInIOOperationReadInput | kAudioServerPlugInIOOperationWriteMix
        );
        *out_will_do = if will_do { 1 } else { 0 };
        *out_will_do_in_place = 1;
    }
    kAudioHardwareNoError as OSStatus
}

unsafe extern "C" fn driver_BeginIOOperation(
    _driver: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _client_id: u32,
    _operation_id: u32,
    _io_buffer_frame_size: u32,
    _io_cycle_info: *const AudioServerPlugInIOCycleInfo,
) -> OSStatus {
    kAudioHardwareNoError as OSStatus
}

unsafe extern "C" fn driver_DoIOOperation(
    _driver: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    stream_id: AudioObjectID,
    _client_id: u32,
    operation_id: u32,
    io_buffer_frame_size: u32,
    io_cycle_info: *const AudioServerPlugInIOCycleInfo,
    io_main_buffer: *mut c_void,
    _io_secondary_buffer: *mut c_void,
) -> OSStatus {
    // Real-time safe: no allocations, no locks, no syscalls.
    // Positional ring-buffer access keyed by `mSampleTime` from the cycle info.
    // Writer and reader agree on absolute frame positions via CoreAudio's
    // safety-offset contract, so there is no FIFO depth to accumulate and
    // latency converges to ~0.

    // Defensive null check — the SDK shouldn't pass null here, but we treat it
    // as success-no-op rather than deref. Also covers any operation ID we
    // don't handle (see match below).
    if io_cycle_info.is_null() {
        return kAudioHardwareNoError as OSStatus;
    }

    let sample_count = io_buffer_frame_size as usize * constants::CHANNEL_COUNT as usize;
    let rb = DRIVER_STATE.get_ring_buffer();

    match (stream_id, operation_id) {
        // WriteMix on output stream (ID 4): app writing audio to device.
        (constants::OUTPUT_STREAM_OBJECT_ID, kAudioServerPlugInIOOperationWriteMix) => {
            // `mSampleTime` is `Float64` in the SDK but is always integer-valued
            // for fixed-rate PCM devices like ours. `as u64` is saturating and
            // panic-free (negative values would saturate to 0, but CoreAudio
            // never produces negative sample times for this device).
            let sample_time = (*io_cycle_info).mOutputTime.mSampleTime as u64;
            // SAFETY: io_main_buffer points to sample_count interleaved Float32
            // samples, provided by CoreAudio. Valid for the duration of this call.
            let buffer = std::slice::from_raw_parts(io_main_buffer as *const f32, sample_count);
            rb.write_at(sample_time, buffer);
        }

        // ReadInput on input stream (ID 3): app reading audio from device.
        (constants::INPUT_STREAM_OBJECT_ID, kAudioServerPlugInIOOperationReadInput) => {
            let sample_time = (*io_cycle_info).mInputTime.mSampleTime as u64;
            // SAFETY: io_main_buffer points to sample_count Float32 samples we must fill.
            let buffer = std::slice::from_raw_parts_mut(io_main_buffer as *mut f32, sample_count);
            rb.read_at(sample_time, buffer);
        }

        _ => {
            // Other operations (e.g., on wrong stream) — no-op
        }
    }

    kAudioHardwareNoError as OSStatus
}

unsafe extern "C" fn driver_EndIOOperation(
    _driver: AudioServerPlugInDriverRef,
    _device_id: AudioObjectID,
    _client_id: u32,
    _operation_id: u32,
    _io_buffer_frame_size: u32,
    _io_cycle_info: *const AudioServerPlugInIOCycleInfo,
) -> OSStatus {
    kAudioHardwareNoError as OSStatus
}
