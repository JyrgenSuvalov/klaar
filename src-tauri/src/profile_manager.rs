/// Profile management — CRUD operations for named presets.
///
/// Each profile is a JSON file in `~/Library/Application Support/eu.jyrkki.Klaar/profiles/`.
/// Profiles store the complete state of all five DSP effects (gate, EQ, de-esser,
/// compressor, limiter) plus bypass states.
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

// ────────────────────────────────────────────────────────────────────────────
// Effect parameter structs (serialisable snapshot of DspParams atomics)
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NoiseGateParams {
    #[serde(default = "default_gate_threshold")]
    pub threshold: f32,
    #[serde(default = "default_gate_attack")]
    pub attack: f32,
    #[serde(default = "default_gate_release")]
    pub release: f32,
    #[serde(default = "default_gate_range")]
    pub range: f32,
    #[serde(default)]
    pub bypassed: bool,
}

fn default_gate_threshold() -> f32 { -80.0 }
fn default_gate_attack() -> f32 { 10.0 }
fn default_gate_release() -> f32 { 100.0 }
fn default_gate_range() -> f32 { -60.0 }

impl Default for NoiseGateParams {
    fn default() -> Self {
        Self {
            threshold: -80.0,
            attack: 10.0,
            release: 100.0,
            range: -60.0,
            bypassed: false,
        }
    }
}

/// Serialisable filter type for profile JSON.
///
/// Uses camelCase string representation in JSON (e.g. `"highPass"`, `"lowShelf"`).
/// Unknown values — from hand-edited profiles or future format changes — fall
/// through to `Unknown`, which maps to `FilterType::Bell` when loaded.
///
/// Note: `BandPass` is intentionally excluded — it is an internal filter type
/// used by the de-esser sidechain and is never stored in user profiles.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum FilterTypeSerde {
    #[default]
    Bell,
    HighPass,
    LowPass,
    HighShelf,
    LowShelf,
    /// 48 dB/oct high-pass (8th-order Butterworth).
    HighPass48,
    /// 48 dB/oct low-pass (8th-order Butterworth).
    LowPass48,
    /// Catch-all for forward-compatibility: any unrecognised string becomes Bell on load.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EqBandData {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub filter_type: FilterTypeSerde,
    #[serde(default = "default_frequency")]
    pub frequency: f32,
    #[serde(default)]
    pub gain: f32,
    #[serde(default = "default_q")]
    pub q: f32,
}

fn default_true() -> bool { true }
fn default_frequency() -> f32 { 1000.0 }
fn default_q() -> f32 { 1.0 }

impl Default for EqBandData {
    fn default() -> Self {
        Self {
            enabled: true,
            filter_type: FilterTypeSerde::Bell,
            frequency: 1000.0,
            gain: 0.0,
            q: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EqParams {
    #[serde(default = "default_eq_bands")]
    pub bands: Vec<EqBandData>,
    #[serde(default)]
    pub bypassed: bool,
}

/// Default EQ band frequencies.
///
/// These match the engine's initial atomic values (80, 200, 500, 1k, 2.5k, 5k, 10k, 16k Hz,
/// all bell type), kept consistent with the audio engine defaults.
fn default_eq_bands() -> Vec<EqBandData> {
    let freqs = [80.0f32, 200.0, 500.0, 1000.0, 2500.0, 5000.0, 10000.0, 16000.0];
    freqs.iter().map(|&f| EqBandData {
        frequency: f,
        ..Default::default()
    }).collect()
}

impl Default for EqParams {
    fn default() -> Self {
        Self {
            bands: default_eq_bands(),
            bypassed: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeEsserParams {
    #[serde(default = "default_deesser_frequency")]
    pub frequency: f32,
    #[serde(default)]
    pub threshold: f32,
    #[serde(default = "default_deesser_ratio")]
    pub ratio: f32,
    #[serde(default = "default_deesser_attack")]
    pub attack_ms: f32,
    #[serde(default = "default_deesser_release")]
    pub release_ms: f32,
    #[serde(default)]
    pub bypassed: bool,
    // NOTE: `reduction` was removed in profile schema v2. Old v1 profiles may
    // still contain a `"reduction": <f32>` field — serde silently drops it
    // (no `deny_unknown_fields`), and `migrate_v1_to_v2` resets ratio/attack/
    // release to their defaults when version < 2. The `listen` flag is never
    // serialised — it is a transient runtime aid (see change
    // `improve-deesser-tweaking`).
}

fn default_deesser_frequency() -> f32 { 6000.0 }
fn default_deesser_ratio() -> f32 { 4.0 }
fn default_deesser_attack() -> f32 { 1.0 }
fn default_deesser_release() -> f32 { 50.0 }

impl Default for DeEsserParams {
    fn default() -> Self {
        Self {
            frequency: 6000.0,
            threshold: 0.0,
            ratio: 4.0,
            attack_ms: 1.0,
            release_ms: 50.0,
            bypassed: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CompressorParams {
    #[serde(default)]
    pub threshold: f32,
    #[serde(default = "default_comp_ratio")]
    pub ratio: f32,
    #[serde(default = "default_comp_attack")]
    pub attack: f32,
    #[serde(default = "default_comp_release")]
    pub release: f32,
    #[serde(default)]
    pub makeup_gain: f32,
    #[serde(default)]
    pub bypassed: bool,
}

fn default_comp_ratio() -> f32 { 1.0 }
fn default_comp_attack() -> f32 { 10.0 }
fn default_comp_release() -> f32 { 100.0 }

impl Default for CompressorParams {
    fn default() -> Self {
        Self {
            threshold: 0.0,
            ratio: 1.0,
            attack: 10.0,
            release: 100.0,
            makeup_gain: 0.0,
            bypassed: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LimiterParams {
    #[serde(default)]
    pub ceiling: f32,
    #[serde(default = "default_limiter_release")]
    pub release: f32,
    #[serde(default)]
    pub bypassed: bool,
}

fn default_limiter_release() -> f32 { 100.0 }

impl Default for LimiterParams {
    fn default() -> Self {
        Self {
            ceiling: 0.0,
            release: 100.0,
            bypassed: false,
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Profile & ProfileSummary
// ────────────────────────────────────────────────────────────────────────────

/// Complete profile — all effect parameters plus metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(default = "default_version")]
    pub version: u32,
    pub created_at: String,
    pub updated_at: String,

    #[serde(default)]
    pub noise_gate: NoiseGateParams,
    #[serde(default)]
    pub eq: EqParams,
    #[serde(default)]
    pub de_esser: DeEsserParams,
    #[serde(default)]
    pub compressor: CompressorParams,
    #[serde(default)]
    pub limiter: LimiterParams,
}

fn default_version() -> u32 { 1 }

/// Current profile schema version. Bumped to 2 by change
/// `improve-deesser-tweaking` (de-esser parameter rework: replaced
/// `reduction` with `ratio`, added `attack`/`release`).
pub const CURRENT_PROFILE_VERSION: u32 = 2;

/// Lightweight summary for listing profiles without loading full data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSummary {
    pub id: String,
    pub name: String,
    pub updated_at: String,
}

impl From<&Profile> for ProfileSummary {
    fn from(p: &Profile) -> Self {
        Self {
            id: p.id.clone(),
            name: p.name.clone(),
            updated_at: p.updated_at.clone(),
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// All-params snapshot (used for batch get/set between engine and profiles)
// ────────────────────────────────────────────────────────────────────────────

/// A complete snapshot of all DSP parameters — used for profile save/load.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AllDspParams {
    pub noise_gate: NoiseGateParams,
    pub eq: EqParams,
    pub de_esser: DeEsserParams,
    pub compressor: CompressorParams,
    pub limiter: LimiterParams,
}

impl Profile {
    /// Extract the DSP parameters portion of this profile.
    pub fn dsp_params(&self) -> AllDspParams {
        AllDspParams {
            noise_gate: self.noise_gate.clone(),
            eq: self.eq.clone(),
            de_esser: self.de_esser.clone(),
            compressor: self.compressor.clone(),
            limiter: self.limiter.clone(),
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Migration
// ────────────────────────────────────────────────────────────────────────────

/// Migrate a profile struct from any older schema to `CURRENT_PROFILE_VERSION`.
///
/// v1 → v2 (change `improve-deesser-tweaking`): the de-esser `reduction` cap
/// (0–20 dB) was replaced by a Compressor-style `ratio` (1.0–10.0). The two
/// formulas do not translate cleanly, so we discard any old reduction value
/// (serde already dropped it on deserialise — there is no field to read) and
/// substitute the defaults for ratio/attack/release. Operators with carefully
/// tuned old profiles will need to re-tune; this is documented in the design.
pub(crate) fn migrate_profile(profile: &mut Profile) {
    if profile.version < 2 {
        log::info!(
            "Migrating profile id={} '{}' from v{} → v2: de-esser ratio/attack/release reset to defaults",
            profile.id,
            profile.name,
            profile.version
        );
        profile.de_esser.ratio = default_deesser_ratio();
        profile.de_esser.attack_ms = default_deesser_attack();
        profile.de_esser.release_ms = default_deesser_release();
        profile.version = 2;
    }
}

// ────────────────────────────────────────────────────────────────────────────
// ProfileManager
// ────────────────────────────────────────────────────────────────────────────

pub struct ProfileManager {
    profiles_dir: PathBuf,
}

impl ProfileManager {
    /// Create a new ProfileManager, ensuring the profiles directory exists.
    pub fn new(app_data_dir: &std::path::Path) -> Result<Self, String> {
        let profiles_dir = app_data_dir.join("profiles");
        fs::create_dir_all(&profiles_dir)
            .map_err(|e| format!("Failed to create profiles directory: {e}"))?;
        Ok(Self { profiles_dir })
    }

    /// List all valid profiles, sorted by updatedAt descending.
    pub fn list(&self) -> Result<Vec<ProfileSummary>, String> {
        let mut summaries = Vec::new();

        let entries = fs::read_dir(&self.profiles_dir)
            .map_err(|e| format!("Failed to read profiles directory: {e}"))?;

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    log::warn!("Skipping unreadable directory entry: {e}");
                    continue;
                }
            };

            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }

            match fs::read_to_string(&path) {
                Ok(contents) => match serde_json::from_str::<Profile>(&contents) {
                    Ok(profile) => summaries.push(ProfileSummary::from(&profile)),
                    Err(e) => {
                        log::warn!("Skipping corrupt profile {}: {e}", path.display());
                    }
                },
                Err(e) => {
                    log::warn!("Failed to read profile file {}: {e}", path.display());
                }
            }
        }

        // Sort by updatedAt descending (most recent first)
        summaries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(summaries)
    }

    /// Save a new profile with the given name and current DSP params.
    pub fn save(&self, name: &str, params: AllDspParams) -> Result<ProfileSummary, String> {
        const MAX_NAME_LEN: usize = 100;

        let name = name.trim();
        if name.is_empty() {
            return Err("Profile name cannot be empty".to_string());
        }
        if name.len() > MAX_NAME_LEN {
            return Err(format!("Profile name must be {MAX_NAME_LEN} characters or fewer"));
        }

        // Reject duplicate names — two profiles with the same name would be
        // indistinguishable in the UI dropdown.
        let existing = self.list()?;
        if existing.iter().any(|p| p.name.eq_ignore_ascii_case(name)) {
            return Err(format!("A profile named \"{name}\" already exists"));
        }

        let now = chrono::Utc::now().to_rfc3339();
        let profile = Profile {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            version: CURRENT_PROFILE_VERSION,
            created_at: now.clone(),
            updated_at: now,
            noise_gate: params.noise_gate,
            eq: params.eq,
            de_esser: params.de_esser,
            compressor: params.compressor,
            limiter: params.limiter,
        };

        self.write_profile(&profile)?;
        Ok(ProfileSummary::from(&profile))
    }

    /// Load a profile by ID.
    ///
    /// Profiles older than `CURRENT_PROFILE_VERSION` are migrated in-memory
    /// before being returned. The on-disk file is NOT rewritten — the user's
    /// next explicit `update_profile`/`save_profile` will write the v2 form.
    pub fn load(&self, id: &str) -> Result<Profile, String> {
        let path = self.profiles_dir.join(format!("{id}.json"));
        let contents = fs::read_to_string(&path)
            .map_err(|_| format!("Profile not found: {id}"))?;
        let mut profile: Profile = serde_json::from_str(&contents)
            .map_err(|e| format!("Failed to parse profile {id}: {e}"))?;

        if profile.version < CURRENT_PROFILE_VERSION {
            migrate_profile(&mut profile);
        }
        Ok(profile)
    }

    /// Update an existing profile with new DSP params (preserves id, name, createdAt).
    pub fn update(&self, id: &str, params: AllDspParams) -> Result<ProfileSummary, String> {
        let mut profile = self.load(id)?;
        let now = chrono::Utc::now().to_rfc3339();

        profile.updated_at = now;
        profile.noise_gate = params.noise_gate;
        profile.eq = params.eq;
        profile.de_esser = params.de_esser;
        profile.compressor = params.compressor;
        profile.limiter = params.limiter;

        self.write_profile(&profile)?;
        Ok(ProfileSummary::from(&profile))
    }

    /// Delete a profile by ID.
    pub fn delete(&self, id: &str) -> Result<(), String> {
        let path = self.profiles_dir.join(format!("{id}.json"));
        if !path.exists() {
            return Err(format!("Profile not found: {id}"));
        }
        fs::remove_file(&path)
            .map_err(|e| format!("Failed to delete profile {id}: {e}"))
    }

    /// Create a default profile if no profiles exist.
    pub fn ensure_default_profile(&self) -> Result<(), String> {
        let has_profiles = fs::read_dir(&self.profiles_dir)
            .map_err(|e| format!("Failed to read profiles directory: {e}"))?
            .any(|entry| {
                entry
                    .ok()
                    .and_then(|e| {
                        e.path()
                            .extension()
                            .and_then(|ext| ext.to_str())
                            .map(|ext| ext == "json")
                    })
                    .unwrap_or(false)
            });

        if !has_profiles {
            log::info!("No profiles found — creating default profile");
            let default_params = AllDspParams {
                noise_gate: NoiseGateParams {
                    threshold: -40.0,
                    attack: 5.0,
                    release: 50.0,
                    range: -60.0,
                    bypassed: true,
                },
                eq: EqParams {
                    bands: default_eq_bands(),
                    bypassed: true,
                },
                de_esser: DeEsserParams {
                    frequency: 6000.0,
                    threshold: -20.0,
                    ratio: 4.0,
                    attack_ms: 1.0,
                    release_ms: 50.0,
                    bypassed: true,
                },
                compressor: CompressorParams {
                    threshold: -18.0,
                    ratio: 3.0,
                    attack: 10.0,
                    release: 100.0,
                    makeup_gain: 0.0,
                    bypassed: true,
                },
                limiter: LimiterParams {
                    ceiling: -1.0,
                    release: 50.0,
                    bypassed: false, // Limiter always active for safety
                },
            };
            self.save("Default", default_params)?;
        }

        Ok(())
    }

    // ── Internal helpers ────────────────────────────────────────────────────

    fn write_profile(&self, profile: &Profile) -> Result<(), String> {
        let path = self.profiles_dir.join(format!("{}.json", profile.id));
        let json = serde_json::to_string_pretty(profile)
            .map_err(|e| format!("Failed to serialize profile: {e}"))?;
        fs::write(&path, json)
            .map_err(|e| format!("Failed to write profile file: {e}"))
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_manager() -> (ProfileManager, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let manager = ProfileManager::new(dir.path()).unwrap();
        (manager, dir)
    }

    #[test]
    fn save_and_load_roundtrip() {
        let (mgr, _dir) = temp_manager();
        let params = AllDspParams::default();

        let summary = mgr.save("Test Profile", params.clone()).unwrap();
        assert_eq!(summary.name, "Test Profile");

        let loaded = mgr.load(&summary.id).unwrap();
        assert_eq!(loaded.name, "Test Profile");
        assert_eq!(loaded.noise_gate, params.noise_gate);
        assert_eq!(loaded.compressor, params.compressor);
        assert_eq!(loaded.limiter, params.limiter);
    }

    #[test]
    fn list_returns_sorted_by_updated_at() {
        let (mgr, _dir) = temp_manager();
        let params = AllDspParams::default();

        let _s1 = mgr.save("First", params.clone()).unwrap();
        // Ensure different timestamp
        std::thread::sleep(std::time::Duration::from_millis(10));
        let _s2 = mgr.save("Second", params).unwrap();

        let list = mgr.list().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "Second"); // most recent first
        assert_eq!(list[1].name, "First");
    }

    #[test]
    fn empty_name_rejected() {
        let (mgr, _dir) = temp_manager();
        let result = mgr.save("", AllDspParams::default());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));
    }

    #[test]
    fn whitespace_only_name_rejected() {
        let (mgr, _dir) = temp_manager();
        let result = mgr.save("   ", AllDspParams::default());
        assert!(result.is_err());
    }

    #[test]
    fn name_too_long_rejected() {
        let (mgr, _dir) = temp_manager();
        let long_name = "x".repeat(101);
        let result = mgr.save(&long_name, AllDspParams::default());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("100 characters"));

        // Exactly at the limit should be accepted
        let limit_name = "y".repeat(100);
        assert!(mgr.save(&limit_name, AllDspParams::default()).is_ok());
    }

    #[test]
    fn duplicate_name_rejected() {
        let (mgr, _dir) = temp_manager();
        mgr.save("My Preset", AllDspParams::default()).unwrap();

        let result = mgr.save("My Preset", AllDspParams::default());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already exists"));
    }

    #[test]
    fn duplicate_name_case_insensitive() {
        let (mgr, _dir) = temp_manager();
        mgr.save("My Preset", AllDspParams::default()).unwrap();

        // Different case should still be rejected
        let result = mgr.save("MY PRESET", AllDspParams::default());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already exists"));
    }

    #[test]
    fn delete_removes_file() {
        let (mgr, _dir) = temp_manager();
        let summary = mgr.save("To Delete", AllDspParams::default()).unwrap();

        mgr.delete(&summary.id).unwrap();
        assert!(mgr.load(&summary.id).is_err());
        assert_eq!(mgr.list().unwrap().len(), 0);
    }

    #[test]
    fn delete_nonexistent_returns_error() {
        let (mgr, _dir) = temp_manager();
        assert!(mgr.delete("nonexistent-id").is_err());
    }

    #[test]
    fn load_nonexistent_returns_error() {
        let (mgr, _dir) = temp_manager();
        assert!(mgr.load("nonexistent-id").is_err());
    }

    #[test]
    fn corrupt_file_skipped_in_list() {
        let (mgr, dir) = temp_manager();

        // Save a valid profile
        mgr.save("Valid", AllDspParams::default()).unwrap();

        // Write a corrupt file
        let corrupt_path = dir.path().join("profiles").join("corrupt.json");
        fs::write(&corrupt_path, "not valid json {{{").unwrap();

        let list = mgr.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Valid");
    }

    #[test]
    fn update_preserves_id_and_name() {
        let (mgr, _dir) = temp_manager();
        let summary = mgr.save("Original", AllDspParams::default()).unwrap();

        let new_params = AllDspParams {
            compressor: CompressorParams {
                threshold: -20.0,
                ratio: 4.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let updated = mgr.update(&summary.id, new_params).unwrap();
        assert_eq!(updated.id, summary.id);
        assert_eq!(updated.name, "Original");

        let loaded = mgr.load(&summary.id).unwrap();
        assert_eq!(loaded.compressor.threshold, -20.0);
        assert_eq!(loaded.compressor.ratio, 4.0);
    }

    #[test]
    fn ensure_default_creates_when_empty() {
        let (mgr, _dir) = temp_manager();

        mgr.ensure_default_profile().unwrap();

        let list = mgr.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Default");

        let profile = mgr.load(&list[0].id).unwrap();
        assert_eq!(profile.noise_gate.threshold, -40.0);
        assert_eq!(profile.limiter.ceiling, -1.0);
        assert!(!profile.limiter.bypassed); // limiter active for safety
    }

    #[test]
    fn ensure_default_noop_when_profiles_exist() {
        let (mgr, _dir) = temp_manager();

        mgr.save("Existing", AllDspParams::default()).unwrap();
        mgr.ensure_default_profile().unwrap();

        let list = mgr.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Existing");
    }

    #[test]
    fn profile_deserialize_with_missing_fields() {
        // Simulate loading a profile saved by an older version missing some fields
        let json = r#"{
            "id": "test-id",
            "name": "Old Profile",
            "createdAt": "2025-01-01T00:00:00Z",
            "updatedAt": "2025-01-01T00:00:00Z"
        }"#;

        let profile: Profile = serde_json::from_str(json).unwrap();
        assert_eq!(profile.version, 1);
        assert_eq!(profile.noise_gate, NoiseGateParams::default());
        assert_eq!(profile.compressor, CompressorParams::default());
    }

    #[test]
    fn unknown_filter_type_deserializes_to_unknown_variant() {
        // A hand-edited profile with an unrecognised filterType should not fail
        // to parse — it should produce FilterTypeSerde::Unknown which set_all_params
        // maps to FilterType::Bell.
        let json = r#"{
            "enabled": true,
            "filterType": "bandStop",
            "frequency": 1000.0,
            "gain": 0.0,
            "q": 1.0
        }"#;

        let band: EqBandData = serde_json::from_str(json).unwrap();
        assert_eq!(band.filter_type, FilterTypeSerde::Unknown);
    }

    /// Task 6.5 — a v1 profile JSON containing the obsolete `reduction` field
    /// must load without error, the `reduction` value must be discarded, and
    /// the de-esser ratio/attack/release must take their v2 defaults when
    /// applied to a `DspParams`.
    #[test]
    fn v1_profile_with_reduction_migrates_to_v2_defaults() {
        use crate::dsp::DspParams;

        let (mgr, dir) = temp_manager();

        // Hand-craft a v1 fixture with the obsolete `reduction` field.
        let v1_json = r#"{
            "id": "legacy-1",
            "name": "Legacy v1",
            "version": 1,
            "createdAt": "2025-01-01T00:00:00Z",
            "updatedAt": "2025-01-01T00:00:00Z",
            "deEsser": {
                "frequency": 7500.0,
                "threshold": -18.0,
                "reduction": 12.0,
                "bypassed": false
            }
        }"#;
        let path = dir.path().join("profiles").join("legacy-1.json");
        fs::write(&path, v1_json).unwrap();

        // Load — migration runs in-memory.
        let loaded = mgr.load("legacy-1").unwrap();
        assert_eq!(loaded.version, 2, "version must be bumped in-memory");
        assert_eq!(loaded.de_esser.ratio, 4.0);
        assert_eq!(loaded.de_esser.attack_ms, 1.0);
        assert_eq!(loaded.de_esser.release_ms, 50.0);
        // Frequency/threshold/bypassed are preserved across migration.
        assert_eq!(loaded.de_esser.frequency, 7500.0);
        assert_eq!(loaded.de_esser.threshold, -18.0);
        assert!(!loaded.de_esser.bypassed);

        // Apply to a fresh DspParams and confirm engine atomics match.
        let dsp = DspParams::new();
        dsp.set_all_params(&loaded.dsp_params());
        assert_eq!(dsp.deesser_ratio(), 4.0);
        assert_eq!(dsp.deesser_attack_ms(), 1.0);
        assert_eq!(dsp.deesser_release_ms(), 50.0);
        assert_eq!(dsp.deesser_frequency(), 7500.0);
        assert_eq!(dsp.deesser_threshold(), -18.0);
    }

    /// Task 6.6 — a v2 profile saved and loaded back must produce identical
    /// engine state. Round-trips through DspParams → AllDspParams → save →
    /// load → set_all_params and asserts the second snapshot matches the first.
    #[test]
    fn v2_save_load_roundtrip_preserves_engine_state() {
        use crate::dsp::DspParams;

        let (mgr, _dir) = temp_manager();

        // Stage a non-default engine state.
        let dsp = DspParams::new();
        let initial = AllDspParams {
            de_esser: DeEsserParams {
                frequency: 8200.0,
                threshold: -12.5,
                ratio: 6.5,
                attack_ms: 2.5,
                release_ms: 120.0,
                bypassed: false,
            },
            compressor: CompressorParams {
                threshold: -22.0,
                ratio: 3.5,
                attack: 8.0,
                release: 180.0,
                makeup_gain: 4.0,
                bypassed: false,
            },
            limiter: LimiterParams {
                ceiling: -0.5,
                release: 30.0,
                bypassed: false,
            },
            ..Default::default()
        };
        dsp.set_all_params(&initial);

        // Save → load via the on-disk path.
        let summary = mgr.save("Roundtrip", dsp.get_all_params()).unwrap();
        let loaded = mgr.load(&summary.id).unwrap();
        assert_eq!(loaded.version, CURRENT_PROFILE_VERSION);

        // Apply loaded params to a fresh DspParams and compare snapshots.
        let dsp2 = DspParams::new();
        dsp2.set_all_params(&loaded.dsp_params());

        let before = dsp.get_all_params();
        let after = dsp2.get_all_params();
        assert_eq!(before.noise_gate, after.noise_gate);
        assert_eq!(before.eq, after.eq);
        assert_eq!(before.de_esser, after.de_esser);
        assert_eq!(before.compressor, after.compressor);
        assert_eq!(before.limiter, after.limiter);
    }

    #[test]
    fn known_filter_types_round_trip_through_json() {
        let band = EqBandData { filter_type: FilterTypeSerde::HighShelf, frequency: 8000.0, ..Default::default() };
        let json = serde_json::to_string(&band).unwrap();
        assert!(json.contains("\"highShelf\""));

        let restored: EqBandData = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.filter_type, FilterTypeSerde::HighShelf);
    }
}
