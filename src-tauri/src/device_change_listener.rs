/// System-wide CoreAudio device change listener.
///
/// Registers a property listener on `kAudioHardwarePropertyDevices` to detect
/// when any audio device is added or removed from the system. This is separate
/// from `DeviceMonitor` which tracks the alive status of specific devices.
///
/// The listener fires a callback whenever the device list changes, which
/// the app uses to re-enumerate devices and check if selected devices are
/// still available.
use coreaudio::sys::{
    kAudioHardwarePropertyDevices,
    kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal,
    kAudioObjectSystemObject,
    AudioObjectAddPropertyListener,
    AudioObjectID,
    AudioObjectPropertyAddress,
    AudioObjectRemovePropertyListener,
    OSStatus,
};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

/// Holds the registered CoreAudio property listener for device changes.
///
/// On drop, the listener is unregistered from CoreAudio.
pub struct DeviceChangeListener {
    /// Prevent double-unregister
    registered: AtomicBool,
    /// Pointer to the boxed closure stored on the heap. We need this to
    /// unregister the exact same pointer that was registered.
    callback_ptr: *mut c_void,
}

// The callback_ptr is a heap-allocated closure that we only access
// from the CoreAudio property listener thread and during drop.
unsafe impl Send for DeviceChangeListener {}
unsafe impl Sync for DeviceChangeListener {}

/// The C-compatible callback that CoreAudio calls when devices change.
/// `in_client_data` is a raw pointer to our boxed closure.
extern "C" fn property_listener_callback(
    _in_object_id: AudioObjectID,
    _in_number_addresses: u32,
    _in_addresses: *const AudioObjectPropertyAddress,
    in_client_data: *mut c_void,
) -> OSStatus {
    if !in_client_data.is_null() {
        let callback = unsafe { &*(in_client_data as *const Box<dyn Fn() + Send + Sync>) };
        callback();
    }
    0 // noErr
}

impl DeviceChangeListener {
    /// Register a listener on `kAudioHardwarePropertyDevices`.
    ///
    /// The `on_change` callback is invoked (on a CoreAudio internal thread)
    /// whenever a device is added or removed from the system.
    ///
    /// Returns `Err` if CoreAudio refuses the registration.
    pub fn new<F>(on_change: F) -> Result<Self, String>
    where
        F: Fn() + Send + Sync + 'static,
    {
        let addr = AudioObjectPropertyAddress {
            mSelector: kAudioHardwarePropertyDevices,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };

        // Box the closure and leak it into a raw pointer for C interop.
        // We'll reclaim it on drop.
        let boxed: Box<Box<dyn Fn() + Send + Sync>> = Box::new(Box::new(on_change));
        let raw_ptr = Box::into_raw(boxed) as *mut c_void;

        let status = unsafe {
            AudioObjectAddPropertyListener(
                kAudioObjectSystemObject,
                &addr,
                Some(property_listener_callback),
                raw_ptr,
            )
        };

        if status != 0 {
            // Reclaim the leaked memory on failure
            unsafe {
                let _ = Box::from_raw(raw_ptr as *mut Box<dyn Fn() + Send + Sync>);
            }
            return Err(format!(
                "AudioObjectAddPropertyListener failed: {:#010x}",
                status as u32
            ));
        }

        log::info!("Registered CoreAudio device change listener");

        Ok(Self {
            registered: AtomicBool::new(true),
            callback_ptr: raw_ptr,
        })
    }
}

impl Drop for DeviceChangeListener {
    fn drop(&mut self) {
        if self.registered.swap(false, Ordering::SeqCst) {
            let addr = AudioObjectPropertyAddress {
                mSelector: kAudioHardwarePropertyDevices,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain,
            };

            let status = unsafe {
                AudioObjectRemovePropertyListener(
                    kAudioObjectSystemObject,
                    &addr,
                    Some(property_listener_callback),
                    self.callback_ptr,
                )
            };

            if status != 0 {
                log::warn!(
                    "AudioObjectRemovePropertyListener failed: {:#010x}",
                    status as u32
                );
            } else {
                log::info!("Unregistered CoreAudio device change listener");
            }

            // Reclaim the leaked closure memory
            unsafe {
                let _ = Box::from_raw(self.callback_ptr as *mut Box<dyn Fn() + Send + Sync>);
            }
        }
    }
}
