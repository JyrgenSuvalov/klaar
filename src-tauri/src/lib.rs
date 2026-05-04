mod audio_engine;
mod commands;
mod constants;
mod device_change_listener;
mod device_format_listener;
mod device_manager;
mod device_monitor;
mod dsp;
mod engine_state;
mod logging;
mod macos_permissions;
pub mod profile_manager;
mod recording_manager;
pub mod settings_manager;
mod tray;
mod util;

use std::sync::{Arc, Mutex};
use tauri::Manager;

use commands::driver::{
    get_installed_driver_version, install_driver, is_driver_file_present, is_driver_installed,
    uninstall_driver,
};
use commands::mute::{get_hotkey_warning, get_mute_state, set_mute, toggle_mute};
use commands::system::{
    collect_coreaudio_diagnostics, get_app_version, get_boot_timestamp,
    get_microphone_authorization_status, get_min_driver_version, open_privacy_microphone_settings,
    read_onboarding_state, read_update_cache, report_frontend_error, request_reboot, restart_mac,
    seed_reboot_pending_from_disk, set_reboot_pending, write_onboarding_state, write_update_cache,
};
use commands::{
    delete_profile, delete_recording, get_meters, get_processing_enabled, get_settings,
    get_spectrum, list_input_devices, list_output_devices, list_profiles, load_profile,
    play_recording, restore_session, save_profile, set_auto_launch, set_bypass,
    set_eq_band, set_input_device, set_param,
    set_processing_enabled, start_recording, stop_engine, stop_playback,
    stop_recording, update_profile, EngineHandle,
};
use dsp::DspParams;
use engine_state::AudioEngineState;
use profile_manager::ProfileManager;
use recording_manager::RecordingManager;
use settings_manager::SettingsManager;
use tray::{init_tray, refresh_tray_menu_labels};

/// Stop the engine and notify the frontend if either currently selected
/// device has disappeared from the system.
///
/// Shared between `DeviceChangeListener` (topology change: add/remove) and
/// `DeviceFormatListener` (format change: nominal SR change). Either kind
/// of CoreAudio notification can race with a device unplug, so both
/// callbacks run this check.
fn reconcile_selected_devices(app_handle: &tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    let engine_handle: tauri::State<'_, EngineHandle> = app_handle.state();
    let input_uid = engine_handle.input_uid.lock().unwrap().clone();
    let output_uid = engine_handle.output_uid.lock().unwrap().clone();

    let input_missing = input_uid
        .as_deref()
        .is_some_and(|uid| device_manager::validate_device_uid(uid, true).is_err());
    let output_missing = output_uid
        .as_deref()
        .is_some_and(|uid| device_manager::validate_device_uid(uid, false).is_err());

    if input_missing || output_missing {
        log::warn!(
            "Selected device removed (input_missing={input_missing}, output_missing={output_missing}) — stopping engine"
        );
        engine_handle.engine.lock().unwrap().stop();
        *engine_handle.monitor.lock().unwrap() = None;

        // Reset meters
        engine_handle.state.set_input_peak(-96.0);
        engine_handle.state.set_output_peak(-96.0);

        let _ = app_handle.emit("audio://device-disconnected", ());
        tray::set_tray_state(app_handle, tray::TrayState::Red);
    }
}

/// If the engine is running and the nominal sample rate of either currently
/// selected device differs from the engine's negotiated SR, stop the engine
/// so the frontend's auto-reconnect path can restart it at the fresh rate
/// (decision D4 of `detect-sample-rate-changes`).
///
/// A `get_device_sample_rate_by_uid` `Err` is treated as "no change, retry
/// next debounce window" rather than a mismatch — AMS occasionally reports
/// stale values during a transition (decision D3 / requirement 3.3).
fn check_selected_sample_rate_drift(app_handle: &tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    let engine_handle: tauri::State<'_, EngineHandle> = app_handle.state();

    // Snapshot relevant state under short locks; release before any heavy work.
    let (current_sr, input_uid, output_uid) = {
        let engine = engine_handle.engine.lock().unwrap();
        let Some(sr) = engine.current_sample_rate() else {
            return; // engine not running — nothing to do
        };
        drop(engine);
        let input_uid = engine_handle.input_uid.lock().unwrap().clone();
        let output_uid = engine_handle.output_uid.lock().unwrap().clone();
        (sr, input_uid, output_uid)
    };

    let mut drift = false;
    for uid in [input_uid.as_deref(), output_uid.as_deref()]
        .into_iter()
        .flatten()
    {
        match device_manager::get_device_sample_rate_by_uid(uid) {
            Ok(sr) => {
                let sr_int = sr.round() as u32;
                if sr_int != current_sr {
                    log::info!(
                        "DeviceFormatListener: SR drift on {uid}: engine={} Hz, device={} Hz",
                        current_sr,
                        sr_int
                    );
                    drift = true;
                }
            }
            Err(e) => {
                // Tolerate transient SR=0 / fetch errors (req. 3.3).
                log::debug!(
                    "DeviceFormatListener: SR fetch err on {uid} (treated as no-change): {e}"
                );
            }
        }
    }

    if drift {
        log::warn!(
            "DeviceFormatListener: in-use device SR changed — stopping engine for auto-reconnect"
        );
        engine_handle.engine.lock().unwrap().stop();
        *engine_handle.monitor.lock().unwrap() = None;
        engine_handle.state.set_input_peak(-96.0);
        engine_handle.state.set_output_peak(-96.0);
        let _ = app_handle.emit("audio://device-disconnected", ());
        tray::set_tray_state(app_handle, tray::TrayState::Red);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if let Err(e) = logging::init_logging() {
        eprintln!("Failed to initialise logging: {}", e);
    }

    // Shared atomic engine state — cloned into Tauri managed state and engine.
    let engine_state = Arc::new(AudioEngineState::new());
    let dsp_params = Arc::new(DspParams::new());
    let engine_handle = EngineHandle::new(engine_state, dsp_params);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // Global mute shortcut.
        // The plugin is built without pre-registered shortcuts; the actual
        // chord (Ctrl+Shift+M) is registered at runtime after the engine and
        // tray are up so registration failures can flow into the
        // `hotkey_registration_failed` flag with full app context.
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(engine_handle)
        .setup(|app| {
            // Suppress Dock icon — this app lives entirely in the menu bar.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Initialize profile and settings managers
            let app_data_dir: std::path::PathBuf = app
                .path()
                .app_data_dir()
                .map_err(|e| e.to_string())?;

            let profile_mgr = ProfileManager::new(&app_data_dir)
                .map_err(|e| e.to_string())?;
            profile_mgr.ensure_default_profile()
                .map_err(|e| e.to_string())?;

            let settings_mgr = SettingsManager::new(&app_data_dir);

            let recording_mgr = RecordingManager::new(&app_data_dir)
                .map_err(|e| e.to_string())?;

            // Auto-cleanup old recordings (>24h)
            if let Err(e) = recording_mgr.cleanup_old_recordings() {
                log::warn!("Failed to clean up old recordings: {e}");
            }

            app.manage(Mutex::new(profile_mgr));
            app.manage(Mutex::new(recording_mgr));
            app.manage(settings_mgr);

            // Register system-wide device change listener (topology: add/remove)
            // and per-device sample-rate listener (format: nominal SR change).
            // The two listeners share the `reconcile_selected_devices` helper
            // so the disconnect-on-removal path stays identical regardless of
            // which CoreAudio property triggered it.
            {
                let app_handle = app.handle().clone();
                match device_change_listener::DeviceChangeListener::new(move || {
                    log::info!("CoreAudio device list changed — notifying frontend");
                    use tauri::Emitter;
                    let _ = app_handle.emit("audio://devices-changed", ());

                    reconcile_selected_devices(&app_handle);

                    // Re-sync the per-device SR listeners against the fresh
                    // device list so plugged devices acquire a listener and
                    // unplugged ones drop theirs.
                    if let Some(format_listener) =
                        app_handle.try_state::<device_format_listener::DeviceFormatListener>()
                    {
                        match device_manager::list_all_device_ids() {
                            Ok(ids) => format_listener.sync(&ids),
                            Err(e) => log::warn!(
                                "DeviceChangeListener: list_all_device_ids failed: {e}"
                            ),
                        }
                    }
                }) {
                    Ok(listener) => {
                        app.manage(listener);
                    }
                    Err(e) => {
                        log::error!("Failed to register device change listener: {e}");
                    }
                }
            }

            // Register per-device nominal-sample-rate listener.
            //
            // Fires (debounced 150 ms) whenever any enumerated device's
            // nominal SR changes in Audio MIDI Setup. On fire we:
            //   1. If engine is running and an in-use device's SR drifted
            //      from the engine's negotiated SR, stop the engine so the
            //      frontend's auto-reconnect path can restart it cleanly at
            //      the new rate (decision D4).
            //   2. Run the standard `reconcile_selected_devices` check.
            //   3. Emit `audio://devices-changed` so the frontend re-enumerates
            //      and (if appropriate) auto-reconnects — this clears a stuck
            //      `sample_rate_mismatch` engine error without user interaction.
            {
                let app_handle = app.handle().clone();
                match device_format_listener::DeviceFormatListener::new(move || {
                    log::info!(
                        "CoreAudio nominal sample rate changed — checking engine + notifying frontend"
                    );
                    check_selected_sample_rate_drift(&app_handle);
                    reconcile_selected_devices(&app_handle);
                    use tauri::Emitter;
                    let _ = app_handle.emit("audio://devices-changed", ());
                }) {
                    Ok(format_listener) => {
                        // Initial sync: register a listener on every currently
                        // enumerated device.
                        match device_manager::list_all_device_ids() {
                            Ok(ids) => {
                                log::info!(
                                    "DeviceFormatListener: initial sync against {} device(s)",
                                    ids.len()
                                );
                                format_listener.sync(&ids);
                            }
                            Err(e) => log::warn!(
                                "DeviceFormatListener: initial list_all_device_ids failed: {e}"
                            ),
                        }
                        app.manage(format_listener);
                    }
                    Err(e) => {
                        log::error!("Failed to register device-format listener: {e}");
                    }
                }
            }

            // Seed the in-memory `reboot_pending` atomic from the
            // file-backed onboarding store so the engine gate fires on the
            // very first `try_start_engine` call (before the frontend has
            // a chance to round-trip through `set_reboot_pending`). The
            // frontend remains the source of truth for clearing the file
            // entry on detected reboots.
            {
                let app_handle = app.handle().clone();
                let engine_handle: tauri::State<'_, EngineHandle> = app_handle.state();
                let pending = seed_reboot_pending_from_disk(&app_handle);
                engine_handle.state.set_reboot_pending(pending);
                if pending {
                    log::info!("startup: seeded reboot_pending=true from onboarding-state.json");
                }
            }

            init_tray(app.handle())?;

            // ── Global mute hotkey (Ctrl+Shift+M) ───────────────────────
            // Registered AFTER the tray + engine handle are live so the
            // shortcut callback can resolve `EngineHandle` from app state
            // on its first fire. Failure is non-fatal: log WARN, set the
            // one-shot flag (read-and-cleared by `get_hotkey_warning`),
            // and continue startup. The mute button in the config window
            // remains the always-available fallback.
            {
                use tauri_plugin_global_shortcut::{
                    Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState,
                };

                let shortcut =
                    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyM);

                let registration = app.global_shortcut().on_shortcut(
                    shortcut,
                    move |app_handle, _shortcut, event| {
                        // The handler fires on both press and release —
                        // only toggle on the press edge to avoid double-
                        // firing per keypress.
                        if event.state() != ShortcutState::Pressed {
                            return;
                        }
                        let engine: tauri::State<'_, EngineHandle> = app_handle.state();
                        let new_muted = commands::mute::toggle_mute_shared(
                            app_handle,
                            &engine.dsp_params,
                        );
                        log::debug!("global hotkey ⌃⇧M → muted={new_muted}");
                    },
                );

                if let Err(e) = registration {
                    log::warn!(
                        "failed to register Ctrl+Shift+M global shortcut: {e}. \
                         Mute button still works."
                    );
                    let engine: tauri::State<'_, EngineHandle> = app.state();
                    engine
                        .hotkey_registration_failed
                        .store(true, std::sync::atomic::Ordering::Release);
                } else {
                    log::info!("registered global mute shortcut: ⌃⇧M");
                }
            }

            // ── Tray icon refresh on mute changes ───────────────────────
            // Subscribe to `klaar://mute-changed` so the tray icon and
            // tooltip flip in real time when mute toggles via *any* path
            // (hotkey, IPC from the config window, or the tray context
            // menu itself). The shared `commands::mute` chokepoint emits
            // the event; this listener is the only place the tray reads it.
            {
                use tauri::Listener;
                let app_handle = app.handle().clone();
                app.listen(commands::mute::MUTE_CHANGED_EVENT, move |_event| {
                    tray::refresh_tray_for_mute_change(&app_handle);
                });
            }

            // Surface the main window on cold launch (Finder, `open -a`,
            // Raycast, etc.). The window ships with `visible: false` in
            // tauri.conf.json to avoid the unstyled flash before the webview
            // is ready; we explicitly show + focus it here once setup is
            // complete. Without this, `LSUIElement=true` leaves the app
            // reachable only via the tray icon on first launch.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
            // Keep the tray show/hide label in sync after the cold-launch
            // show. (`init_tray` defaults the label to "Hide" assuming this
            // path runs, but make the invariant explicit.)
            refresh_tray_menu_labels(app.handle());

            // Intercept the red close button and `Cmd+W`: hide the window
            // instead of letting Tauri destroy it (which would tear the app
            // down with the only registered window). The audio engine keeps
            // streaming while hidden; quit is reachable from the tray menu.
            // Spec: app-shell "Red close button hides window" / "Cmd+W hides
            // window".
            if let Some(window) = app.get_webview_window("main") {
                let app_handle = app.handle().clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        if let Some(w) = app_handle.get_webview_window("main") {
                            let _ = w.hide();
                        }
                        refresh_tray_menu_labels(&app_handle);
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_input_devices,
            list_output_devices,
            set_input_device,
            set_processing_enabled,
            get_processing_enabled,
            get_meters,
            get_spectrum,
            stop_engine,
            set_param,
            set_bypass,
            set_eq_band,
            // Profile management
            list_profiles,
            save_profile,
            load_profile,
            update_profile,
            delete_profile,
            // Settings
            get_settings,
            set_auto_launch,
            restore_session,
            // Record & playback
            start_recording,
            stop_recording,
            play_recording,
            stop_playback,
            delete_recording,
            // Driver installation / detection
            is_driver_file_present,
            is_driver_installed,
            get_installed_driver_version,
            install_driver,
            uninstall_driver,
            // System / diagnostics / update cache
            restart_mac,
            collect_coreaudio_diagnostics,
            read_update_cache,
            write_update_cache,
            get_min_driver_version,
            get_app_version,
            // Reboot-required onboarding flow
            request_reboot,
            get_boot_timestamp,
            read_onboarding_state,
            write_onboarding_state,
            set_reboot_pending,
            // TCC microphone recovery
            open_privacy_microphone_settings,
            get_microphone_authorization_status,
            // Frontend error reporting (diagnostics)
            report_frontend_error,
            // Global mute
            toggle_mute,
            set_mute,
            get_mute_state,
            get_hotkey_warning,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Klaar")
        .run(|app_handle, event| {
            // Re-surface the main window when the app is relaunched while
            // already running with no visible windows. On macOS with
            // `LSUIElement=true`, a second `open -a Klaar` (or Finder
            // double-click, or Raycast launch) delivers `RunEvent::Reopen`
            // rather than a fresh process — without this handler the user
            // would have to click the tray icon to find the window again.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } = event
            {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
                refresh_tray_menu_labels(app_handle);
            }
            // Suppress unused-variable warnings on non-macOS targets.
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (app_handle, event);
            }
        });
}
