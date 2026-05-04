/// CoreAudio device enumeration and UID resolution.
///
/// Uses `coreaudio::audio_unit::macos_helpers` for device listing and scope checks,
/// and raw CoreAudio property queries for UID and sample rate (not exposed by macos_helpers).
use coreaudio::audio_unit::macos_helpers::{
    get_audio_device_supports_scope, get_device_name,
};
use coreaudio::audio_unit::Scope;
use coreaudio::sys::{
    kAudioDevicePropertyDeviceUID,
    kAudioDevicePropertyNominalSampleRate,
    kAudioHardwarePropertyDevices,
    kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal,
    kAudioObjectSystemObject,
    AudioDeviceID,
    AudioObjectGetPropertyData,
    AudioObjectGetPropertyDataSize,
    AudioObjectPropertyAddress,
};
use core_foundation::string::{CFString, CFStringRef};
use serde::{Deserialize, Serialize};
use std::mem;
use std::ptr;

/// Represents an audio device exposed to the frontend.
///
/// Field names follow camelCase for JSON serialisation (`sampleRate`),
/// matching the TypeScript `AudioDevice` interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    /// CoreAudio device UID (unique, stable across reboots). Serialised as `"id"`.
    #[serde(rename = "id")]
    pub uid: String,
    /// Human-readable display name
    pub name: String,
    /// Native sample rate in Hz. Serialised as `"sampleRate"`.
    pub sample_rate: f64,
    /// Number of input or output channels (depending on query scope)
    pub channels: u32,
}

/// Enumerate all CoreAudio input devices (devices with ≥ 1 input channel).
pub fn list_input_devices() -> Result<Vec<AudioDevice>, String> {
    list_devices(Scope::Input)
}

/// Enumerate all CoreAudio output devices (devices with ≥ 1 output channel).
pub fn list_output_devices() -> Result<Vec<AudioDevice>, String> {
    list_devices(Scope::Output)
}

/// Validate that a device UID exists in the given scope.
pub fn validate_device_uid(uid: &str, input: bool) -> Result<(), String> {
    let devices = if input {
        list_input_devices()?
    } else {
        list_output_devices()?
    };
    if devices.iter().any(|d| d.uid == uid) {
        Ok(())
    } else {
        Err(format!("Device '{}' not found", uid))
    }
}

/// Resolve a device UID → AudioDeviceID.
pub fn get_device_id_by_uid(uid: &str) -> Result<AudioDeviceID, String> {
    let all_ids = get_all_device_ids()?;
    for id in all_ids {
        if let Ok(d_uid) = raw_get_device_uid(id) {
            if d_uid == uid {
                return Ok(id);
            }
        }
    }
    Err(format!("Device '{}' not found", uid))
}

/// Get the nominal sample rate for a device identified by UID.
pub fn get_device_sample_rate_by_uid(uid: &str) -> Result<f64, String> {
    let id = get_device_id_by_uid(uid)?;
    raw_get_sample_rate(id)
}

// ──────────────────────────────────────────────────────────────────────────────
// Private helpers
// ──────────────────────────────────────────────────────────────────────────────

fn list_devices(scope: Scope) -> Result<Vec<AudioDevice>, String> {
    let all_ids = get_all_device_ids()?;
    let mut results = Vec::new();

    for id in all_ids {
        // Filter by scope using macos_helpers
        let supports = get_audio_device_supports_scope(id, scope)
            .unwrap_or(false);
        if !supports {
            continue;
        }

        let name = get_device_name(id)
            .unwrap_or_else(|_| format!("Device {id}"));
        let uid = raw_get_device_uid(id)
            .unwrap_or_else(|_| id.to_string());
        let sample_rate = raw_get_sample_rate(id).unwrap_or(0.0);

        // Rough channel count (1 means mono capable; exact count is less important for UI)
        let channels = raw_get_channel_count(id, scope).unwrap_or(1);

        results.push(AudioDevice {
            uid,
            name,
            sample_rate,
            channels,
        });
    }

    Ok(results)
}

/// Enumerate every CoreAudio device ID known to the system, regardless of
/// scope (input or output). Used by `DeviceFormatListener` to register a
/// `kAudioDevicePropertyNominalSampleRate` listener on every device.
pub fn list_all_device_ids() -> Result<Vec<AudioDeviceID>, String> {
    get_all_device_ids()
}

fn get_all_device_ids() -> Result<Vec<AudioDeviceID>, String> {
    let addr = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDevices,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };

    let mut data_size: u32 = 0;
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            kAudioObjectSystemObject,
            &addr,
            0,
            ptr::null(),
            &mut data_size,
        )
    };
    if status != 0 {
        return Err(format!(
            "AudioObjectGetPropertyDataSize(devices): {:#010x}",
            status as u32
        ));
    }

    let count = data_size as usize / mem::size_of::<AudioDeviceID>();
    let mut ids: Vec<AudioDeviceID> = vec![0; count];

    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject,
            &addr,
            0,
            ptr::null(),
            &mut data_size,
            ids.as_mut_ptr() as *mut _,
        )
    };
    if status != 0 {
        return Err(format!(
            "AudioObjectGetPropertyData(devices): {:#010x}",
            status as u32
        ));
    }

    Ok(ids)
}

fn raw_get_device_uid(device_id: AudioDeviceID) -> Result<String, String> {
    let addr = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyDeviceUID,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };

    let mut cf_string_ref: CFStringRef = ptr::null();
    let mut data_size = mem::size_of::<CFStringRef>() as u32;

    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            &addr,
            0,
            ptr::null(),
            &mut data_size,
            &mut cf_string_ref as *mut _ as *mut _,
        )
    };

    if status != 0 || cf_string_ref.is_null() {
        return Err(format!("get_uid({device_id}): {:#010x}", status as u32));
    }

    let uid = unsafe {
        use core_foundation::base::TCFType;
        CFString::wrap_under_create_rule(cf_string_ref).to_string()
    };

    Ok(uid)
}

fn raw_get_sample_rate(device_id: AudioDeviceID) -> Result<f64, String> {
    let addr = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyNominalSampleRate,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };

    let mut rate: f64 = 0.0;
    let mut data_size = mem::size_of::<f64>() as u32;

    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            &addr,
            0,
            ptr::null(),
            &mut data_size,
            &mut rate as *mut _ as *mut _,
        )
    };

    if status != 0 {
        return Err(format!(
            "get_sample_rate({device_id}): {:#010x}",
            status as u32
        ));
    }

    Ok(rate)
}

fn raw_get_channel_count(device_id: AudioDeviceID, scope: Scope) -> Result<u32, String> {
    use coreaudio::sys::{
        kAudioDevicePropertyStreamConfiguration,
        kAudioObjectPropertyScopeInput,
        kAudioObjectPropertyScopeOutput,
        AudioBufferList,
    };

    let scope_const = match scope {
        Scope::Input => kAudioObjectPropertyScopeInput,
        Scope::Output => kAudioObjectPropertyScopeOutput,
        _ => kAudioObjectPropertyScopeGlobal,
    };

    let addr = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyStreamConfiguration,
        mScope: scope_const,
        mElement: kAudioObjectPropertyElementMain,
    };

    let mut data_size: u32 = 0;
    let status = unsafe {
        AudioObjectGetPropertyDataSize(device_id, &addr, 0, ptr::null(), &mut data_size)
    };
    if status != 0 || data_size == 0 {
        return Ok(0);
    }

    let mut buf: Vec<u8> = vec![0; data_size as usize];
    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            &addr,
            0,
            ptr::null(),
            &mut data_size,
            buf.as_mut_ptr() as *mut _,
        )
    };
    if status != 0 {
        return Ok(0);
    }

    let buf_list = unsafe { &*(buf.as_ptr() as *const AudioBufferList) };
    let num_buffers = buf_list.mNumberBuffers as usize;
    let mut total = 0u32;

    for i in 0..num_buffers {
        let offset = mem::offset_of!(AudioBufferList, mBuffers)
            + i * mem::size_of::<coreaudio::sys::AudioBuffer>();
        if offset + mem::size_of::<coreaudio::sys::AudioBuffer>() <= buf.len() {
            let ab = unsafe {
                &*(buf.as_ptr().add(offset) as *const coreaudio::sys::AudioBuffer)
            };
            total += ab.mNumberChannels;
        }
    }

    Ok(total)
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_device_uid_invalid_returns_err_with_uid() {
        let bogus = "BOGUS-UID-THAT-DOES-NOT-EXIST-12345";
        let result = validate_device_uid(bogus, true);
        assert!(result.is_err(), "invalid UID must return Err");
        let msg = result.unwrap_err();
        assert!(
            msg.contains(bogus),
            "error message must contain the invalid UID; got: {msg}"
        );
    }

    #[test]
    fn validate_device_uid_empty_returns_err() {
        let result = validate_device_uid("", true);
        assert!(result.is_err(), "empty UID must return Err");
    }

    #[test]
    fn validate_device_uid_empty_output_returns_err() {
        let result = validate_device_uid("", false);
        assert!(result.is_err(), "empty UID (output scope) must return Err");
    }
}
