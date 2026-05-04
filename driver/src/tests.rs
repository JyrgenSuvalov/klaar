use std::ffi::c_void;
use std::mem;

use coreaudio_sys::*;

use crate::{constants, device, plugin, stream};

/// Helper: make an AudioObjectPropertyAddress with global scope.
fn addr(selector: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    }
}

/// Helper: make an AudioObjectPropertyAddress with a specific scope.
fn addr_scoped(selector: u32, scope: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMain,
    }
}

// ===========================================================================
// Plugin (Object ID 1) Tests
// ===========================================================================

#[test]
fn plugin_has_known_properties() {
    assert_eq!(plugin::has_property(&addr(kAudioObjectPropertyBaseClass)), 1);
    assert_eq!(plugin::has_property(&addr(kAudioObjectPropertyClass)), 1);
    assert_eq!(plugin::has_property(&addr(kAudioObjectPropertyOwner)), 1);
    assert_eq!(
        plugin::has_property(&addr(kAudioObjectPropertyManufacturer)),
        1
    );
    assert_eq!(
        plugin::has_property(&addr(kAudioPlugInPropertyDeviceList)),
        1
    );
    assert_eq!(plugin::has_property(&addr(kAudioPlugInPropertyBoxList)), 1);
    assert_eq!(
        plugin::has_property(&addr(kAudioPlugInPropertyResourceBundle)),
        1
    );
}

#[test]
fn plugin_rejects_unknown_property() {
    assert_eq!(plugin::has_property(&addr(0x12345678)), 0);
}

#[test]
fn plugin_no_properties_settable() {
    assert_eq!(
        plugin::is_property_settable(&addr(kAudioObjectPropertyManufacturer)),
        0
    );
    assert_eq!(
        plugin::is_property_settable(&addr(kAudioPlugInPropertyDeviceList)),
        0
    );
}

#[test]
fn plugin_base_class_is_object() {
    unsafe {
        let mut size: u32 = 0;
        let status = plugin::get_property_data_size(&addr(kAudioObjectPropertyBaseClass), &mut size);
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(size, mem::size_of::<AudioClassID>() as u32);

        let mut value: AudioClassID = 0;
        let mut out_size: u32 = 0;
        let status = plugin::get_property_data(
            &addr(kAudioObjectPropertyBaseClass),
            size,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(value, kAudioObjectClassID);
    }
}

#[test]
fn plugin_class_is_plugin() {
    unsafe {
        let mut value: AudioClassID = 0;
        let mut out_size: u32 = 0;
        let status = plugin::get_property_data(
            &addr(kAudioObjectPropertyClass),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(value, kAudioPlugInClassID);
    }
}

#[test]
fn plugin_device_list_contains_device_2() {
    unsafe {
        let mut value: AudioObjectID = 0;
        let mut out_size: u32 = 0;
        let status = plugin::get_property_data(
            &addr(kAudioPlugInPropertyDeviceList),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(value, constants::DEVICE_OBJECT_ID);
        assert_eq!(out_size, 4);
    }
}

#[test]
fn plugin_box_list_is_empty() {
    unsafe {
        let mut size: u32 = 0;
        let status = plugin::get_property_data_size(&addr(kAudioPlugInPropertyBoxList), &mut size);
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(size, 0);
    }
}

// ===========================================================================
// Device (Object ID 2) Tests
// ===========================================================================

#[test]
fn device_has_known_properties() {
    assert_eq!(device::has_property(&addr(kAudioObjectPropertyName)), 1);
    assert_eq!(
        device::has_property(&addr(kAudioDevicePropertyDeviceUID)),
        1
    );
    assert_eq!(
        device::has_property(&addr(kAudioDevicePropertyNominalSampleRate)),
        1
    );
    assert_eq!(
        device::has_property(&addr(kAudioDevicePropertyBufferFrameSize)),
        1
    );
    assert_eq!(
        device::has_property(&addr(kAudioDevicePropertyTransportType)),
        1
    );
    assert_eq!(
        device::has_property(&addr(kAudioDevicePropertyClockAlgorithm)),
        1
    );
}

#[test]
fn device_rejects_unknown_property() {
    assert_eq!(device::has_property(&addr(0xDEADBEEF)), 0);
}

#[test]
fn device_sample_rate_is_settable() {
    assert_eq!(
        device::is_property_settable(&addr(kAudioDevicePropertyNominalSampleRate)),
        1
    );
}

#[test]
fn device_buffer_size_is_settable() {
    assert_eq!(
        device::is_property_settable(&addr(kAudioDevicePropertyBufferFrameSize)),
        1
    );
}

#[test]
fn device_name_is_not_settable() {
    assert_eq!(
        device::is_property_settable(&addr(kAudioObjectPropertyName)),
        0
    );
}

#[test]
fn device_class_is_device() {
    unsafe {
        let mut value: AudioClassID = 0;
        let mut out_size: u32 = 0;
        let status = device::get_property_data(
            &addr(kAudioObjectPropertyClass),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(value, kAudioDeviceClassID);
    }
}

#[test]
fn device_transport_type_is_virtual() {
    unsafe {
        let mut value: u32 = 0;
        let mut out_size: u32 = 0;
        let status = device::get_property_data(
            &addr(kAudioDevicePropertyTransportType),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(value, kAudioDeviceTransportTypeVirtual);
    }
}

#[test]
fn device_is_alive() {
    unsafe {
        let mut value: u32 = 0;
        let mut out_size: u32 = 0;
        let status = device::get_property_data(
            &addr(kAudioDevicePropertyDeviceIsAlive),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(value, 1);
    }
}

#[test]
fn default_device_input_scope_returns_one() {
    // Input scope: this virtual device IS eligible to be the default mic.
    unsafe {
        let mut value: u32 = 0;
        let mut out_size: u32 = 0;
        let status = device::get_property_data(
            &addr_scoped(
                kAudioDevicePropertyDeviceCanBeDefaultDevice,
                kAudioObjectPropertyScopeInput,
            ),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(value, 1);
    }
}

#[test]
fn default_device_output_scope_returns_zero() {
    // Output scope: the virtual mic MUST NOT be eligible as default output.
    // This is the core invariant of add-driver-scope-aware-default-capability.
    unsafe {
        let mut value: u32 = 0xDEAD_BEEF;
        let mut out_size: u32 = 0;
        let status = device::get_property_data(
            &addr_scoped(
                kAudioDevicePropertyDeviceCanBeDefaultDevice,
                kAudioObjectPropertyScopeOutput,
            ),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(value, 0);
    }
}

#[test]
fn default_system_device_input_scope_returns_zero() {
    // We don't want system sounds routed through the virtual mic on any scope.
    unsafe {
        let mut value: u32 = 0xDEAD_BEEF;
        let mut out_size: u32 = 0;
        let status = device::get_property_data(
            &addr_scoped(
                kAudioDevicePropertyDeviceCanBeDefaultSystemDevice,
                kAudioObjectPropertyScopeInput,
            ),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(value, 0);
    }
}

#[test]
fn default_system_device_output_scope_returns_zero() {
    unsafe {
        let mut value: u32 = 0xDEAD_BEEF;
        let mut out_size: u32 = 0;
        let status = device::get_property_data(
            &addr_scoped(
                kAudioDevicePropertyDeviceCanBeDefaultSystemDevice,
                kAudioObjectPropertyScopeOutput,
            ),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(value, 0);
    }
}

#[test]
fn device_sample_rate_is_48k() {
    unsafe {
        let mut value: f64 = 0.0;
        let mut out_size: u32 = 0;
        let status = device::get_property_data(
            &addr(kAudioDevicePropertyNominalSampleRate),
            8,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(out_size, 8);
        assert_eq!(value, 48000.0);
    }
}

#[test]
fn device_buffer_frame_size_is_512() {
    unsafe {
        let mut value: u32 = 0;
        let mut out_size: u32 = 0;
        let status = device::get_property_data(
            &addr(kAudioDevicePropertyBufferFrameSize),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(value, 512);
    }
}

#[test]
fn device_available_sample_rates() {
    unsafe {
        let mut size: u32 = 0;
        let status = device::get_property_data_size(
            &addr(kAudioDevicePropertyAvailableNominalSampleRates),
            &mut size,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(
            size,
            (2 * mem::size_of::<AudioValueRange>()) as u32
        );

        let mut ranges: [AudioValueRange; 2] = [
            AudioValueRange { mMinimum: 0.0, mMaximum: 0.0 },
            AudioValueRange { mMinimum: 0.0, mMaximum: 0.0 },
        ];
        let mut out_size: u32 = 0;
        let status = device::get_property_data(
            &addr(kAudioDevicePropertyAvailableNominalSampleRates),
            size,
            &mut out_size,
            ranges.as_mut_ptr() as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(ranges[0].mMinimum, 44100.0);
        assert_eq!(ranges[0].mMaximum, 44100.0);
        assert_eq!(ranges[1].mMinimum, 48000.0);
        assert_eq!(ranges[1].mMaximum, 48000.0);
    }
}

#[test]
fn device_zero_timestamp_period() {
    unsafe {
        let mut value: u32 = 0;
        let mut out_size: u32 = 0;
        let status = device::get_property_data(
            &addr(kAudioDevicePropertyZeroTimeStampPeriod),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(value, constants::ZERO_TIMESTAMP_PERIOD);
    }
}

#[test]
fn device_zero_timestamp_period_is_16384() {
    // Pinned literal — fix-driver-latency requires this exact value. If you
    // change the constant, you must update the proposal/spec, not just this
    // test.
    unsafe {
        let mut value: u32 = 0;
        let mut out_size: u32 = 0;
        let status = device::get_property_data(
            &addr(kAudioDevicePropertyZeroTimeStampPeriod),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(out_size, 4);
        assert_eq!(value, 16_384u32);
    }
}

#[test]
fn device_has_latency_property() {
    assert_eq!(
        device::has_property(&addr(kAudioDevicePropertyLatency)),
        1
    );
}

#[test]
fn device_has_safety_offset_property() {
    assert_eq!(
        device::has_property(&addr(kAudioDevicePropertySafetyOffset)),
        1
    );
}

#[test]
fn device_latency_value_is_zero() {
    unsafe {
        let mut value: u32 = 0xDEAD_BEEF;
        let mut out_size: u32 = 0;
        let status = device::get_property_data(
            &addr(kAudioDevicePropertyLatency),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(out_size, 4);
        assert_eq!(value, 0u32);
    }
}

#[test]
fn device_safety_offset_value_is_zero() {
    unsafe {
        let mut value: u32 = 0xDEAD_BEEF;
        let mut out_size: u32 = 0;
        let status = device::get_property_data(
            &addr(kAudioDevicePropertySafetyOffset),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(out_size, 4);
        assert_eq!(value, 0u32);
    }
}

#[test]
fn device_clock_algorithm_raw() {
    unsafe {
        let mut value: u32 = 0;
        let mut out_size: u32 = 0;
        let status = device::get_property_data(
            &addr(kAudioDevicePropertyClockAlgorithm),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(value, kAudioDeviceClockAlgorithmRaw);
    }
}

#[test]
fn device_streams_input_scope() {
    unsafe {
        let mut value: AudioObjectID = 0;
        let mut out_size: u32 = 0;
        let status = device::get_property_data(
            &addr_scoped(kAudioDevicePropertyStreams, kAudioObjectPropertyScopeInput),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(value, constants::INPUT_STREAM_OBJECT_ID);
    }
}

#[test]
fn device_streams_output_scope() {
    unsafe {
        let mut value: AudioObjectID = 0;
        let mut out_size: u32 = 0;
        let status = device::get_property_data(
            &addr_scoped(kAudioDevicePropertyStreams, kAudioObjectPropertyScopeOutput),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(value, constants::OUTPUT_STREAM_OBJECT_ID);
    }
}

#[test]
fn device_owned_objects() {
    unsafe {
        let mut values: [AudioObjectID; 2] = [0, 0];
        let mut out_size: u32 = 0;
        let status = device::get_property_data(
            &addr(kAudioObjectPropertyOwnedObjects),
            8,
            &mut out_size,
            values.as_mut_ptr() as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(out_size, 8);
        assert_eq!(values[0], constants::INPUT_STREAM_OBJECT_ID);
        assert_eq!(values[1], constants::OUTPUT_STREAM_OBJECT_ID);
    }
}

// ===========================================================================
// Stream (Object IDs 3 & 4) Tests
// ===========================================================================

#[test]
fn stream_has_known_properties() {
    let id = constants::INPUT_STREAM_OBJECT_ID;
    assert_eq!(
        stream::has_property(id, &addr(kAudioStreamPropertyDirection)),
        1
    );
    assert_eq!(
        stream::has_property(id, &addr(kAudioStreamPropertyVirtualFormat)),
        1
    );
    assert_eq!(
        stream::has_property(id, &addr(kAudioObjectPropertyClass)),
        1
    );
    assert_eq!(
        stream::has_property(id, &addr(kAudioObjectPropertyOwner)),
        1
    );
}

#[test]
fn stream_rejects_unknown_property() {
    assert_eq!(
        stream::has_property(constants::INPUT_STREAM_OBJECT_ID, &addr(0xBAADF00D)),
        0
    );
}

#[test]
fn stream_has_latency_property() {
    assert_eq!(
        stream::has_property(
            constants::INPUT_STREAM_OBJECT_ID,
            &addr(kAudioStreamPropertyLatency)
        ),
        1
    );
    assert_eq!(
        stream::has_property(
            constants::OUTPUT_STREAM_OBJECT_ID,
            &addr(kAudioStreamPropertyLatency)
        ),
        1
    );
}

#[test]
fn stream_latency_value_is_zero() {
    unsafe {
        for &id in &[
            constants::INPUT_STREAM_OBJECT_ID,
            constants::OUTPUT_STREAM_OBJECT_ID,
        ] {
            let mut value: u32 = 0xDEAD_BEEF;
            let mut out_size: u32 = 0;
            let status = stream::get_property_data(
                id,
                &addr(kAudioStreamPropertyLatency),
                4,
                &mut out_size,
                &mut value as *mut _ as *mut c_void,
            );
            assert_eq!(status, kAudioHardwareNoError as OSStatus);
            assert_eq!(out_size, 4);
            assert_eq!(value, 0u32);
        }
    }
}

#[test]
fn input_stream_direction() {
    unsafe {
        let mut value: u32 = 99;
        let mut out_size: u32 = 0;
        let status = stream::get_property_data(
            constants::INPUT_STREAM_OBJECT_ID,
            &addr(kAudioStreamPropertyDirection),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(value, 1); // Input
    }
}

#[test]
fn output_stream_direction() {
    unsafe {
        let mut value: u32 = 99;
        let mut out_size: u32 = 0;
        let status = stream::get_property_data(
            constants::OUTPUT_STREAM_OBJECT_ID,
            &addr(kAudioStreamPropertyDirection),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(value, 0); // Output
    }
}

#[test]
fn stream_class_is_stream() {
    unsafe {
        let mut value: AudioClassID = 0;
        let mut out_size: u32 = 0;
        let status = stream::get_property_data(
            constants::INPUT_STREAM_OBJECT_ID,
            &addr(kAudioObjectPropertyClass),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(value, kAudioStreamClassID);
    }
}

#[test]
fn stream_owner_is_device() {
    unsafe {
        let mut value: AudioObjectID = 0;
        let mut out_size: u32 = 0;
        let status = stream::get_property_data(
            constants::INPUT_STREAM_OBJECT_ID,
            &addr(kAudioObjectPropertyOwner),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(value, constants::DEVICE_OBJECT_ID);
    }
}

#[test]
fn stream_virtual_format_asbd() {
    unsafe {
        let mut asbd: AudioStreamBasicDescription = mem::zeroed();
        let mut out_size: u32 = 0;
        let status = stream::get_property_data(
            constants::INPUT_STREAM_OBJECT_ID,
            &addr(kAudioStreamPropertyVirtualFormat),
            mem::size_of::<AudioStreamBasicDescription>() as u32,
            &mut out_size,
            &mut asbd as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(
            out_size,
            mem::size_of::<AudioStreamBasicDescription>() as u32
        );
        assert_eq!(asbd.mSampleRate, 48000.0);
        assert_eq!(asbd.mFormatID, kAudioFormatLinearPCM);
        assert_eq!(
            asbd.mFormatFlags,
            kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked
        );
        assert_eq!(asbd.mChannelsPerFrame, 2);
        assert_eq!(asbd.mBitsPerChannel, 32);
        assert_eq!(asbd.mBytesPerFrame, 8);
        assert_eq!(asbd.mBytesPerPacket, 8);
        assert_eq!(asbd.mFramesPerPacket, 1);
    }
}

#[test]
fn stream_input_terminal_type_is_microphone() {
    unsafe {
        let mut value: u32 = 0;
        let mut out_size: u32 = 0;
        let status = stream::get_property_data(
            constants::INPUT_STREAM_OBJECT_ID,
            &addr(kAudioStreamPropertyTerminalType),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(value, kAudioStreamTerminalTypeMicrophone);
    }
}

#[test]
fn stream_output_terminal_type_is_speaker() {
    unsafe {
        let mut value: u32 = 0;
        let mut out_size: u32 = 0;
        let status = stream::get_property_data(
            constants::OUTPUT_STREAM_OBJECT_ID,
            &addr(kAudioStreamPropertyTerminalType),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(value, kAudioStreamTerminalTypeSpeaker);
    }
}

#[test]
fn stream_starting_channel_is_1() {
    unsafe {
        let mut value: u32 = 0;
        let mut out_size: u32 = 0;
        let status = stream::get_property_data(
            constants::INPUT_STREAM_OBJECT_ID,
            &addr(kAudioStreamPropertyStartingChannel),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(value, 1);
    }
}

#[test]
fn stream_available_formats() {
    unsafe {
        let mut size: u32 = 0;
        let status = stream::get_property_data_size(
            constants::INPUT_STREAM_OBJECT_ID,
            &addr(kAudioStreamPropertyAvailableVirtualFormats),
            &mut size,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(
            size,
            (2 * mem::size_of::<AudioStreamRangedDescription>()) as u32
        );

        let mut descs: [AudioStreamRangedDescription; 2] = mem::zeroed();
        let mut out_size: u32 = 0;
        let status = stream::get_property_data(
            constants::INPUT_STREAM_OBJECT_ID,
            &addr(kAudioStreamPropertyAvailableVirtualFormats),
            size,
            &mut out_size,
            descs.as_mut_ptr() as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(descs[0].mFormat.mSampleRate, 44100.0);
        assert_eq!(descs[1].mFormat.mSampleRate, 48000.0);
        assert_eq!(descs[0].mFormat.mChannelsPerFrame, 2);
    }
}

// ===========================================================================
// SetPropertyData tests (B2 fix)
// ===========================================================================

#[test]
fn device_set_buffer_frame_size() {
    unsafe {
        let new_size: u32 = 256;
        let status = device::set_property_data(
            &addr(kAudioDevicePropertyBufferFrameSize),
            mem::size_of::<u32>() as u32,
            &new_size as *const _ as *const c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);

        // Read back the value
        let mut value: u32 = 0;
        let mut out_size: u32 = 0;
        let status = device::get_property_data(
            &addr(kAudioDevicePropertyBufferFrameSize),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
        assert_eq!(value, 256);

        // Restore default
        let default: u32 = constants::DEFAULT_BUFFER_FRAME_SIZE;
        device::set_property_data(
            &addr(kAudioDevicePropertyBufferFrameSize),
            4,
            &default as *const _ as *const c_void,
        );
    }
}

#[test]
fn device_set_buffer_frame_size_clamps_to_range() {
    unsafe {
        // Try setting above max
        let huge: u32 = 999_999;
        let status = device::set_property_data(
            &addr(kAudioDevicePropertyBufferFrameSize),
            4,
            &huge as *const _ as *const c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);

        let mut value: u32 = 0;
        let mut out_size: u32 = 0;
        device::get_property_data(
            &addr(kAudioDevicePropertyBufferFrameSize),
            4,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(value, constants::MAX_BUFFER_FRAME_SIZE);

        // Restore default
        let default: u32 = constants::DEFAULT_BUFFER_FRAME_SIZE;
        device::set_property_data(
            &addr(kAudioDevicePropertyBufferFrameSize),
            4,
            &default as *const _ as *const c_void,
        );
    }
}

#[test]
fn device_set_sample_rate() {
    unsafe {
        let new_rate: f64 = 44_100.0;
        let status = device::set_property_data(
            &addr(kAudioDevicePropertyNominalSampleRate),
            mem::size_of::<f64>() as u32,
            &new_rate as *const _ as *const c_void,
        );
        assert_eq!(status, kAudioHardwareNoError as OSStatus);

        // Read back
        let mut value: f64 = 0.0;
        let mut out_size: u32 = 0;
        device::get_property_data(
            &addr(kAudioDevicePropertyNominalSampleRate),
            8,
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(value, 44_100.0);

        // Restore default
        let default: f64 = 48_000.0;
        device::set_property_data(
            &addr(kAudioDevicePropertyNominalSampleRate),
            8,
            &default as *const _ as *const c_void,
        );
    }
}

#[test]
fn device_set_sample_rate_rejects_unsupported() {
    unsafe {
        let bad_rate: f64 = 22_050.0;
        let status = device::set_property_data(
            &addr(kAudioDevicePropertyNominalSampleRate),
            mem::size_of::<f64>() as u32,
            &bad_rate as *const _ as *const c_void,
        );
        assert_eq!(status, kAudioHardwareIllegalOperationError as OSStatus);
    }
}

#[test]
fn device_set_unknown_property_returns_error() {
    unsafe {
        let value: u32 = 0;
        let status = device::set_property_data(
            &addr(kAudioObjectPropertyName), // Not settable
            4,
            &value as *const _ as *const c_void,
        );
        assert_eq!(status, kAudioHardwareUnknownPropertyError as OSStatus);
    }
}

#[test]
fn device_set_buffer_size_rejects_too_small_data() {
    unsafe {
        let value: u8 = 0;
        let status = device::set_property_data(
            &addr(kAudioDevicePropertyBufferFrameSize),
            1, // Too small — needs 4 bytes
            &value as *const _ as *const c_void,
        );
        assert_eq!(status, kAudioHardwareBadPropertySizeError as OSStatus);
    }
}

// ===========================================================================
// Data size mismatch tests
// ===========================================================================

#[test]
fn device_rejects_too_small_buffer() {
    unsafe {
        let mut value: u32 = 0;
        let mut out_size: u32 = 0;
        let status = device::get_property_data(
            &addr(kAudioDevicePropertyNominalSampleRate),
            4, // Too small — needs 8 bytes for f64
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareBadPropertySizeError as OSStatus);
    }
}

#[test]
fn plugin_rejects_too_small_buffer() {
    unsafe {
        let mut value: u8 = 0;
        let mut out_size: u32 = 0;
        let status = plugin::get_property_data(
            &addr(kAudioObjectPropertyBaseClass),
            1, // Too small — needs 4 bytes
            &mut out_size,
            &mut value as *mut _ as *mut c_void,
        );
        assert_eq!(status, kAudioHardwareBadPropertySizeError as OSStatus);
    }
}

// ===========================================================================
// UUID comparison tests
// ===========================================================================

#[test]
fn uuid_bytes_equal_works() {
    use crate::types::*;
    assert!(uuid_bytes_equal(
        &KAUDIO_SERVER_PLUGIN_TYPE_UUID_BYTES,
        &KAUDIO_SERVER_PLUGIN_TYPE_UUID_BYTES
    ));
    assert!(!uuid_bytes_equal(
        &KAUDIO_SERVER_PLUGIN_TYPE_UUID_BYTES,
        &IUNKNOWN_UUID_BYTES
    ));
}

// ===========================================================================
// Ring Buffer Tests (positional API)
// ===========================================================================

use crate::ring_buffer::RingBuffer;

/// Frames-to-samples helper for the positional tests. The ring buffer stores
/// interleaved stereo `f32`s (CHANNEL_COUNT = 2), so N frames → 2N samples.
const CH: usize = constants::CHANNEL_COUNT as usize;

#[test]
fn positional_roundtrip_returns_written_samples() {
    // 256 frames of capacity, write/read 128 frames (256 samples) at sample
    // time 0 — roundtrip must be bit-exact.
    let rb = RingBuffer::new(256);
    let input: Vec<f32> = (0..128 * CH).map(|i| i as f32).collect();
    let mut output = vec![0.0f32; 128 * CH];

    rb.write_at(0, &input);
    rb.read_at(0, &mut output);

    assert_eq!(input, output);
}

#[test]
fn positional_read_does_not_consume() {
    // Positional reads are non-destructive — reading the same slot twice
    // returns the same data. This is the fundamental departure from the old
    // FIFO semantic and is the reason latency converges to zero.
    let rb = RingBuffer::new(256);
    let input: Vec<f32> = (0..64 * CH).map(|i| i as f32 + 1.0).collect();

    rb.write_at(0, &input);

    let mut out1 = vec![0.0f32; 64 * CH];
    let mut out2 = vec![0.0f32; 64 * CH];
    rb.read_at(0, &mut out1);
    rb.read_at(0, &mut out2);

    assert_eq!(input, out1);
    assert_eq!(input, out2);
}

#[test]
fn positional_wrap_at_capacity_boundary() {
    // Capacity = 256 frames. Write 512-sample (256-frame) block starting at
    // frame index (256 - 100) = 156 → 100 frames before end, 156 frames after
    // wrap. Tests the two-chunk `copy_nonoverlapping` split logic in both
    // `write_at` and `read_at`.
    let rb = RingBuffer::new(256);
    let start_frame = 256 - 100; // 156
    let input: Vec<f32> = (0..256 * CH).map(|i| i as f32 + 1.0).collect();

    rb.write_at(start_frame as u64, &input);

    let mut out = vec![0.0f32; 256 * CH];
    rb.read_at(start_frame as u64, &mut out);

    assert_eq!(input, out, "Data must survive the wrap split");
}

#[test]
fn positional_overwrite_at_n_plus_capacity() {
    // `write_at(T + capacity_frames, ...)` must land on the same slot as
    // `write_at(T, ...)` — both sample times map to the same frame index
    // under the wrap. A subsequent `read_at(T, ...)` returns the later data.
    let rb = RingBuffer::new(256);
    let data_a: Vec<f32> = (0..64 * CH).map(|i| i as f32).collect();
    let data_b: Vec<f32> = (0..64 * CH).map(|i| (i + 10_000) as f32).collect();

    rb.write_at(0, &data_a);
    rb.write_at(256, &data_b); // 256 is exactly one wrap → same slot as 0

    let mut out = vec![0.0f32; 64 * CH];
    rb.read_at(0, &mut out);

    assert_eq!(out, data_b, "Later write must have overwritten the earlier one");
}

#[test]
fn positional_underrun_returns_zeros_at_fresh_slot() {
    // `reset()` zero-fills storage, so any read of a never-written slot
    // returns silence.
    let rb = RingBuffer::new(256);
    rb.reset();
    let mut out = vec![99.0f32; 128 * CH];
    rb.read_at(0, &mut out);
    assert!(out.iter().all(|&s| s == 0.0), "Unwritten slot must be silence");
}

#[test]
fn positional_underrun_distant_slot_returns_zeros() {
    // Writing at one slot must not bleed into other slots.
    let rb = RingBuffer::new(256);
    rb.reset();
    let written: Vec<f32> = vec![1.0f32; 32 * CH];
    rb.write_at(0, &written);

    let mut out = vec![99.0f32; 32 * CH];
    rb.read_at(128, &mut out); // distant, unwritten slot
    assert!(out.iter().all(|&s| s == 0.0), "Distant unwritten slot must be silence");
}

#[test]
fn positional_reset_zero_fills_storage() {
    // After a write, `reset()` must zero the storage so subsequent reads at
    // the same slot return silence.
    let rb = RingBuffer::new(256);
    let data: Vec<f32> = vec![0.5f32; 64 * CH];
    rb.write_at(0, &data);

    rb.reset();

    let mut out = vec![99.0f32; 64 * CH];
    rb.read_at(0, &mut out);
    assert!(out.iter().all(|&s| s == 0.0), "reset() must zero all storage");
}

#[test]
fn positional_capacity_at_default_config_is_65536_frames_and_power_of_two() {
    // Mirrors the formula in `DriverState::init_audio`. The realised capacity
    // must equal `RING_BUFFER_CAPACITY_MULTIPLIER × ZERO_TIMESTAMP_PERIOD`
    // frames (= 65_536 at default settings), and be a power of two.
    let requested_frames =
        constants::RING_BUFFER_CAPACITY_MULTIPLIER * constants::ZERO_TIMESTAMP_PERIOD as usize;
    assert_eq!(requested_frames, 65_536);
    assert!(requested_frames.is_power_of_two());

    let rb = RingBuffer::new(requested_frames);
    assert_eq!(rb.capacity_frames(), 65_536);
}

#[test]
fn positional_capacity_rounds_up_to_power_of_two() {
    // Requesting 300 frames rounds up to 512.
    let rb = RingBuffer::new(300);
    assert_eq!(rb.capacity_frames(), 512);
    // Sanity: a full-capacity roundtrip at offset 0 must work.
    let input: Vec<f32> = (0..512 * CH).map(|i| i as f32).collect();
    let mut out = vec![0.0f32; 512 * CH];
    rb.write_at(0, &input);
    rb.read_at(0, &mut out);
    assert_eq!(out, input);
}

#[test]
fn positional_loopback_latency_contract() {
    // The reason we did all this: at steady state, if the writer places a
    // buffer at sample_time T and the reader immediately reads at the same
    // sample_time T (as CoreAudio does when safety offsets are 0), the reader
    // gets exactly what the writer just wrote — zero FIFO depth, zero
    // latency.
    let rb = RingBuffer::new(1024);
    for cycle in 0..256u64 {
        let t = cycle * 128; // 128-frame IO cycles
        let input: Vec<f32> = (0..128 * CH).map(|i| (t as usize + i) as f32).collect();
        rb.write_at(t, &input);

        let mut out = vec![0.0f32; 128 * CH];
        rb.read_at(t, &mut out);
        assert_eq!(out, input, "Latency contract broken at cycle {cycle}");
    }
}

#[test]
fn positional_concurrent_writer_reader() {
    // Writer and reader run in separate threads. Writer advances `t_out` and
    // writes; reader follows with `t_in = t_out - delta` and checks that the
    // data it reads is either (a) zeros (writer not yet reached this slot) or
    // (b) exactly what the writer placed there. No torn samples, no NaN/Inf.
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
    use std::sync::{Arc, Barrier};
    use std::thread;

    const FRAMES_PER_CYCLE: usize = 256;
    const NUM_CYCLES: u64 = 5_000;

    // Capacity 4096 frames = 16 × FRAMES_PER_CYCLE. The writer is capped to
    // stay at most `MAX_LEAD_CYCLES` ahead of the reader so we never lap —
    // this mirrors CoreAudio's safety-offset contract where `t_out - t_in` is
    // bounded and the writer never overwrites a slot the reader still owns.
    const MAX_LEAD_CYCLES: u64 = 4;
    let rb = Arc::new(RingBuffer::new(4096));
    rb.reset();
    let writer_cycle = Arc::new(AtomicU64::new(0));
    let reader_cycle = Arc::new(AtomicU64::new(0));
    let barrier = Arc::new(Barrier::new(2));

    let rb_w = Arc::clone(&rb);
    let wc_w = Arc::clone(&writer_cycle);
    let rc_w = Arc::clone(&reader_cycle);
    let b_w = Arc::clone(&barrier);
    let writer = thread::spawn(move || {
        b_w.wait();
        for cycle in 0..NUM_CYCLES {
            // Don't run more than MAX_LEAD_CYCLES ahead of the reader.
            while cycle >= rc_w.load(AtomicOrdering::Acquire) + MAX_LEAD_CYCLES {
                std::hint::spin_loop();
            }
            let t = cycle * FRAMES_PER_CYCLE as u64;
            let data: Vec<f32> = (0..FRAMES_PER_CYCLE * CH)
                .map(|s| (t as usize + s + 1) as f32) // +1 so zero ≠ valid data
                .collect();
            rb_w.write_at(t, &data);
            wc_w.store(cycle + 1, AtomicOrdering::Release);
        }
    });

    let rb_r = Arc::clone(&rb);
    let wc_r = Arc::clone(&writer_cycle);
    let rc_r = Arc::clone(&reader_cycle);
    let b_r = Arc::clone(&barrier);
    let reader = thread::spawn(move || {
        b_r.wait();
        let mut out = vec![0.0f32; FRAMES_PER_CYCLE * CH];
        let mut matched_any_nonzero = false;
        for cycle in 0..NUM_CYCLES {
            // Wait for writer to finish this cycle.
            while wc_r.load(AtomicOrdering::Acquire) <= cycle {
                std::hint::spin_loop();
            }
            let t = cycle * FRAMES_PER_CYCLE as u64;
            rb_r.read_at(t, &mut out);
            for (i, &val) in out.iter().enumerate() {
                let expected = (t as usize + i + 1) as f32;
                assert!(val.is_finite(), "Non-finite at cycle {cycle} offset {i}: {val}");
                assert_eq!(val, expected, "Mismatch at cycle {cycle} offset {i}");
                if val != 0.0 {
                    matched_any_nonzero = true;
                }
            }
            rc_r.store(cycle + 1, AtomicOrdering::Release);
        }
        assert!(matched_any_nonzero, "Reader never observed any non-zero data");
    });

    writer.join().expect("Writer thread panicked");
    reader.join().expect("Reader thread panicked");
}

// ===========================================================================
// DoIOOperation C-ABI end-to-end tests
// ===========================================================================
//
// These tests call `driver_DoIOOperation` directly through its
// `unsafe extern "C" fn` signature, synthesising `AudioServerPlugInIOCycleInfo`
// values via `mem::zeroed`. They pin the contract that CoreAudio relies on:
// WriteMix/ReadInput dispatch by (stream_id, operation_id) and dereference
// `mOutputTime.mSampleTime` / `mInputTime.mSampleTime` from the cycle info to
// address the positional ring buffer.
//
// Shared-state invariant
// ----------------------
// `DRIVER_STATE` is a single static initialised once per test process. Every
// test in this section MUST call `setup()` at entry to zero-fill the ring
// buffer via `crate::test_reset_ring_buffer()`. As defence in depth each
// test also uses a disjoint `sample_time` base (T = 1e6, 2e6, 3e6, 4e6) so
// that any accidental leakage across tests would fail loudly rather than
// alias into a valid-looking slot.

use std::sync::{Mutex, Once};

static INIT_DRIVER: Once = Once::new();

/// Serialises the `DoIOOperation` e2e tests. `cargo test` runs tests in
/// parallel by default, and every test in this section mutates the single
/// global `DRIVER_STATE.ring_buffer`. Each test acquires this lock for its
/// entire duration so `reset()` plus the subsequent reads/writes are not
/// interleaved with another test's writes.
static E2E_LOCK: Mutex<()> = Mutex::new(());

/// Ensure `driver_Initialize` has been called exactly once for this process.
/// `driver_Initialize` itself is internally `Once`-guarded via
/// `DRIVER_STATE.init_once`, so this is double-protection — the outer `Once`
/// avoids even entering the extern "C" call across multiple test threads.
fn ensure_initialised() {
    INIT_DRIVER.call_once(|| {
        // SAFETY: `driver_Initialize` accepts a null host — the test code path
        // only stores the host pointer and runs `init_audio()`. None of the
        // IO entry points we exercise below dereferences the host.
        let status = unsafe {
            crate::driver_Initialize(std::ptr::null_mut(), std::ptr::null_mut())
        };
        assert_eq!(status, kAudioHardwareNoError as OSStatus);
    });
}

/// Initialise the driver (once), acquire the e2e serialisation lock, and
/// zero-fill the ring buffer. Must be called at the top of every
/// `DoIOOperation` e2e test. Returns the lock guard; drop it at end of test.
fn setup() -> std::sync::MutexGuard<'static, ()> {
    ensure_initialised();
    let guard = E2E_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    crate::test_reset_ring_buffer();
    guard
}

/// Call `driver_DoIOOperation` with `WriteMix` on the output stream at the
/// given `sample_time`, passing `samples` as the main buffer. `samples.len()`
/// must be `frames * CHANNEL_COUNT`.
fn do_write_mix(sample_time: u64, samples: &[f32]) {
    assert!(samples.len().is_multiple_of(CH), "samples must be a multiple of CHANNEL_COUNT");
    let frames = (samples.len() / CH) as u32;

    // SAFETY: All fields in `AudioServerPlugInIOCycleInfo` are plain `repr(C)`
    // POD (Float64 / UInt64 / UInt32 / AudioSMPTETime). `mem::zeroed` produces
    // a valid bit pattern; we then set only the field the production code
    // reads (`mOutputTime.mSampleTime`).
    let mut cycle_info: AudioServerPlugInIOCycleInfo = unsafe { mem::zeroed() };
    cycle_info.mOutputTime.mSampleTime = sample_time as f64;

    // SAFETY: `driver_DoIOOperation` reads at most `frames * CHANNEL_COUNT`
    // f32s from `io_main_buffer`. The slice lives for the full call.
    let status = unsafe {
        crate::driver_DoIOOperation(
            std::ptr::null_mut(),
            constants::DEVICE_OBJECT_ID,
            constants::OUTPUT_STREAM_OBJECT_ID,
            0,
            kAudioServerPlugInIOOperationWriteMix,
            frames,
            &cycle_info as *const _,
            samples.as_ptr() as *mut c_void,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(status, kAudioHardwareNoError as OSStatus, "WriteMix returned non-zero status");
}

/// Call `driver_DoIOOperation` with `ReadInput` on the input stream at the
/// given `sample_time`, filling `out`. `out.len()` must be `frames * CHANNEL_COUNT`.
fn do_read_input(sample_time: u64, out: &mut [f32]) {
    assert!(out.len().is_multiple_of(CH), "out must be a multiple of CHANNEL_COUNT");
    let frames = (out.len() / CH) as u32;

    // SAFETY: See `do_write_mix` — same zero-init pattern; we set the field
    // that `ReadInput` reads (`mInputTime.mSampleTime`).
    let mut cycle_info: AudioServerPlugInIOCycleInfo = unsafe { mem::zeroed() };
    cycle_info.mInputTime.mSampleTime = sample_time as f64;

    let status = unsafe {
        crate::driver_DoIOOperation(
            std::ptr::null_mut(),
            constants::DEVICE_OBJECT_ID,
            constants::INPUT_STREAM_OBJECT_ID,
            0,
            kAudioServerPlugInIOOperationReadInput,
            frames,
            &cycle_info as *const _,
            out.as_mut_ptr() as *mut c_void,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(status, kAudioHardwareNoError as OSStatus, "ReadInput returned non-zero status");
}

#[test]
fn do_io_operation_roundtrip_at_same_sample_time() {
    // Bit-exact roundtrip via the C ABI: WriteMix at T, ReadInput at T,
    // assert the read buffer equals the written buffer byte-for-byte.
    let _guard = setup();
    const T: u64 = 1_000_000;
    const FRAMES: usize = 256;

    let input: Vec<f32> = (0..FRAMES * CH).map(|i| (i as f32) + 1.5).collect();
    do_write_mix(T, &input);

    let mut out = vec![0.0f32; FRAMES * CH];
    do_read_input(T, &mut out);

    assert_eq!(out, input, "WriteMix → ReadInput at the same sample_time must be bit-exact");
}

#[test]
fn do_io_operation_zero_lag_steady_state() {
    // At steady state with `t_out == t_in`, every ReadInput must return the
    // samples the matching WriteMix just placed — no accumulated FIFO depth.
    // Run 32 cycles at each of frames ∈ {128, 256, 512}.
    let _guard = setup();
    const T_BASE: u64 = 2_000_000;
    const NUM_CYCLES: u64 = 32;

    for &frames_per_cycle in &[128usize, 256, 512] {
        // Fresh base per block so an off-by-one in one block doesn't alias
        // into another block's slots.
        crate::test_reset_ring_buffer();

        for cycle in 0..NUM_CYCLES {
            let t = T_BASE + cycle * frames_per_cycle as u64;
            let input: Vec<f32> = (0..frames_per_cycle * CH)
                .map(|i| (cycle as usize * frames_per_cycle * CH + i + 1) as f32)
                .collect();
            do_write_mix(t, &input);

            let mut out = vec![0.0f32; frames_per_cycle * CH];
            do_read_input(t, &mut out);

            assert_eq!(
                out, input,
                "Zero-lag contract broken at frames={frames_per_cycle}, cycle={cycle}"
            );
        }
    }
}

#[test]
fn do_io_operation_safety_offset_contract() {
    // When `t_out = t_in + k × frames_per_cycle`, the read at cycle c returns
    // the data written at cycle c - k (and zeros for c < k). This is the
    // exact behaviour CoreAudio relies on when it schedules IO with a
    // positive safety offset between output and input.
    let _guard = setup();
    const T_BASE: u64 = 3_000_000;
    const FRAMES_PER_CYCLE: usize = 256;
    const NUM_CYCLES: usize = 16;
    const K: u64 = 2;

    // Pre-compute expected write payloads so the reader can reconstruct what
    // cycle c - k wrote.
    let writes: Vec<Vec<f32>> = (0..NUM_CYCLES)
        .map(|cycle| {
            (0..FRAMES_PER_CYCLE * CH)
                .map(|i| (cycle * FRAMES_PER_CYCLE * CH + i + 1) as f32)
                .collect()
        })
        .collect();

    for cycle in 0..NUM_CYCLES as u64 {
        let t_out = T_BASE + (cycle + K) * FRAMES_PER_CYCLE as u64;
        let t_in = T_BASE + cycle * FRAMES_PER_CYCLE as u64;

        do_write_mix(t_out, &writes[cycle as usize]);

        let mut out = vec![0.0f32; FRAMES_PER_CYCLE * CH];
        do_read_input(t_in, &mut out);

        if cycle < K {
            // No prior write landed on this slot — must be silence.
            assert!(
                out.iter().all(|&s| s == 0.0),
                "Pre-warmup read at cycle {cycle} must be zeros"
            );
        } else {
            let expected = &writes[(cycle - K) as usize];
            assert_eq!(
                &out, expected,
                "Safety-offset contract broken at cycle {cycle}: read should match write from cycle {}",
                cycle - K
            );
        }
    }
}

#[test]
fn do_io_operation_wraps_across_capacity_boundary() {
    // Run enough cycles that the positional indexing wraps through the full
    // ring-buffer storage twice. With capacity = 65_536 frames and
    // frames_per_cycle = 256, `2 × 65_536 / 256 = 512` cycles is the target.
    // Each cycle has `t_in == t_out`, so every read must match its write.
    let _guard = setup();
    const T_BASE: u64 = 4_000_000;
    const FRAMES_PER_CYCLE: usize = 256;
    // Matches the production capacity (RING_BUFFER_CAPACITY_MULTIPLIER × ZERO_TIMESTAMP_PERIOD).
    const CAPACITY_FRAMES: usize = 65_536;
    const NUM_CYCLES: u64 = (2 * CAPACITY_FRAMES / FRAMES_PER_CYCLE) as u64; // 512

    for cycle in 0..NUM_CYCLES {
        let t = T_BASE + cycle * FRAMES_PER_CYCLE as u64;
        // Encode cycle index in the payload so a stale-slot read fails loudly.
        let input: Vec<f32> = (0..FRAMES_PER_CYCLE * CH)
            .map(|i| (cycle as usize * FRAMES_PER_CYCLE * CH + i + 1) as f32)
            .collect();
        do_write_mix(t, &input);

        let mut out = vec![0.0f32; FRAMES_PER_CYCLE * CH];
        do_read_input(t, &mut out);

        assert_eq!(out, input, "Wrap-around stress broken at cycle {cycle}");
    }
}

// ===========================================================================
// Virtual Clock Tests
// ===========================================================================

use crate::timing::VirtualClock;

#[test]
fn clock_timestamps_are_period_aligned() {
    let clock = VirtualClock::new();
    let now = VirtualClock::now();
    clock.anchor(now, 48_000.0);

    // Wait a tiny bit so elapsed > 0
    std::thread::sleep(std::time::Duration::from_millis(5));

    let period = constants::ZERO_TIMESTAMP_PERIOD;

    let (sample_time, _host_time, _seed) = clock.get_zero_timestamp(period);

    // sample_time must be a multiple of period
    let remainder = sample_time % (period as f64);
    assert_eq!(remainder, 0.0, "sample_time {sample_time} not a multiple of period {period}");
}

#[test]
fn clock_timestamps_monotonically_non_decreasing() {
    let clock = VirtualClock::new();
    let now = VirtualClock::now();
    clock.anchor(now, 48_000.0);

    let period = constants::ZERO_TIMESTAMP_PERIOD;

    let mut prev_sample_time = 0.0f64;
    let mut prev_host_time = 0u64;

    for _ in 0..100 {
        let (sample_time, host_time, _seed) = clock.get_zero_timestamp(period);
        assert!(
            sample_time >= prev_sample_time,
            "sample_time went backwards: {prev_sample_time} -> {sample_time}"
        );
        assert!(
            host_time >= prev_host_time,
            "host_time went backwards: {prev_host_time} -> {host_time}"
        );
        prev_sample_time = sample_time;
        prev_host_time = host_time;
    }
}

#[test]
fn clock_seed_constant_during_stable_operation() {
    let clock = VirtualClock::new();
    let now = VirtualClock::now();
    clock.anchor(now, 48_000.0);

    let period = constants::ZERO_TIMESTAMP_PERIOD;

    let (_, _, seed1) = clock.get_zero_timestamp(period);
    let (_, _, seed2) = clock.get_zero_timestamp(period);
    let (_, _, seed3) = clock.get_zero_timestamp(period);

    assert_eq!(seed1, seed2);
    assert_eq!(seed2, seed3);
}

#[test]
fn clock_seed_increments_on_reset() {
    let clock = VirtualClock::new();
    let now = VirtualClock::now();
    clock.anchor(now, 48_000.0);

    let initial_seed = clock.seed();

    clock.increment_seed();
    assert_eq!(clock.seed(), initial_seed + 1);

    clock.increment_seed();
    assert_eq!(clock.seed(), initial_seed + 2);
}

#[test]
fn clock_initial_seed_is_one() {
    let clock = VirtualClock::new();
    assert_eq!(clock.seed(), 1);
}

#[test]
fn clock_unanchored_returns_zero_sample_time() {
    let clock = VirtualClock::new();
    let (sample_time, _host_time, _seed) = clock.get_zero_timestamp(48_000);
    assert_eq!(sample_time, 0.0);
}
