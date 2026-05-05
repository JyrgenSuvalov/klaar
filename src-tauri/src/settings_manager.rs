/// Application settings persistence — device IDs, active profile, processing state, auto-launch.
///
/// Settings are stored in `~/Library/Application Support/eu.jyrkki.Klaar/settings.json` and
/// auto-saved on every change.
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

// ────────────────────────────────────────────────────────────────────────────
// AppSettings data model
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default)]
    pub input_device_id: Option<String>,
    #[serde(default)]
    pub output_device_id: Option<String>,
    #[serde(default)]
    pub active_profile_id: Option<String>,
    #[serde(default)]
    pub auto_launch: bool,
    #[serde(default = "default_true")]
    pub processing_enabled: bool,
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
    #[serde(default = "default_buffer_size")]
    pub buffer_size: u32,
    /// Migration counter for the LaunchAgent `ProgramArguments` we expect
    /// our installed plist to carry. Compared against
    /// `EXPECTED_LAUNCH_AGENT_ARGS_VERSION` (defined alongside the
    /// autostart-plugin init in `lib.rs`) on startup; when it lags, we
    /// re-register the LaunchAgent once and persist the new value.
    ///
    /// `0` (default for users who upgraded from a pre-migration build)
    /// means "no marker — assume the plist needs to be rewritten if
    /// auto-launch is currently enabled".
    #[serde(default)]
    pub launch_agent_args_version: u32,
}

fn default_true() -> bool { true }
fn default_sample_rate() -> u32 { 48000 }
fn default_buffer_size() -> u32 { 256 }

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            input_device_id: None,
            output_device_id: None,
            active_profile_id: None,
            auto_launch: false,
            processing_enabled: true,
            sample_rate: 48000,
            buffer_size: 256,
            launch_agent_args_version: 0,
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// SettingsManager
// ────────────────────────────────────────────────────────────────────────────

/// Manages reading and writing `AppSettings` to disk.
///
/// **Mutex pattern note:** `SettingsManager` owns its `Mutex<AppSettings>` internally
/// and is registered with Tauri as `Arc<SettingsManager>` (not `Mutex<SettingsManager>`).
/// This differs from `ProfileManager`, which is registered as `Mutex<ProfileManager>`.
/// Both approaches are valid; the internal Mutex is used here because `SettingsManager::new()`
/// is infallible and the manager needs to be cheaply clonable/shareable without an outer lock.
pub struct SettingsManager {
    file_path: PathBuf,
    settings: Mutex<AppSettings>,
}

impl SettingsManager {
    /// Create a new SettingsManager, loading settings from disk (or defaults).
    pub fn new(app_data_dir: &std::path::Path) -> Self {
        // Ensure data directory exists
        let _ = fs::create_dir_all(app_data_dir);
        let file_path = app_data_dir.join("settings.json");
        let settings = Self::load_from_disk(&file_path);
        Self {
            file_path,
            settings: Mutex::new(settings),
        }
    }

    /// Get a clone of the current settings.
    pub fn get(&self) -> AppSettings {
        self.settings.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Update settings via a closure, then auto-save to disk.
    pub fn update<F>(&self, f: F) -> Result<(), String>
    where
        F: FnOnce(&mut AppSettings),
    {
        let mut settings = self.settings.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut settings);
        Self::save_to_disk(&self.file_path, &settings)
    }

    /// Force a save of the current settings to disk.
    pub fn save(&self) -> Result<(), String> {
        let settings = self.settings.lock().unwrap_or_else(|e| e.into_inner());
        Self::save_to_disk(&self.file_path, &settings)
    }

    // ── Internal helpers ────────────────────────────────────────────────────

    fn load_from_disk(path: &std::path::Path) -> AppSettings {
        match fs::read_to_string(path) {
            Ok(contents) => {
                serde_json::from_str(&contents).unwrap_or_else(|e| {
                    log::warn!("Corrupt settings.json, using defaults: {e}");
                    AppSettings::default()
                })
            }
            Err(_) => {
                log::info!("No settings.json found, using defaults");
                AppSettings::default()
            }
        }
    }

    fn save_to_disk(path: &std::path::Path, settings: &AppSettings) -> Result<(), String> {
        let json = serde_json::to_string_pretty(settings)
            .map_err(|e| format!("Failed to serialize settings: {e}"))?;
        fs::write(path, json)
            .map_err(|e| format!("Failed to write settings.json: {e}"))
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_settings() -> (SettingsManager, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mgr = SettingsManager::new(dir.path());
        (mgr, dir)
    }

    #[test]
    fn defaults_when_no_file() {
        let (mgr, _dir) = temp_settings();
        let s = mgr.get();
        assert!(s.input_device_id.is_none());
        assert!(s.output_device_id.is_none());
        assert!(s.active_profile_id.is_none());
        assert!(!s.auto_launch);
        assert!(s.processing_enabled);
        assert_eq!(s.sample_rate, 48000);
        assert_eq!(s.buffer_size, 256);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();

        // Save some settings
        {
            let mgr = SettingsManager::new(dir.path());
            mgr.update(|s| {
                s.input_device_id = Some("input-uid-123".to_string());
                s.output_device_id = Some("output-uid-456".to_string());
                s.active_profile_id = Some("profile-id-789".to_string());
                s.auto_launch = true;
                s.processing_enabled = false;
            }).unwrap();
        }

        // Load them back
        {
            let mgr = SettingsManager::new(dir.path());
            let s = mgr.get();
            assert_eq!(s.input_device_id.as_deref(), Some("input-uid-123"));
            assert_eq!(s.output_device_id.as_deref(), Some("output-uid-456"));
            assert_eq!(s.active_profile_id.as_deref(), Some("profile-id-789"));
            assert!(s.auto_launch);
            assert!(!s.processing_enabled);
        }
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("settings.json");
        fs::write(&file_path, "not valid json {{{").unwrap();

        let mgr = SettingsManager::new(dir.path());
        let s = mgr.get();
        assert!(s.processing_enabled); // default
        assert_eq!(s.sample_rate, 48000);
    }

    #[test]
    fn partial_json_uses_defaults_for_missing() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("settings.json");
        fs::write(&file_path, r#"{"autoLaunch": true}"#).unwrap();

        let mgr = SettingsManager::new(dir.path());
        let s = mgr.get();
        assert!(s.auto_launch);
        assert!(s.processing_enabled); // default
        assert_eq!(s.sample_rate, 48000); // default
    }

    #[test]
    fn launch_agent_args_version_default_is_zero_and_round_trips() {
        // Default → 0 (the "needs migration" sentinel for pre-migration users).
        let s = AppSettings::default();
        assert_eq!(s.launch_agent_args_version, 0);

        // Round-trip a non-zero value through serde to confirm the field is
        // correctly named/cased on the wire and survives a save+load cycle.
        let dir = tempfile::tempdir().unwrap();
        {
            let mgr = SettingsManager::new(dir.path());
            mgr.update(|s| s.launch_agent_args_version = 1).unwrap();
        }
        {
            let mgr = SettingsManager::new(dir.path());
            assert_eq!(mgr.get().launch_agent_args_version, 1);
        }
    }

    #[test]
    fn missing_launch_agent_args_version_defaults_to_zero() {
        // A settings.json written by a pre-migration build has no
        // `launchAgentArgsVersion` key — the field's `#[serde(default)]`
        // SHALL produce `0` rather than failing the whole load.
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("settings.json");
        fs::write(&file_path, r#"{"autoLaunch": true, "processingEnabled": true}"#).unwrap();
        let mgr = SettingsManager::new(dir.path());
        let s = mgr.get();
        assert_eq!(s.launch_agent_args_version, 0);
        assert!(s.auto_launch);
    }

    #[test]
    fn update_auto_saves() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = SettingsManager::new(dir.path());

        mgr.update(|s| {
            s.auto_launch = true;
        }).unwrap();

        // Verify file was written
        let contents = fs::read_to_string(dir.path().join("settings.json")).unwrap();
        let parsed: AppSettings = serde_json::from_str(&contents).unwrap();
        assert!(parsed.auto_launch);
    }
}
