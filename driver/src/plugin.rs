//! Property handling for the Plugin object (Object ID 1).

use std::ffi::c_void;

use coreaudio_sys::*;

use crate::constants;
use crate::types::cfstring_create;

/// Check if the plugin object has the given property.
pub fn has_property(addr: &AudioObjectPropertyAddress) -> Boolean {
    match addr.mSelector {
        kAudioObjectPropertyBaseClass
        | kAudioObjectPropertyClass
        | kAudioObjectPropertyOwner
        | kAudioObjectPropertyManufacturer
        | kAudioPlugInPropertyDeviceList
        | kAudioPlugInPropertyBoxList
        | kAudioPlugInPropertyResourceBundle => 1,
        _ => 0,
    }
}

/// Check if the property is settable.
pub fn is_property_settable(_addr: &AudioObjectPropertyAddress) -> Boolean {
    0 // No plugin properties are settable
}

/// Get the data size of the property.
pub unsafe fn get_property_data_size(
    addr: &AudioObjectPropertyAddress,
    out_data_size: *mut u32,
) -> OSStatus {
    let size: u32 = match addr.mSelector {
        kAudioObjectPropertyBaseClass => std::mem::size_of::<AudioClassID>() as u32,
        kAudioObjectPropertyClass => std::mem::size_of::<AudioClassID>() as u32,
        kAudioObjectPropertyOwner => std::mem::size_of::<AudioObjectID>() as u32,
        kAudioObjectPropertyManufacturer => std::mem::size_of::<CFStringRef>() as u32,
        kAudioPlugInPropertyDeviceList => std::mem::size_of::<AudioObjectID>() as u32, // 1 device
        kAudioPlugInPropertyBoxList => 0, // empty array
        kAudioPlugInPropertyResourceBundle => std::mem::size_of::<CFStringRef>() as u32,
        _ => return kAudioHardwareUnknownPropertyError as OSStatus,
    };

    *out_data_size = size;
    kAudioHardwareNoError as OSStatus
}

/// Get the property data.
pub unsafe fn get_property_data(
    addr: &AudioObjectPropertyAddress,
    in_data_size: u32,
    out_data_size: *mut u32,
    out_data: *mut c_void,
) -> OSStatus {
    match addr.mSelector {
        kAudioObjectPropertyBaseClass => {
            let needed = std::mem::size_of::<AudioClassID>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut AudioClassID) = kAudioObjectClassID;
            *out_data_size = needed;
        }
        kAudioObjectPropertyClass => {
            let needed = std::mem::size_of::<AudioClassID>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut AudioClassID) = kAudioPlugInClassID;
            *out_data_size = needed;
        }
        kAudioObjectPropertyOwner => {
            let needed = std::mem::size_of::<AudioObjectID>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut AudioObjectID) = kAudioObjectPlugInObject;
            *out_data_size = needed;
        }
        kAudioObjectPropertyManufacturer => {
            let needed = std::mem::size_of::<CFStringRef>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            let cf_str = cfstring_create(constants::MANUFACTURER);
            *(out_data as *mut CFStringRef) = cf_str;
            *out_data_size = needed;
        }
        kAudioPlugInPropertyDeviceList => {
            let needed = std::mem::size_of::<AudioObjectID>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            *(out_data as *mut AudioObjectID) = constants::DEVICE_OBJECT_ID;
            *out_data_size = needed;
        }
        kAudioPlugInPropertyBoxList => {
            // Empty array — no boxes
            *out_data_size = 0;
        }
        kAudioPlugInPropertyResourceBundle => {
            let needed = std::mem::size_of::<CFStringRef>() as u32;
            if in_data_size < needed {
                return kAudioHardwareBadPropertySizeError as OSStatus;
            }
            // Empty string for resource bundle — driver has no separate resources
            let cf_str = cfstring_create("");
            *(out_data as *mut CFStringRef) = cf_str;
            *out_data_size = needed;
        }
        _ => return kAudioHardwareUnknownPropertyError as OSStatus,
    }

    kAudioHardwareNoError as OSStatus
}

