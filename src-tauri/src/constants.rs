//! Application-wide constants.

/// Minimum driver `CFBundleVersion` the app requires.
///
/// **Bump policy:** Only bump this when the driver changes its ABI (device
/// UID, ring-buffer layout, property table) in a way the app depends on.
/// Do **NOT** bump for cosmetic driver changes — every bump forces existing
/// users through an admin-prompted reinstall.
///
/// Compared against `get_installed_driver_version()` using `util::semver`.
/// Installed `<` MIN → user is prompted to update.
/// Installed `>=` MIN → accepted silently, even if newer than the shipped driver.
pub const MIN_DRIVER_VERSION: &str = "1.0.0";
