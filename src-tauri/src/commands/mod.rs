/// Tauri IPC command handlers for Klaar.
///
/// Commands exposed to the frontend:
///   - list_input_devices / list_output_devices
///   - set_input_device / set_output_device
///   - set_processing_enabled / get_processing_enabled
///   - get_meters
///   - set_param / set_bypass / set_eq_band
///   - stop_engine
///   - list_profiles / save_profile / load_profile / update_profile / delete_profile
///   - get_settings / set_auto_launch / restore_session
///   - is_driver_file_present / is_driver_installed / get_installed_driver_version
///   - install_driver / uninstall_driver
///   - restart_mac / collect_coreaudio_diagnostics
///   - read_update_cache / write_update_cache / get_min_driver_version
pub mod driver;
pub mod mute;
pub mod system;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Runtime, State};
use crate::device_manager::{self, AudioDevice};
use crate::device_monitor::DeviceMonitor;
use crate::engine_state::{AudioEngineState, MeterData};
use crate::audio_engine::AudioEngine;
use crate::tray::{set_tray_state, TrayState};
use crate::dsp::DspParams;
use crate::dsp::spectrum::SpectrumReader;
use crate::profile_manager::{ProfileManager, ProfileSummary, AllDspParams};
use crate::recording_manager::RecordingManager;
use crate::settings_manager::{SettingsManager, AppSettings};
use crate::window_readiness::WindowReadiness;

// ────────────────────────────────────────────────────────────────────────────
// App state managed by Tauri
// ────────────────────────────────────────────────────────────────────────────

/// Tauri-managed state wrapping the audio engine behind a Mutex.
///
/// The Mutex is NOT on the audio thread — it's only locked by command handlers
/// and the device-monitor callback (all non-real-time callers).
pub struct EngineHandle {
    pub engine: Mutex<AudioEngine>,
    pub monitor: Mutex<Option<DeviceMonitor>>,
    pub state: Arc<AudioEngineState>,
    pub dsp_params: Arc<DspParams>,
    /// Spectrum reader — created when engine starts, None when stopped.
    /// Only locked by IPC command handlers, never by the audio thread.
    pub spectrum_reader: Mutex<Option<SpectrumReader>>,
    // Currently selected device UIDs (written/read by command handlers only)
    pub input_uid: Mutex<Option<String>>,
    pub output_uid: Mutex<Option<String>>,
    /// One-shot flag: `true` when `tauri-plugin-global-shortcut` failed to
    /// register `Ctrl+Shift+M` at startup (typically because another app
    /// already owns the chord). Read-and-clear via `get_hotkey_warning`;
    /// the frontend surfaces a toast on the next config-window mount, then
    /// the flag is cleared so subsequent mounts don't re-show the warning.
    pub hotkey_registration_failed: AtomicBool,
}

impl EngineHandle {
    pub fn new(state: Arc<AudioEngineState>, dsp_params: Arc<DspParams>) -> Self {
        Self {
            engine: Mutex::new(AudioEngine::new(state.clone(), dsp_params.clone())),
            monitor: Mutex::new(None),
            state,
            dsp_params,
            spectrum_reader: Mutex::new(None),
            input_uid: Mutex::new(None),
            output_uid: Mutex::new(None),
            hotkey_registration_failed: AtomicBool::new(false),
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Device enumeration
// ────────────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn list_input_devices() -> Result<Vec<AudioDevice>, String> {
    device_manager::list_input_devices()
}

#[tauri::command]
pub fn list_output_devices() -> Result<Vec<AudioDevice>, String> {
    device_manager::list_output_devices()
}

// ────────────────────────────────────────────────────────────────────────────
// Device selection
// ────────────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn set_input_device<R: Runtime>(
    uid: String,
    app: AppHandle<R>,
    handle: State<'_, EngineHandle>,
    settings_mgr: State<'_, SettingsManager>,
) -> Result<bool, String> {
    device_manager::validate_device_uid(&uid, true)?;

    log::info!("Input device selected: {uid}");
    *handle.input_uid.lock().unwrap() = Some(uid.clone());

    // Persist to settings
    if let Err(e) = settings_mgr.update(|s| s.input_device_id = Some(uid.clone())) {
        log::warn!("Failed to persist input device to settings: {e}");
    }

    try_start_engine(&app, &handle).await
}

// NOTE: `set_output_device` was removed when the driver-routing migration
// landed. The engine is hard-routed to the Klaar virtual driver UID
// (see `commands::driver::KLAAR_DRIVER_UID`). No output selector exists
// in the UI.

// ────────────────────────────────────────────────────────────────────────────
// Processing toggle
// ────────────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn set_processing_enabled<R: Runtime>(
    enabled: bool,
    app: AppHandle<R>,
    handle: State<'_, EngineHandle>,
    settings_mgr: State<'_, SettingsManager>,
) -> Result<(), String> {
    log::info!("Processing {}", if enabled { "enabled" } else { "disabled" });
    handle.state.processing_enabled.store(enabled, Ordering::Relaxed);

    // Persist to settings
    if let Err(e) = settings_mgr.update(|s| s.processing_enabled = enabled) {
        log::warn!("Failed to persist processing state to settings: {e}");
    }

    // Update tray icon based on processing + running state
    let is_running = handle.engine.lock().unwrap().is_running();
    let tray_state = if !is_running {
        TrayState::Red
    } else if enabled {
        TrayState::Green
    } else {
        TrayState::Yellow
    };
    set_tray_state(&app, tray_state);

    Ok(())
}

#[tauri::command]
pub fn get_processing_enabled(handle: State<'_, EngineHandle>) -> bool {
    handle.state.processing_enabled.load(Ordering::Relaxed)
}

// ────────────────────────────────────────────────────────────────────────────
// Meters
// ────────────────────────────────────────────────────────────────────────────
//
// NOTE: `get_meters` and `get_spectrum` are kept as separate IPC endpoints
// (rather than a combined `get_visualization_data`) because they serve
// different UI consumers with independent lifecycles:
//   - `get_meters` is polled by the engine store for level/GR meters
//   - `get_spectrum` is polled by the EQ canvas at its own RAF cadence
// Separate endpoints allow independent polling rates and simpler ownership.
// The IPC overhead of two calls (~0.3ms each) is negligible.

#[tauri::command]
pub fn get_meters(handle: State<'_, EngineHandle>) -> MeterData {
    let s = &handle.state;
    // Relaxed ordering is sufficient here — these are independent single-word
    // reads of display-only values where stale-by-one-frame is acceptable.
    // No happens-before relationship with the audio thread's Relaxed stores
    // is required.
    MeterData {
        input_level: f32::from_bits(s.input_level_peak.load(Ordering::Relaxed)),
        output_level: f32::from_bits(s.output_level_peak.load(Ordering::Relaxed)),
        gate_reduction: f32::from_bits(s.gate_reduction.load(Ordering::Relaxed)),
        de_esser_reduction: f32::from_bits(s.deesser_reduction.load(Ordering::Relaxed)),
        de_esser_sidechain_db: f32::from_bits(s.deesser_sidechain_db.load(Ordering::Relaxed)),
        compressor_reduction: f32::from_bits(s.comp_reduction.load(Ordering::Relaxed)),
        limiter_reduction: f32::from_bits(s.limiter_reduction.load(Ordering::Relaxed)),
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Spectrum
// ────────────────────────────────────────────────────────────────────────────

/// Response for the `get_spectrum` IPC command.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SpectrumData {
    /// 256 smoothed dB values, each in [DB_FLOOR, 0.0] (currently [-80.0, 0.0]).
    pub bins: Vec<f32>,
}

#[tauri::command]
pub fn get_spectrum(handle: State<'_, EngineHandle>) -> SpectrumData {
    let mut reader_guard = handle.spectrum_reader.lock().unwrap();
    match reader_guard.as_mut() {
        Some(reader) => {
            let bins = reader.get_spectrum().to_vec();
            SpectrumData { bins }
        }
        None => {
            // Engine not running — return floor values
            SpectrumData {
                bins: vec![crate::dsp::spectrum::DB_FLOOR; crate::dsp::spectrum::SPECTRUM_BINS],
            }
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Engine lifecycle (explicit)
// ────────────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn stop_engine<R: Runtime>(
    app: AppHandle<R>,
    handle: State<'_, EngineHandle>,
) -> Result<(), String> {
    log::info!("Stopping audio engine (explicit stop command)");
    handle.engine.lock().unwrap().stop();
    *handle.spectrum_reader.lock().unwrap() = None;
    *handle.monitor.lock().unwrap() = None;
    set_tray_state(&app, TrayState::Red);
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// Internal helper — start engine when both devices are selected
// ────────────────────────────────────────────────────────────────────────────

/// Returns `true` (and emits the canonical INFO log line) when the audio
/// engine must NOT start because the user has installed/updated the driver
/// and not yet rebooted.
///
/// Pulled out of `try_start_engine` so the gate is unit-testable without a
/// live `AppHandle` / CoreAudio. The single log line per blocked attempt is
/// the contract asserted by code review.
pub(crate) fn engine_start_blocked_for_reboot(state: &AudioEngineState) -> bool {
    if state.is_reboot_pending() {
        log::info!("engine_start_blocked: reboot_pending");
        true
    } else {
        false
    }
}

pub(crate) async fn try_start_engine<R: Runtime>(
    app: &AppHandle<R>,
    handle: &EngineHandle,
) -> Result<bool, String> {
    // Reboot-pending gate — short-circuit BEFORE the input-device / TCC /
    // CoreAudio work. Returning `Ok(false)` matches the "engine not started yet"
    // signal callers already handle (e.g. when no input is selected), so no
    // call-site changes are required.
    if engine_start_blocked_for_reboot(&handle.state) {
        return Ok(false);
    }

    let input_uid = handle.input_uid.lock().unwrap().clone();

    let Some(in_uid) = input_uid else {
        log::debug!("try_start_engine: waiting for input device to be selected");
        return Ok(false);
    };

    // Proactive TCC microphone gate. Must run BEFORE any CoreAudio work —
    // on macOS 12+ an un-prompted app gets `AudioUnit.start()` → noErr and
    // a callback full of zeros.
    //
    // On Deny we return early without mutating `output_uid` or touching
    // CoreAudio, so a future grant (via System Settings → focus-hook retry)
    // can re-enter this function with the same `input_uid`.
    // The `on_prompt_resolved` callback restores focus to the Klaar window
    // after the macOS TCC prompt closes — without it, focus returns to
    // whichever app was previously frontmost and the config window slips
    // behind other windows. Reuses the same helper that fixed the analogous
    // post-install focus-loss.
    crate::macos_permissions::ensure_microphone_permission(|| {
        crate::commands::driver::refocus_main_window(app);
    })
    .await?;

    // Output is hard-routed to the Klaar virtual driver — no user selection.
    let out_uid = crate::commands::driver::KLAAR_DRIVER_UID.to_string();
    *handle.output_uid.lock().unwrap() = Some(out_uid.clone());

    // Resolve device IDs before starting (needed for monitor).
    let input_id = device_manager::get_device_id_by_uid(&in_uid)?;
    let output_id = device_manager::get_device_id_by_uid(&out_uid)
        .map_err(|_| "driver_missing".to_string())?;

    log::info!("Starting audio engine: input={in_uid}, output={out_uid}");

    // Start engine
    {
        let mut engine = handle.engine.lock().unwrap();
        let spectrum_reader = engine.start(&in_uid, &out_uid).inspect_err(|e| {
            log::error!("Engine start failed: {e}");
            // On engine start failure, tray goes red
            set_tray_state(app, TrayState::Red);
        })?;
        // Store the spectrum reader for IPC access
        *handle.spectrum_reader.lock().unwrap() = Some(spectrum_reader);
    }

    // Update tray based on processing_enabled
    let processing = handle.state.processing_enabled.load(Ordering::Relaxed);
    set_tray_state(
        app,
        if processing {
            TrayState::Green
        } else {
            TrayState::Yellow
        },
    );

    // Start device monitor
    let app_clone = app.clone();
    let state_clone = handle.state.clone();

    // We need to stop the engine from the monitor callback. Since EngineHandle isn't
    // Clone, we capture the relevant pieces instead of the whole handle.
    let monitor = DeviceMonitor::start(input_id, output_id, move || {
        log::warn!("Device disconnected — stopping engine");
        // Reset meters to silence
        state_clone.set_input_peak(-96.0);
        state_clone.set_output_peak(-96.0);

        // Emit event to frontend
        let _ = app_clone.emit("audio://device-disconnected", ());

        // Set tray to red
        set_tray_state(&app_clone, TrayState::Red);

        // NOTE: We can't lock the engine mutex here without risking deadlock
        // since the monitor thread holds no other locks. The engine will stop
        // naturally — CoreAudio will error on the next callback cycle and
        // the UI will call stop_engine() on receiving the event.
    });

    *handle.monitor.lock().unwrap() = Some(monitor);

    Ok(true) // engine started successfully
}

// ────────────────────────────────────────────────────────────────────────────
// DSP parameter control
// ────────────────────────────────────────────────────────────────────────────

/// Set a single DSP parameter value by effect name and parameter name.
///
/// Effect names: noiseGate, eq (use set_eq_band for per-band), deEsser, compressor, limiter
/// Returns `Err` if the effect or parameter name is unrecognised, or if the value
/// falls outside the valid range for that parameter.
/// Inner routing logic for `set_param` — extracted for testability.
///
/// IMPORTANT: All `param` name strings use camelCase, matching the TypeScript
/// frontend exactly (e.g. "makeupGain", not "makeup_gain"). Any caller using
/// snake_case will receive an "Unknown … param" error. This is intentional —
/// the camelCase contract is the IPC boundary definition.
pub(crate) fn route_param(p: &DspParams, effect: &str, param: &str, v: f32) -> Result<(), String> {
    /// Return `Err` if `v` is outside `[min, max]`.
    fn range(effect: &str, param: &str, v: f32, min: f32, max: f32) -> Result<(), String> {
        if v < min || v > max {
            Err(format!(
                "{effect}.{param} value {v} out of range (expected {min}..={max})"
            ))
        } else {
            Ok(())
        }
    }

    match effect {
        "noiseGate" => match param {
            "threshold" => { range("noiseGate", "threshold", v, -80.0, 0.0)?;    DspParams::set_f32(&p.gate_threshold, v); }
            "attack"    => { range("noiseGate", "attack",    v,   0.1, 100.0)?;  DspParams::set_f32(&p.gate_attack,    v); }
            "release"   => { range("noiseGate", "release",   v,   1.0, 1000.0)?; DspParams::set_f32(&p.gate_release,   v); }
            "range"     => { range("noiseGate", "range",     v, -80.0, 0.0)?;    DspParams::set_f32(&p.gate_range,     v); }
            _ => return Err(format!("Unknown noiseGate param: {param}")),
        },
        "deEsser" => match param {
            "frequency" => { range("deEsser", "frequency", v, 2000.0, 12000.0)?; DspParams::set_f32(&p.deesser_frequency,  v); }
            "threshold" => { range("deEsser", "threshold", v,  -40.0,     0.0)?; DspParams::set_f32(&p.deesser_threshold,  v); }
            "ratio"     => { range("deEsser", "ratio",     v,    1.0,    10.0)?; DspParams::set_f32(&p.deesser_ratio,      v); }
            "attack"    => { range("deEsser", "attack",    v,    0.1,    10.0)?; DspParams::set_f32(&p.deesser_attack_ms,  v); }
            "release"   => { range("deEsser", "release",   v,   10.0,   500.0)?; DspParams::set_f32(&p.deesser_release_ms, v); }
            // The legacy `reduction` parameter was replaced by `ratio` in change
            // `improve-deesser-tweaking`. Reject explicitly so a stale frontend
            // surface fails loudly instead of silently no-op'ing.
            "reduction" => return Err("deEsser.reduction was replaced by 'ratio' in profile schema v2".to_string()),
            _ => return Err(format!("Unknown deEsser param: {param}")),
        },
        "compressor" => match param {
            "threshold"  => { range("compressor", "threshold",  v,  -60.0,    0.0)?; DspParams::set_f32(&p.comp_threshold, v); }
            "ratio"      => { range("compressor", "ratio",      v,    1.0,   20.0)?; DspParams::set_f32(&p.comp_ratio,     v); }
            "attack"     => { range("compressor", "attack",     v,    0.1,  100.0)?; DspParams::set_f32(&p.comp_attack,    v); }
            "release"    => { range("compressor", "release",    v,    1.0, 1000.0)?; DspParams::set_f32(&p.comp_release,   v); }
            "makeupGain" => { range("compressor", "makeupGain", v,    0.0,   30.0)?; DspParams::set_f32(&p.comp_makeup,    v); }
            _ => return Err(format!("Unknown compressor param: {param}")),
        },
        "limiter" => match param {
            "ceiling" => { range("limiter", "ceiling", v, -20.0,   0.0)?; DspParams::set_f32(&p.limiter_ceiling, v); }
            "release" => { range("limiter", "release", v,   1.0, 500.0)?; DspParams::set_f32(&p.limiter_release, v); }
            _ => return Err(format!("Unknown limiter param: {param}")),
        },
        _ => return Err(format!("Unknown effect: {effect}")),
    }
    Ok(())
}

#[tauri::command]
pub fn set_param(
    effect: String,
    param: String,
    value: f64,
    handle: State<'_, EngineHandle>,
) -> Result<(), String> {
    route_param(&handle.dsp_params, &effect, &param, value as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::params::DspParams;

    /// Verify that "makeupGain" (camelCase) routes correctly to the compressor
    /// makeup gain atomic and returns Ok.
    #[test]
    fn set_param_makeup_gain_camel_case_routes_ok() {
        let p = DspParams::new();
        let result = route_param(&p, "compressor", "makeupGain", 10.0);
        assert!(result.is_ok(), "expected Ok but got: {:?}", result);
        // Verify the atomic was actually updated
        let stored = DspParams::get_f32(&p.comp_makeup);
        assert!(
            (stored - 10.0).abs() < 1e-5,
            "expected comp_makeup = 10.0, got {stored}"
        );
    }

    /// Engine-start gate: when `reboot_pending` is set, the helper returns
    /// `true` so `try_start_engine` short-circuits before any CoreAudio /
    /// TCC / device-resolution work. Toggle round-trips through
    /// `AudioEngineState::set_reboot_pending` so we also exercise the
    /// atomic helpers.
    #[test]
    fn engine_start_blocked_for_reboot_tracks_atomic() {
        let state = AudioEngineState::new();
        assert!(
            !engine_start_blocked_for_reboot(&state),
            "fresh state must not block engine start"
        );

        state.set_reboot_pending(true);
        assert!(
            engine_start_blocked_for_reboot(&state),
            "reboot_pending=true must block engine start"
        );
        assert!(state.is_reboot_pending());

        state.set_reboot_pending(false);
        assert!(
            !engine_start_blocked_for_reboot(&state),
            "clearing reboot_pending must unblock engine start"
        );
        assert!(!state.is_reboot_pending());
    }

    /// Verify that snake_case "makeup_gain" is rejected — callers must use camelCase.
    #[test]
    fn set_param_makeup_gain_snake_case_returns_error() {
        let p = DspParams::new();
        let result = route_param(&p, "compressor", "makeup_gain", 10.0);
        assert!(
            result.is_err(),
            "expected Err for snake_case param name but got Ok"
        );
        let msg = result.unwrap_err();
        assert!(
            msg.contains("makeup_gain"),
            "error message should contain the bad param name, got: {msg}"
        );
    }
}

/// Set bypass state for a named effect.
///
/// Writes directly to `DspParams` atomics — no locking, no allocation.
/// Effect names: noiseGate, eq, deEsser, compressor, limiter
#[tauri::command]
pub fn set_bypass(
    effect: String,
    bypassed: bool,
    handle: State<'_, EngineHandle>,
) -> Result<(), String> {
    let p = &handle.dsp_params;
    match effect.as_str() {
        "noiseGate"  => p.bypass_gate.store(bypassed, Ordering::Relaxed),
        "eq"         => p.bypass_eq.store(bypassed, Ordering::Relaxed),
        "deEsser"    => p.bypass_deesser.store(bypassed, Ordering::Relaxed),
        "compressor" => p.bypass_compressor.store(bypassed, Ordering::Relaxed),
        "limiter"    => p.bypass_limiter.store(bypassed, Ordering::Relaxed),
        _ => return Err(format!("Unknown effect: {effect}")),
    }
    Ok(())
}

/// EQ band data from the frontend.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EqBandUpdate {
    pub enabled: bool,
    pub filter_type: String,
    pub frequency: f64,
    pub gain: f64,
    pub q: f64,
}

/// Update all parameters for a single EQ band (index 0–7).
#[tauri::command]
pub fn set_eq_band(
    band_index: usize,
    band: EqBandUpdate,
    handle: State<'_, EngineHandle>,
) -> Result<(), String> {
    if band_index > 7 {
        return Err(format!("Invalid band index {band_index}: must be 0–7"));
    }

    let filter_type_val = match band.filter_type.as_str() {
        "bell"       => crate::dsp::params::FilterType::Bell,
        "highPass"   => crate::dsp::params::FilterType::HighPass,
        "lowPass"    => crate::dsp::params::FilterType::LowPass,
        "highShelf"  => crate::dsp::params::FilterType::HighShelf,
        "lowShelf"   => crate::dsp::params::FilterType::LowShelf,
        "highPass48" => crate::dsp::params::FilterType::HighPass48,
        "lowPass48"  => crate::dsp::params::FilterType::LowPass48,
        _ => return Err(format!("Unknown filterType: {}", band.filter_type)),
    };

    let b = &handle.dsp_params.eq_bands[band_index];
    use std::sync::atomic::Ordering;
    b.enabled.store(if band.enabled { 1.0f32 } else { 0.0f32 }.to_bits(), Ordering::Relaxed);
    b.filter_type.store(filter_type_val.to_f32().to_bits(), Ordering::Relaxed);
    b.frequency.store((band.frequency as f32).clamp(20.0, 20000.0).to_bits(), Ordering::Relaxed);
    b.gain.store((band.gain as f32).clamp(-24.0, 24.0).to_bits(), Ordering::Relaxed);
    b.q.store((band.q as f32).clamp(0.1, 18.0).to_bits(), Ordering::Relaxed);

    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// Profile management
// ────────────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn list_profiles(
    profile_mgr: State<'_, Mutex<ProfileManager>>,
) -> Result<Vec<ProfileSummary>, String> {
    profile_mgr.lock().map_err(|e| format!("Lock poisoned: {e}"))?.list()
}

#[tauri::command]
pub fn save_profile(
    name: String,
    handle: State<'_, EngineHandle>,
    profile_mgr: State<'_, Mutex<ProfileManager>>,
    settings_mgr: State<'_, SettingsManager>,
) -> Result<ProfileSummary, String> {
    let params = handle.dsp_params.get_all_params();
    let summary = profile_mgr.lock().map_err(|e| format!("Lock poisoned: {e}"))?.save(&name, params)?;
    log::info!("Profile saved: '{}' (id={})", name, summary.id);

    // Set the newly saved profile as active
    settings_mgr.update(|s| {
        s.active_profile_id = Some(summary.id.clone());
    })?;

    Ok(summary)
}

#[tauri::command]
pub fn load_profile(
    id: String,
    handle: State<'_, EngineHandle>,
    profile_mgr: State<'_, Mutex<ProfileManager>>,
    settings_mgr: State<'_, SettingsManager>,
) -> Result<AllDspParams, String> {
    log::info!("Loading profile: {id}");
    let profile = profile_mgr.lock().map_err(|e| format!("Lock poisoned: {e}"))?.load(&id)?;
    let params = profile.dsp_params();

    // Apply all parameters to the engine atomics
    handle.dsp_params.set_all_params(&params);

    // Update active profile in settings
    settings_mgr.update(|s| {
        s.active_profile_id = Some(id);
    })?;

    Ok(params)
}

#[tauri::command]
pub fn update_profile(
    id: String,
    handle: State<'_, EngineHandle>,
    profile_mgr: State<'_, Mutex<ProfileManager>>,
) -> Result<ProfileSummary, String> {
    let params = handle.dsp_params.get_all_params();
    profile_mgr.lock().map_err(|e| format!("Lock poisoned: {e}"))?.update(&id, params)
}

#[tauri::command]
pub fn delete_profile(
    id: String,
    profile_mgr: State<'_, Mutex<ProfileManager>>,
    settings_mgr: State<'_, SettingsManager>,
) -> Result<(), String> {
    log::info!("Deleting profile: {id}");
    profile_mgr.lock().map_err(|e| format!("Lock poisoned: {e}"))?.delete(&id)?;

    // If deleted profile was active, clear active profile
    let settings = settings_mgr.get();
    if settings.active_profile_id.as_deref() == Some(&id) {
        settings_mgr.update(|s| {
            s.active_profile_id = None;
        })?;
    }

    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// Settings commands
// ────────────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_settings(
    settings_mgr: State<'_, SettingsManager>,
) -> AppSettings {
    settings_mgr.get()
}

#[tauri::command]
pub fn set_auto_launch<R: Runtime>(
    enabled: bool,
    app: AppHandle<R>,
    settings_mgr: State<'_, SettingsManager>,
) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;

    let autostart = app.autolaunch();
    if enabled {
        autostart.enable().map_err(|e| format!("Failed to enable auto-launch: {e}"))?;
    } else {
        autostart.disable().map_err(|e| format!("Failed to disable auto-launch: {e}"))?;
    }

    settings_mgr.update(|s| {
        s.auto_launch = enabled;
        // Fresh opt-ins write the current expected args version inline so
        // the migration path doesn't have to fire on the next launch (the
        // plugin already wrote the new args during `enable()` above). When
        // disabling, we still bump the marker — the next `enable()` will
        // pick up whatever the binary's current args are anyway, and a
        // non-zero marker here avoids a spurious migration on the next
        // launch with auto_launch=true.
        s.launch_agent_args_version = crate::EXPECTED_LAUNCH_AGENT_ARGS_VERSION;
    })
}

/// IPC notification fired by the frontend after the React `Root` component
/// mounts AND on every window-focus event. Acks the current readiness
/// generation so a stale ack from a previous show cycle (one whose
/// generation was bumped without a remount) is replaced by a fresh ack
/// for the live generation.
///
/// No payload, no return — idempotent within a generation. A second call
/// in the same generation is a no-op.
#[tauri::command]
pub fn frontend_ready<R: Runtime>(
    _app: AppHandle<R>,
    readiness: State<'_, WindowReadiness>,
) {
    let live = readiness.generation();
    readiness.mark_ready_for(live);
}

// ────────────────────────────────────────────────────────────────────────────
// Session restore
// ────────────────────────────────────────────────────────────────────────────

/// Called by the frontend on startup to restore the previous session state.
/// Returns the settings and optionally the loaded profile params.
#[tauri::command]
pub async fn restore_session<R: Runtime>(
    app: AppHandle<R>,
    handle: State<'_, EngineHandle>,
    profile_mgr: State<'_, Mutex<ProfileManager>>,
    settings_mgr: State<'_, SettingsManager>,
) -> Result<RestoreSessionResult, String> {
    log::info!("Restoring session...");
    let settings = settings_mgr.get();
    let mut result = RestoreSessionResult {
        settings: settings.clone(),
        loaded_profile_params: None,
        warnings: Vec::new(),
    };

    // 1. Select input device (if saved)
    if let Some(ref uid) = settings.input_device_id {
        match crate::device_manager::validate_device_uid(uid, true) {
            Ok(_) => {
                *handle.input_uid.lock().unwrap() = Some(uid.clone());
            }
            Err(_) => {
                result.warnings.push(format!("Saved input device '{}' not found, using default", uid));
                settings_mgr.update(|s| s.input_device_id = None).ok();
            }
        }
    }

    // 2. Output device is hard-routed to the Klaar virtual driver —
    // the persisted `output_device_id` from previous builds is ignored.
    // `try_start_engine` resolves the UID at start time.
    if settings.output_device_id.is_some() {
        settings_mgr.update(|s| s.output_device_id = None).ok();
    }

    // 3. Load active profile (if set)
    if let Some(ref profile_id) = settings.active_profile_id {
        match profile_mgr.lock().map_err(|e| format!("Lock poisoned: {e}"))?.load(profile_id) {
            Ok(profile) => {
                let params = profile.dsp_params();
                handle.dsp_params.set_all_params(&params);
                result.loaded_profile_params = Some(params);
            }
            Err(_) => {
                result.warnings.push(format!("Saved profile '{}' not found, using defaults", profile_id));
                settings_mgr.update(|s| s.active_profile_id = None).ok();
            }
        }
    }

    // 4. Restore processing state
    handle.state.processing_enabled.store(settings.processing_enabled, Ordering::Relaxed);

    // 5. Try to start the engine if both devices are set.
    //    A persistent mic-permission denial surfaces as a structured warning
    //    so the frontend's existing `parseEngineError` can recognise it and
    //    set `engineError` to `microphone_permission_denied_persistent`.
    if let Err(e) = try_start_engine(&app, &handle).await {
        result.warnings.push(format!("Failed to start audio engine: {}", e));
    }

    for warning in &result.warnings {
        log::warn!("Session restore: {}", warning);
    }

    Ok(result)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreSessionResult {
    pub settings: AppSettings,
    pub loaded_profile_params: Option<AllDspParams>,
    pub warnings: Vec<String>,
}

// ────────────────────────────────────────────────────────────────────────────
// Record & Playback commands
// ────────────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn start_recording(
    max_seconds: u32,
    handle: State<'_, EngineHandle>,
    recording_mgr: State<'_, Mutex<RecordingManager>>,
) -> Result<(), String> {
    // Verify engine is running
    if !handle.engine.lock().map_err(|e| format!("Lock poisoned: {e}"))?.is_running() {
        return Err("Engine is not running — cannot record".to_string());
    }

    let sample_rate = handle.state.get_sample_rate();

    let mut mgr = recording_mgr
        .lock()
        .map_err(|e| format!("Lock poisoned: {e}"))?;

    let buffer = mgr.start_recording(max_seconds, sample_rate)?;

    // Store the buffer in the engine state so the audio callback can access it
    handle.state.set_recording(Some(buffer));

    Ok(())
}

#[tauri::command]
pub fn stop_recording(
    handle: State<'_, EngineHandle>,
    recording_mgr: State<'_, Mutex<RecordingManager>>,
) -> Result<String, String> {
    // Clear the recording state from the audio callback first
    handle.state.clear_recording();

    let mut mgr = recording_mgr
        .lock()
        .map_err(|e| format!("Lock poisoned: {e}"))?;

    mgr.stop_recording()
}

#[tauri::command]
pub fn play_recording(
    recording_id: String,
    output_device_uid: String,
    handle: State<'_, EngineHandle>,
    recording_mgr: State<'_, Mutex<RecordingManager>>,
) -> Result<(), String> {
    let mut mgr = recording_mgr
        .lock()
        .map_err(|e| format!("Lock poisoned: {e}"))?;

    mgr.play_recording(&recording_id, &output_device_uid, &handle.state)
}

#[tauri::command]
pub fn stop_playback(
    handle: State<'_, EngineHandle>,
    recording_mgr: State<'_, Mutex<RecordingManager>>,
) -> Result<(), String> {
    let mut mgr = recording_mgr
        .lock()
        .map_err(|e| format!("Lock poisoned: {e}"))?;

    mgr.stop_playback(&handle.state)
}

#[tauri::command]
pub fn delete_recording(
    recording_id: String,
    recording_mgr: State<'_, Mutex<RecordingManager>>,
) -> Result<(), String> {
    let mgr = recording_mgr
        .lock()
        .map_err(|e| format!("Lock poisoned: {e}"))?;

    mgr.delete_recording(&recording_id)
}
