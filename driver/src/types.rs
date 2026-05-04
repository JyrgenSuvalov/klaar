//! Supplemental FFI types and constants for the AudioServerPlugin API.
//!
//! coreaudio-sys provides all struct definitions. This module adds:
//! - COM HRESULT constants
//! - UUID byte definitions for plugin type/interface matching
//! - Helper for comparing CFUUIDBytes

use coreaudio_sys::{CFUUIDBytes, HRESULT};

// COM HRESULT values
pub const S_OK: HRESULT = 0;
pub const E_NOINTERFACE: HRESULT = 0x80000004_u32 as HRESULT;

// kAudioServerPlugInTypeUUID: 443ABAB8-E7B3-491A-B985-BEB9187030DB
pub const KAUDIO_SERVER_PLUGIN_TYPE_UUID_BYTES: CFUUIDBytes = CFUUIDBytes {
    byte0: 0x44, byte1: 0x3A, byte2: 0xBA, byte3: 0xB8,
    byte4: 0xE7, byte5: 0xB3, byte6: 0x49, byte7: 0x1A,
    byte8: 0xB9, byte9: 0x85, byte10: 0xBE, byte11: 0xB9,
    byte12: 0x18, byte13: 0x70, byte14: 0x30, byte15: 0xDB,
};

// kAudioServerPlugInDriverInterfaceUUID: EEA5773D-CC43-49F1-8E00-8F96E7D23B17
// From AudioServerPlugIn.h: CFUUIDGetConstantUUIDWithBytes(NULL, 0xEE, 0xA5, 0x77, 0x3D, ...)
pub const KAUDIO_SERVER_PLUGIN_DRIVER_INTERFACE_UUID_BYTES: CFUUIDBytes = CFUUIDBytes {
    byte0: 0xEE, byte1: 0xA5, byte2: 0x77, byte3: 0x3D,
    byte4: 0xCC, byte5: 0x43, byte6: 0x49, byte7: 0xF1,
    byte8: 0x8E, byte9: 0x00, byte10: 0x8F, byte11: 0x96,
    byte12: 0xE7, byte13: 0xD2, byte14: 0x3B, byte15: 0x17,
};

// IUnknown UUID: 00000000-0000-0000-C000-000000000046
pub const IUNKNOWN_UUID_BYTES: CFUUIDBytes = CFUUIDBytes {
    byte0: 0x00, byte1: 0x00, byte2: 0x00, byte3: 0x00,
    byte4: 0x00, byte5: 0x00, byte6: 0x00, byte7: 0x00,
    byte8: 0xC0, byte9: 0x00, byte10: 0x00, byte11: 0x00,
    byte12: 0x00, byte13: 0x00, byte14: 0x00, byte15: 0x46,
};

/// Create a CFStringRef from a Rust string. The caller (coreaudiod) owns the reference.
pub fn cfstring_create(s: &str) -> coreaudio_sys::CFStringRef {
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;
    let cf = CFString::new(s);
    let ptr = cf.as_concrete_TypeRef();
    std::mem::forget(cf); // Transfer ownership to the caller
    ptr as coreaudio_sys::CFStringRef
}

/// Compare two CFUUIDBytes for equality.
#[inline]
pub fn uuid_bytes_equal(a: &CFUUIDBytes, b: &CFUUIDBytes) -> bool {
    a.byte0 == b.byte0 && a.byte1 == b.byte1 && a.byte2 == b.byte2 && a.byte3 == b.byte3
        && a.byte4 == b.byte4 && a.byte5 == b.byte5 && a.byte6 == b.byte6 && a.byte7 == b.byte7
        && a.byte8 == b.byte8 && a.byte9 == b.byte9 && a.byte10 == b.byte10 && a.byte11 == b.byte11
        && a.byte12 == b.byte12 && a.byte13 == b.byte13 && a.byte14 == b.byte14 && a.byte15 == b.byte15
}
