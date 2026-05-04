//! Property handling for Stream objects (Object IDs 3 and 4).
//!
//! Stream 3 = input (what Zoom reads), direction = 1
//! Stream 4 = output (what Klaar writes), direction = 0

use std::ffi::c_void;
use std::mem;

use coreaudio_sys::*;

use crate::constants;
use crate::types::cfstring_create;
use crate::DRIVER_STATE;

/// Build the ASBD for the current sample rate.
fn make_asbd(sample_rate: f64) -> AudioStreamBasicDescription {
    AudioStreamBasicDescription {
        mSampleRate: sample_rate,
        mFormatID: kAudioFormatLinearPCM,
        mFormatFlags: kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
        mBytesPerPacket: constants::CHANNEL_COUNT * 4, // 2ch × 4 bytes
        mFramesPerPacket: 1,
        mBytesPerFrame: constants::CHANNEL_COUNT * 4,
        mChannelsPerFrame: constants::CHANNEL_COUNT,
        mBitsPerChannel: 32,
        mReserved: 0,
    }
}

fn is_input(object_id: AudioObjectID) -> bool {
    object_id == constants::INPUT_STREAM_OBJECT_ID
}

// ---------------------------------------------------------------------------
// HasProperty
// ---------------------------------------------------------------------------

pub fn has_property(_object_id: AudioObjectID, addr: &AudioObjectPropertyAddress) -> Boolean {
    match addr.mSelector {
        kAudioObjectPropertyBaseClass
        | kAudioObjectPropertyClass
        | kAudioObjectPropertyOwner
        | kAudioObjectPropertyName
        | kAudioStreamPropertyDirection
        | kAudioStreamPropertyTerminalType
        | kAudioStreamPropertyStartingChannel
        | kAudioStreamPropertyLatency
        | kAudioStreamPropertyVirtualFormat
        | kAudioStreamPropertyAvailableVirtualFormats
        | kAudioStreamPropertyPhysicalFormat
        | kAudioStreamPropertyAvailablePhysicalFormats => 1,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// IsPropertySettable
// ---------------------------------------------------------------------------

pub fn is_property_settable(
    _object_id: AudioObjectID,
    _addr: &AudioObjectPropertyAddress,
) -> Boolean {
    0 // No stream properties are settable.
}

// ---------------------------------------------------------------------------
// GetPropertyDataSize
// ---------------------------------------------------------------------------

pub unsafe fn get_property_data_size(
    _object_id: AudioObjectID,
    addr: &AudioObjectPropertyAddress,
    out_data_size: *mut u32,
) -> OSStatus {
    let size: u32 = match addr.mSelector {
        kAudioObjectPropertyBaseClass => mem::size_of::<AudioClassID>() as u32,
        kAudioObjectPropertyClass => mem::size_of::<AudioClassID>() as u32,
        kAudioObjectPropertyOwner => mem::size_of::<AudioObjectID>() as u32,
        kAudioObjectPropertyName => mem::size_of::<CFStringRef>() as u32,
        kAudioStreamPropertyDirection => mem::size_of::<u32>() as u32,
        kAudioStreamPropertyTerminalType => mem::size_of::<u32>() as u32,
        kAudioStreamPropertyStartingChannel => mem::size_of::<u32>() as u32,
        kAudioStreamPropertyLatency => mem::size_of::<u32>() as u32,
        kAudioStreamPropertyVirtualFormat => mem::size_of::<AudioStreamBasicDescription>() as u32,
        kAudioStreamPropertyAvailableVirtualFormats => {
            (constants::SUPPORTED_SAMPLE_RATES.len()
                * mem::size_of::<AudioStreamRangedDescription>()) as u32
        }
        kAudioStreamPropertyPhysicalFormat => {
            mem::size_of::<AudioStreamBasicDescription>() as u32
        }
        kAudioStreamPropertyAvailablePhysicalFormats => {
            (constants::SUPPORTED_SAMPLE_RATES.len()
                * mem::size_of::<AudioStreamRangedDescription>()) as u32
        }
        _ => return kAudioHardwareUnknownPropertyError as OSStatus,
    };

    *out_data_size = size;
    kAudioHardwareNoError as OSStatus
}

// ---------------------------------------------------------------------------
// GetPropertyData
// ---------------------------------------------------------------------------

pub unsafe fn get_property_data(
    object_id: AudioObjectID,
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
            *(out_data as *mut AudioClassID) = kAudioStreamClassID;
            *out_data_size = needed;
        }

        kAudioObjectPropertyOwner => {
            let needed = mem::size_of::<AudioObjectID>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut AudioObjectID) = constants::DEVICE_OBJECT_ID;
            *out_data_size = needed;
        }

        kAudioObjectPropertyName => {
            let needed = mem::size_of::<CFStringRef>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            let name = if is_input(object_id) {
                constants::INPUT_STREAM_NAME
            } else {
                constants::OUTPUT_STREAM_NAME
            };
            *(out_data as *mut CFStringRef) = cfstring_create(name);
            *out_data_size = needed;
        }

        kAudioStreamPropertyDirection => {
            let needed = mem::size_of::<u32>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            // Input = 1, Output = 0
            *(out_data as *mut u32) = if is_input(object_id) { 1 } else { 0 };
            *out_data_size = needed;
        }

        kAudioStreamPropertyTerminalType => {
            let needed = mem::size_of::<u32>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            let terminal = if is_input(object_id) {
                kAudioStreamTerminalTypeMicrophone
            } else {
                kAudioStreamTerminalTypeSpeaker
            };
            *(out_data as *mut u32) = terminal;
            *out_data_size = needed;
        }

        kAudioStreamPropertyStartingChannel => {
            let needed = mem::size_of::<u32>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut u32) = 1;
            *out_data_size = needed;
        }

        kAudioStreamPropertyLatency => {
            let needed = mem::size_of::<u32>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut u32) = 0;
            *out_data_size = needed;
        }

        kAudioStreamPropertyVirtualFormat | kAudioStreamPropertyPhysicalFormat => {
            let needed = mem::size_of::<AudioStreamBasicDescription>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut AudioStreamBasicDescription) =
                make_asbd(DRIVER_STATE.sample_rate());
            *out_data_size = needed;
        }

        kAudioStreamPropertyAvailableVirtualFormats
        | kAudioStreamPropertyAvailablePhysicalFormats => {
            let count = constants::SUPPORTED_SAMPLE_RATES.len();
            let needed = (count * mem::size_of::<AudioStreamRangedDescription>()) as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            let descs = out_data as *mut AudioStreamRangedDescription;
            for (i, &rate) in constants::SUPPORTED_SAMPLE_RATES.iter().enumerate() {
                *descs.add(i) = AudioStreamRangedDescription {
                    mFormat: make_asbd(rate),
                    mSampleRateRange: AudioValueRange {
                        mMinimum: rate,
                        mMaximum: rate,
                    },
                };
            }
            *out_data_size = needed;
        }

        _ => return kAudioHardwareUnknownPropertyError as OSStatus,
    }

    kAudioHardwareNoError as OSStatus
}
