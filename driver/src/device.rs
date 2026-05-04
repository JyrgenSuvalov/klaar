//! Property handling for the Device object (Object ID 2).

use std::ffi::c_void;
use std::mem;

use coreaudio_sys::*;

use crate::constants;
use crate::types::cfstring_create;
use crate::DRIVER_STATE;

// ---------------------------------------------------------------------------
// HasProperty
// ---------------------------------------------------------------------------

pub fn has_property(addr: &AudioObjectPropertyAddress) -> Boolean {
    match addr.mSelector {
        kAudioObjectPropertyBaseClass
        | kAudioObjectPropertyClass
        | kAudioObjectPropertyOwner
        | kAudioObjectPropertyName
        | kAudioObjectPropertyManufacturer
        | kAudioObjectPropertyOwnedObjects
        | kAudioDevicePropertyDeviceUID
        | kAudioDevicePropertyModelUID
        | kAudioDevicePropertyTransportType
        | kAudioDevicePropertyDeviceIsAlive
        | kAudioDevicePropertyDeviceIsRunning
        | kAudioDevicePropertyDeviceCanBeDefaultDevice
        | kAudioDevicePropertyDeviceCanBeDefaultSystemDevice
        | kAudioDevicePropertyLatency
        | kAudioDevicePropertySafetyOffset
        | kAudioDevicePropertyStreams
        | kAudioDevicePropertyNominalSampleRate
        | kAudioDevicePropertyAvailableNominalSampleRates
        | kAudioDevicePropertyClockDomain
        | kAudioDevicePropertyBufferFrameSize
        | kAudioDevicePropertyBufferFrameSizeRange
        | kAudioDevicePropertyZeroTimeStampPeriod
        | kAudioDevicePropertyClockAlgorithm
        | kAudioDevicePropertyClockIsStable
        | kAudioDevicePropertyConfigurationApplication
        | kAudioDevicePropertyRelatedDevices
        | kAudioObjectPropertyControlList
        | kAudioDevicePropertyIsHidden => 1,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// IsPropertySettable
// ---------------------------------------------------------------------------

pub fn is_property_settable(addr: &AudioObjectPropertyAddress) -> Boolean {
    match addr.mSelector {
        kAudioDevicePropertyNominalSampleRate | kAudioDevicePropertyBufferFrameSize => 1,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// GetPropertyDataSize
// ---------------------------------------------------------------------------

pub unsafe fn get_property_data_size(
    addr: &AudioObjectPropertyAddress,
    out_data_size: *mut u32,
) -> OSStatus {
    let size: u32 = match addr.mSelector {
        kAudioObjectPropertyBaseClass => mem::size_of::<AudioClassID>() as u32,
        kAudioObjectPropertyClass => mem::size_of::<AudioClassID>() as u32,
        kAudioObjectPropertyOwner => mem::size_of::<AudioObjectID>() as u32,
        kAudioObjectPropertyName => mem::size_of::<CFStringRef>() as u32,
        kAudioObjectPropertyManufacturer => mem::size_of::<CFStringRef>() as u32,
        kAudioObjectPropertyOwnedObjects => {
            // Two streams: input (3) and output (4)
            (2 * mem::size_of::<AudioObjectID>()) as u32
        }
        kAudioDevicePropertyDeviceUID => mem::size_of::<CFStringRef>() as u32,
        kAudioDevicePropertyModelUID => mem::size_of::<CFStringRef>() as u32,
        kAudioDevicePropertyTransportType => mem::size_of::<u32>() as u32,
        kAudioDevicePropertyDeviceIsAlive => mem::size_of::<u32>() as u32,
        kAudioDevicePropertyDeviceIsRunning => mem::size_of::<u32>() as u32,
        kAudioDevicePropertyDeviceCanBeDefaultDevice => mem::size_of::<u32>() as u32,
        kAudioDevicePropertyDeviceCanBeDefaultSystemDevice => mem::size_of::<u32>() as u32,
        kAudioDevicePropertyLatency => mem::size_of::<u32>() as u32,
        kAudioDevicePropertySafetyOffset => mem::size_of::<u32>() as u32,
        kAudioDevicePropertyStreams => {
            // Scope-dependent: input scope returns [3], output scope returns [4]
            mem::size_of::<AudioObjectID>() as u32
        }
        kAudioDevicePropertyNominalSampleRate => mem::size_of::<f64>() as u32,
        kAudioDevicePropertyAvailableNominalSampleRates => {
            (constants::SUPPORTED_SAMPLE_RATES.len() * mem::size_of::<AudioValueRange>()) as u32
        }
        kAudioDevicePropertyClockDomain => mem::size_of::<u32>() as u32,
        kAudioDevicePropertyBufferFrameSize => mem::size_of::<u32>() as u32,
        kAudioDevicePropertyBufferFrameSizeRange => mem::size_of::<AudioValueRange>() as u32,
        kAudioDevicePropertyZeroTimeStampPeriod => mem::size_of::<u32>() as u32,
        kAudioDevicePropertyClockAlgorithm => mem::size_of::<u32>() as u32,
        kAudioDevicePropertyClockIsStable => mem::size_of::<u32>() as u32,
        kAudioDevicePropertyConfigurationApplication => mem::size_of::<CFStringRef>() as u32,
        kAudioDevicePropertyRelatedDevices => mem::size_of::<AudioObjectID>() as u32,
        kAudioObjectPropertyControlList => 0, // No controls — empty array
        kAudioDevicePropertyIsHidden => mem::size_of::<u32>() as u32,
        _ => return kAudioHardwareUnknownPropertyError as OSStatus,
    };

    *out_data_size = size;
    kAudioHardwareNoError as OSStatus
}

// ---------------------------------------------------------------------------
// GetPropertyData
// ---------------------------------------------------------------------------

pub unsafe fn get_property_data(
    addr: &AudioObjectPropertyAddress,
    in_data_size: u32,
    out_data_size: *mut u32,
    out_data: *mut c_void,
) -> OSStatus {
    match addr.mSelector {
        kAudioObjectPropertyBaseClass => {
            let needed = mem::size_of::<AudioClassID>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut AudioClassID) = kAudioObjectClassID;
            *out_data_size = needed;
        }

        kAudioObjectPropertyClass => {
            let needed = mem::size_of::<AudioClassID>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut AudioClassID) = kAudioDeviceClassID;
            *out_data_size = needed;
        }

        kAudioObjectPropertyOwner => {
            let needed = mem::size_of::<AudioObjectID>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut AudioObjectID) = constants::PLUGIN_OBJECT_ID;
            *out_data_size = needed;
        }

        kAudioObjectPropertyName => {
            let needed = mem::size_of::<CFStringRef>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut CFStringRef) = cfstring_create(constants::DEVICE_NAME);
            *out_data_size = needed;
        }

        kAudioObjectPropertyManufacturer => {
            let needed = mem::size_of::<CFStringRef>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut CFStringRef) = cfstring_create(constants::MANUFACTURER);
            *out_data_size = needed;
        }

        kAudioObjectPropertyOwnedObjects => {
            let needed = (2 * mem::size_of::<AudioObjectID>()) as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            let ids = out_data as *mut AudioObjectID;
            *ids = constants::INPUT_STREAM_OBJECT_ID;
            *ids.add(1) = constants::OUTPUT_STREAM_OBJECT_ID;
            *out_data_size = needed;
        }

        kAudioDevicePropertyDeviceUID => {
            let needed = mem::size_of::<CFStringRef>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut CFStringRef) = cfstring_create(constants::DEVICE_UID);
            *out_data_size = needed;
        }

        kAudioDevicePropertyModelUID => {
            let needed = mem::size_of::<CFStringRef>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut CFStringRef) = cfstring_create(constants::MODEL_UID);
            *out_data_size = needed;
        }

        kAudioDevicePropertyTransportType => {
            let needed = mem::size_of::<u32>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut u32) = kAudioDeviceTransportTypeVirtual;
            *out_data_size = needed;
        }

        kAudioDevicePropertyDeviceIsAlive => {
            let needed = mem::size_of::<u32>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut u32) = 1; // Always alive
            *out_data_size = needed;
        }

        kAudioDevicePropertyDeviceIsRunning => {
            let needed = mem::size_of::<u32>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            let running = DRIVER_STATE
                .io_is_running
                .load(std::sync::atomic::Ordering::Acquire);
            *(out_data as *mut u32) = if running { 1 } else { 0 };
            *out_data_size = needed;
        }

        // Scope-aware per add-driver-scope-aware-default-capability: output scope MUST
        // remain 0 for both selectors — prevents macOS from auto-promoting the virtual
        // mic to default output.
        kAudioDevicePropertyDeviceCanBeDefaultDevice => {
            let needed = mem::size_of::<u32>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            // Only input scope is eligible to be the default device (i.e. default mic).
            // Output and global scopes must return 0 so macOS cannot auto-promote this
            // virtual device to default output.
            let value: u32 = if addr.mScope == kAudioObjectPropertyScopeInput {
                1
            } else {
                0
            };
            *(out_data as *mut u32) = value;
            *out_data_size = needed;
        }

        kAudioDevicePropertyDeviceCanBeDefaultSystemDevice => {
            let needed = mem::size_of::<u32>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            // Never eligible to be the system default device on any scope — we don't
            // want system sounds (alerts, UI chirps) routed through the virtual mic.
            *(out_data as *mut u32) = 0;
            *out_data_size = needed;
        }

        kAudioDevicePropertyLatency => {
            let needed = mem::size_of::<u32>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut u32) = 0; // Zero latency
            *out_data_size = needed;
        }

        kAudioDevicePropertySafetyOffset => {
            let needed = mem::size_of::<u32>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut u32) = 0; // No safety offset
            *out_data_size = needed;
        }

        kAudioDevicePropertyStreams => {
            let needed = mem::size_of::<AudioObjectID>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            // Scope-dependent: input returns stream 3, output returns stream 4
            let stream_id = if addr.mScope == kAudioObjectPropertyScopeInput {
                constants::INPUT_STREAM_OBJECT_ID
            } else {
                constants::OUTPUT_STREAM_OBJECT_ID
            };
            *(out_data as *mut AudioObjectID) = stream_id;
            *out_data_size = needed;
        }

        kAudioDevicePropertyNominalSampleRate => {
            let needed = mem::size_of::<f64>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut f64) = DRIVER_STATE.sample_rate();
            *out_data_size = needed;
        }

        kAudioDevicePropertyAvailableNominalSampleRates => {
            let count = constants::SUPPORTED_SAMPLE_RATES.len();
            let needed = (count * mem::size_of::<AudioValueRange>()) as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            let ranges = out_data as *mut AudioValueRange;
            for (i, &rate) in constants::SUPPORTED_SAMPLE_RATES.iter().enumerate() {
                *ranges.add(i) = AudioValueRange {
                    mMinimum: rate,
                    mMaximum: rate,
                };
            }
            *out_data_size = needed;
        }

        kAudioDevicePropertyClockDomain => {
            let needed = mem::size_of::<u32>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut u32) = 0;
            *out_data_size = needed;
        }

        kAudioDevicePropertyBufferFrameSize => {
            let needed = mem::size_of::<u32>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut u32) = DRIVER_STATE
                .buffer_frame_size
                .load(std::sync::atomic::Ordering::Acquire);
            *out_data_size = needed;
        }

        kAudioDevicePropertyBufferFrameSizeRange => {
            let needed = mem::size_of::<AudioValueRange>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut AudioValueRange) = AudioValueRange {
                mMinimum: constants::MIN_BUFFER_FRAME_SIZE as f64,
                mMaximum: constants::MAX_BUFFER_FRAME_SIZE as f64,
            };
            *out_data_size = needed;
        }

        kAudioDevicePropertyZeroTimeStampPeriod => {
            let needed = mem::size_of::<u32>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut u32) = constants::ZERO_TIMESTAMP_PERIOD;
            *out_data_size = needed;
        }

        kAudioDevicePropertyClockAlgorithm => {
            let needed = mem::size_of::<u32>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut u32) = kAudioDeviceClockAlgorithmRaw;
            *out_data_size = needed;
        }

        kAudioDevicePropertyClockIsStable => {
            let needed = mem::size_of::<u32>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut u32) = 1; // Stable clock
            *out_data_size = needed;
        }

        kAudioDevicePropertyConfigurationApplication => {
            let needed = mem::size_of::<CFStringRef>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut CFStringRef) = cfstring_create(constants::APP_BUNDLE_ID);
            *out_data_size = needed;
        }

        kAudioDevicePropertyRelatedDevices => {
            let needed = mem::size_of::<AudioObjectID>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            // Self only
            *(out_data as *mut AudioObjectID) = constants::DEVICE_OBJECT_ID;
            *out_data_size = needed;
        }

        kAudioObjectPropertyControlList => {
            // No controls — empty array
            *out_data_size = 0;
        }

        kAudioDevicePropertyIsHidden => {
            let needed = mem::size_of::<u32>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut u32) = 0; // Not hidden
            *out_data_size = needed;
        }

        _ => return kAudioHardwareUnknownPropertyError as OSStatus,
    }

    kAudioHardwareNoError as OSStatus
}

// ---------------------------------------------------------------------------
// SetPropertyData
// ---------------------------------------------------------------------------

pub unsafe fn set_property_data(
    addr: &AudioObjectPropertyAddress,
    in_data_size: u32,
    in_data: *const std::ffi::c_void,
) -> OSStatus {
    match addr.mSelector {
        kAudioDevicePropertyNominalSampleRate => {
            let needed = mem::size_of::<f64>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            let new_rate = *(in_data as *const f64);
            // Validate against supported rates
            if !constants::SUPPORTED_SAMPLE_RATES.contains(&new_rate) {
                return kAudioHardwareIllegalOperationError as OSStatus;
            }

            // Store pending value and request configuration change via host
            DRIVER_STATE
                .pending_sample_rate_bits
                .store(new_rate.to_bits(), std::sync::atomic::Ordering::Release);

            let host = DRIVER_STATE.host.load(std::sync::atomic::Ordering::Acquire);
            if !host.is_null() {
                let host_ref = &*host;
                if let Some(request_fn) = host_ref.RequestDeviceConfigurationChange {
                    let status = request_fn(
                        host,
                        constants::DEVICE_OBJECT_ID,
                        constants::ACTION_SET_SAMPLE_RATE,
                        std::ptr::null_mut(),
                    );
                    if status != kAudioHardwareNoError as OSStatus {
                        // Fallback: apply directly if host rejects the request
                        DRIVER_STATE
                            .sample_rate_bits
                            .store(new_rate.to_bits(), std::sync::atomic::Ordering::Release);
                    }
                    return status;
                }
            }

            // No host available (e.g., during tests) — apply directly
            DRIVER_STATE
                .sample_rate_bits
                .store(new_rate.to_bits(), std::sync::atomic::Ordering::Release);
            kAudioHardwareNoError as OSStatus
        }

        kAudioDevicePropertyBufferFrameSize => {
            let needed = mem::size_of::<u32>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            let new_size = *(in_data as *const u32);
            // Clamp to valid range
            let clamped = new_size.clamp(
                constants::MIN_BUFFER_FRAME_SIZE,
                constants::MAX_BUFFER_FRAME_SIZE,
            );

            // Store pending value and request configuration change via host
            DRIVER_STATE
                .pending_buffer_frame_size
                .store(clamped, std::sync::atomic::Ordering::Release);

            let host = DRIVER_STATE.host.load(std::sync::atomic::Ordering::Acquire);
            if !host.is_null() {
                let host_ref = &*host;
                if let Some(request_fn) = host_ref.RequestDeviceConfigurationChange {
                    let status = request_fn(
                        host,
                        constants::DEVICE_OBJECT_ID,
                        constants::ACTION_SET_BUFFER_SIZE,
                        std::ptr::null_mut(),
                    );
                    if status != kAudioHardwareNoError as OSStatus {
                        // Fallback: apply directly
                        DRIVER_STATE
                            .buffer_frame_size
                            .store(clamped, std::sync::atomic::Ordering::Release);
                    }
                    return status;
                }
            }

            // No host available — apply directly
            DRIVER_STATE
                .buffer_frame_size
                .store(clamped, std::sync::atomic::Ordering::Release);
            kAudioHardwareNoError as OSStatus
        }

        _ => kAudioHardwareUnknownPropertyError as OSStatus,
    }
}
