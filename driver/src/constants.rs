/// Number of audio channels (stereo)
pub const CHANNEL_COUNT: u32 = 2;

/// Device name shown in macOS audio device pickers
pub const DEVICE_NAME: &str = "Klaar";

/// Persistent device UID (must not change between versions)
pub const DEVICE_UID: &str = "KlaarVirtualMic_UID";

/// Model UID
pub const MODEL_UID: &str = "KlaarVirtualMic_ModelUID";

/// Manufacturer name
pub const MANUFACTURER: &str = "Klaar";

/// Supported sample rates
pub const SUPPORTED_SAMPLE_RATES: &[f64] = &[44_100.0, 48_000.0];

/// Default sample rate
pub const DEFAULT_SAMPLE_RATE: f64 = 48_000.0;

/// Default buffer frame size
pub const DEFAULT_BUFFER_FRAME_SIZE: u32 = 512;

/// Min/max buffer frame sizes
pub const MIN_BUFFER_FRAME_SIZE: u32 = 2;
pub const MAX_BUFFER_FRAME_SIZE: u32 = 8192;

/// Ring buffer capacity multiplier (× ZERO_TIMESTAMP_PERIOD *frames*).
///
/// The realised capacity is `RING_BUFFER_CAPACITY_MULTIPLIER *
/// ZERO_TIMESTAMP_PERIOD` frames (= 65_536 frames at default settings), each
/// frame holding `CHANNEL_COUNT` interleaved `f32` samples (total allocation
/// 131_072 `f32`s = 512 KiB).
///
/// The 4× factor provides headroom for timing jitter between the WriteMix
/// and ReadInput IO cycles while keeping the allocated capacity small enough
/// to fit comfortably in cache.
pub const RING_BUFFER_CAPACITY_MULTIPLIER: usize = 4;

/// Zero timestamp period (frames).
///
/// CoreAudio uses this period to schedule IO cycles and reason about clock
/// drift; an over-long period causes the host to add conservative guard
/// buffering, which manifested as several hundred ms of perceptible latency
/// in Zoom prior to fix-driver-latency.
pub const ZERO_TIMESTAMP_PERIOD: u32 = 16_384;

/// Plugin bundle identifier
#[allow(dead_code)] // Used in Info.plist / bundle metadata
pub const BUNDLE_ID: &str = "com.klaar.driver";

/// App bundle identifier (for kAudioDevicePropertyConfigurationApplication)
pub const APP_BUNDLE_ID: &str = "com.klaar.app";

// --- Object IDs ---
// CoreAudio requires a specific object tree with fixed IDs.

/// Plugin object ID
pub const PLUGIN_OBJECT_ID: u32 = 1; // kAudioObjectPlugInObject

/// Device object ID
pub const DEVICE_OBJECT_ID: u32 = 2;

/// Input stream object ID (what Zoom reads)
pub const INPUT_STREAM_OBJECT_ID: u32 = 3;

/// Output stream object ID (what Klaar writes)
pub const OUTPUT_STREAM_OBJECT_ID: u32 = 4;

// --- Info.plist factory UUID ---
/// Must match the UUID in Info.plist CFPlugInFactories
#[allow(dead_code)] // Referenced by Info.plist, not Rust code
pub const FACTORY_UUID: &str = "7B9CFF5D-6BD2-4CB1-B84F-DA58F4D92BCA";

// --- Configuration change action constants ---

/// Action: pending sample rate change
pub const ACTION_SET_SAMPLE_RATE: u64 = 1;

/// Action: pending buffer frame size change
pub const ACTION_SET_BUFFER_SIZE: u64 = 2;

/// Input stream name
pub const INPUT_STREAM_NAME: &str = "Klaar Input Stream";

/// Output stream name
pub const OUTPUT_STREAM_NAME: &str = "Klaar Output Stream";
