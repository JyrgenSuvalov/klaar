//! Tauri IPC commands for detecting, installing, and uninstalling the
//! Klaar Virtual Audio Driver (HAL plug-in).
//!
//! The driver's UID is fixed in `driver/src/constants.rs::DEVICE_UID`. This
//! module hard-codes the same value so the engine and detection paths can
//! resolve the device by UID without enumerating names. If the driver's
//! constants ever change, both sides must update together.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use serde::Serialize;
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager, Runtime};

/// Reclaim focus on the main window after an `osascript … with administrator
/// privileges` round-trip. macOS hands focus to SecurityAgent for the admin
/// prompt and does NOT return it to the calling app on dismissal — without
/// this, the post-install success/failure dialog renders behind whatever
/// window happened to be frontmost when the prompt closed.
///
/// Best-effort: every step is `let _ = …` because nothing here is recoverable
/// and we still want the IPC result to surface unchanged.
pub(crate) fn refocus_main_window<R: Runtime>(app_handle: &AppHandle<R>) {
    if let Some(window) = app_handle.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

use crate::device_manager;

/// Last osascript exit code observed by `install_driver`. Set on every error
/// path, cleared on `Ok(())`. Read by `collect_coreaudio_diagnostics`.
static LAST_OSASCRIPT_EXIT: Mutex<Option<i32>> = Mutex::new(None);

fn set_last_osascript_exit(code: Option<i32>) {
    if let Ok(mut guard) = LAST_OSASCRIPT_EXIT.lock() {
        *guard = code;
    }
}

/// Public read helper consumed by the diagnostics command.
pub fn last_osascript_exit() -> Option<i32> {
    LAST_OSASCRIPT_EXIT.lock().ok().and_then(|g| *g)
}

/// Stable device UID advertised by the Klaar virtual driver.
/// Must match `driver/src/constants.rs::DEVICE_UID`.
pub const KLAAR_DRIVER_UID: &str = "KlaarVirtualMic_UID";

/// On-disk location of the installed driver bundle.
pub const KLAAR_DRIVER_PATH: &str = "/Library/Audio/Plug-Ins/HAL/Klaar.driver";

// ────────────────────────────────────────────────────────────────────────────
// InstallError
// ────────────────────────────────────────────────────────────────────────────

/// Structured error returned by `install_driver`.
///
/// Serialised to JS as `{ kind: "UserCancelled" }` or `{ kind: "CopyFailed", message: "…" }`
/// so the frontend can `switch` on `kind` without parsing strings.
///
/// NOTE: `RestartFailed` and `DeviceNotAppeared` were removed when the install
/// IPC stopped restarting `coreaudiod`. Install no longer kicks coreaudiod;
/// macOS picks up the new bundle on its own within a few seconds, and forcing
/// a restart was causing a CPU spike when a malformed bundle was present in
/// the HAL directory. Activation now requires a system reboot, surfaced via
/// the `reboot-required` onboarding screen.
#[derive(Debug, Serialize, thiserror::Error)]
#[serde(tag = "kind", content = "message")]
pub enum InstallError {
    #[error("User cancelled the admin prompt")]
    UserCancelled,
    #[error("Failed to copy driver: {0}")]
    CopyFailed(String),
}

// ────────────────────────────────────────────────────────────────────────────
// Detection
// ────────────────────────────────────────────────────────────────────────────

/// Fast filesystem-only check for the installed driver bundle.
#[tauri::command]
pub fn is_driver_file_present() -> bool {
    Path::new(KLAAR_DRIVER_PATH).exists()
}

/// CoreAudio enumeration check — returns `true` iff an output device with the
/// Klaar UID is currently visible to the HAL.
fn driver_device_present() -> bool {
    match device_manager::list_output_devices() {
        Ok(devices) => devices.iter().any(|d| d.uid == KLAAR_DRIVER_UID),
        Err(_) => false,
    }
}

/// Full "is it installed and usable" check: filesystem first (sub-ms), then
/// CoreAudio enumeration. Short-circuits if the filesystem path is missing.
#[tauri::command]
pub fn is_driver_installed() -> bool {
    if !is_driver_file_present() {
        return false;
    }
    driver_device_present()
}

/// Read `CFBundleVersion` from the installed driver's `Info.plist`.
/// Returns `None` on any read or parse failure (the update check only
/// needs the happy-path version string).
#[tauri::command]
pub fn get_installed_driver_version() -> Option<String> {
    read_bundle_version(Path::new(KLAAR_DRIVER_PATH))
}

fn read_bundle_version(bundle: &Path) -> Option<String> {
    let plist_path = bundle.join("Contents").join("Info.plist");
    let value = plist::Value::from_file(&plist_path).ok()?;
    let dict = value.as_dictionary()?;
    dict.get("CFBundleVersion")?.as_string().map(|s| s.to_string())
}

// ────────────────────────────────────────────────────────────────────────────
// Install
// ────────────────────────────────────────────────────────────────────────────

/// Install the bundled driver into `/Library/Audio/Plug-Ins/HAL/`.
///
/// One admin-prompt round-trip via `osascript`. The IPC does NOT restart
/// `coreaudiod` and does NOT poll for device enumeration — after install we
/// route the user to a reboot-required screen rather than restarting
/// coreaudiod ourselves; the kickstart path is SIP-blocked on stock macOS and
/// killall is unsafe when the HAL state is unknown. The new bundle is
/// activated by a system reboot, surfaced via the `reboot-required` onboarding
/// screen.
#[tauri::command]
pub async fn install_driver<R: Runtime>(
    app_handle: AppHandle<R>,
) -> Result<(), InstallError> {
    // Resolve the embedded driver bundle from app Resources.
    let source: PathBuf = app_handle
        .path()
        .resolve("resources/Klaar.driver", BaseDirectory::Resource)
        .map_err(|e| InstallError::CopyFailed(format!("resource resolve failed: {e}")))?;

    log::info!("install_driver: resolved source bundle = {}", source.display());
    if !source.exists() {
        return Err(InstallError::CopyFailed(format!(
            "driver bundle not found at {}. Is the driver staged into src-tauri/resources/? \
             Run `cd driver && cargo build --release && bash scripts/stage-driver.sh`.",
            source.display()
        )));
    }

    let source_str = source
        .to_str()
        .ok_or_else(|| InstallError::CopyFailed("non-UTF-8 source path".to_string()))?;

    let script = build_install_script(source_str, KLAAR_DRIVER_PATH);
    log::info!("install_driver: privileged script = {script}");

    // osascript blocks synchronously while the admin prompt is shown.
    // Run it on a blocking thread so we don't stall the Tauri async runtime.
    let invocation = build_osascript_invocation(&script, INSTALL_PROMPT);
    let output = tokio::task::spawn_blocking(move || {
        Command::new("osascript").arg("-e").arg(invocation).output()
    })
    .await
    .map_err(|e| {
        refocus_main_window(&app_handle);
        InstallError::CopyFailed(format!("osascript task join failed: {e}"))
    })?
    .map_err(|e| {
        refocus_main_window(&app_handle);
        InstallError::CopyFailed(format!("osascript spawn failed: {e}"))
    })?;

    // SecurityAgent has dismissed by this point — reclaim focus before the
    // frontend mounts the success/failure dialog so it doesn't render behind
    // another window. Done once here so every return path
    // below benefits.
    refocus_main_window(&app_handle);

    if !output.status.success() {
        let exit_code = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        log::warn!(
            "install_driver: osascript failed (exit={exit_code}) stdout={stdout:?} stderr={stderr:?}"
        );
        set_last_osascript_exit(Some(exit_code));
        return Err(classify_install_error(exit_code, &stderr));
    }
    log::info!("install_driver: osascript exited 0; verifying bundle on disk");

    // Synchronous post-install verification — the bundle's Info.plist must be
    // readable. This catches silent install corruption (missing resource,
    // wrong permissions) that the previous polling loop could mask. The
    // bundle won't enumerate as a CoreAudio device until reboot, so we do
    // NOT poll `is_driver_installed` here.
    let plist_path: PathBuf = Path::new(KLAAR_DRIVER_PATH)
        .join("Contents")
        .join("Info.plist");
    match std::fs::metadata(&plist_path) {
        Ok(_) => {
            set_last_osascript_exit(None);
            log::info!(
                "install_driver: bundle verified at {} — reboot required to activate",
                KLAAR_DRIVER_PATH
            );
            Ok(())
        }
        Err(e) => {
            set_last_osascript_exit(Some(0));
            Err(InstallError::CopyFailed(format!(
                "post-install verification: Info.plist unreadable at {}: {e}",
                plist_path.display()
            )))
        }
    }
}

/// Compose the single-line privileged shell script used by `install_driver`.
/// Kept separate for testability and to keep the quoting in one place.
///
/// IMPORTANT: This script does NOT restart `coreaudiod`. Restarting it after
/// dropping a fresh AudioServerPlugIn into the HAL directory is unreliable on
/// macOS 14+ (coreaudiod CPU-spins on plug-in cache
/// reconciliation, requiring a system reboot to recover). `launchctl kickstart`
/// is SIP-blocked, and SIGTERM/SIGKILL via `killall` both trigger the spike.
/// We surface a reboot-required prompt instead of restarting coreaudiod; see
/// the install_driver doc-comment above for rationale.
fn build_install_script(source: &str, dest: &str) -> String {
    // Quote the source path so spaces in the app Resources path are handled.
    // Destination is a trusted constant — no quoting needed.
    format!(
        "mkdir -p /Library/Audio/Plug-Ins/HAL && \
         rm -rf {dest} && \
         cp -R '{source}' {dest} && \
         xattr -dr com.apple.quarantine {dest} && \
         chown -R root:wheel {dest} && \
         chmod -R 755 {dest}"
    )
}

/// User-facing prompt shown in the macOS authorization dialog when installing
/// the driver. Becomes the headline of the dialog via AppleScript's `with
/// prompt` clause; "osascript wants to make changes" is demoted to small
/// italic subtitle. This is the closest we can get to Docker-style branding
/// without shipping a signed `SMAppService` privileged helper.
pub(crate) const INSTALL_PROMPT: &str = "Klaar wants to install the Klaar virtual audio driver.";

/// Counterpart prompt for the dev/QA-only uninstall path.
pub(crate) const UNINSTALL_PROMPT: &str = "Klaar wants to uninstall the Klaar virtual audio driver.";

/// Build the full AppleScript snippet passed to `osascript -e`:
///
/// `do shell script "<cmd>" with prompt "<prompt>" with administrator privileges`
///
/// Both `cmd` and `prompt` are escaped for embedding inside AppleScript `"..."`
/// literals. Pulled out so the format is unit-testable and identical across
/// install / uninstall paths.
fn build_osascript_invocation(cmd: &str, prompt: &str) -> String {
    format!(
        "do shell script \"{}\" with prompt \"{}\" with administrator privileges",
        escape_applescript(cmd),
        escape_applescript(prompt),
    )
}

/// Escape a string for embedding inside an AppleScript `"…"` literal that is
/// itself passed via `osascript -e`. Only `"` and `\` need escaping.
fn escape_applescript(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            other => out.push(other),
        }
    }
    out
}

/// Map `(exit_code, stderr)` from `osascript` to an `InstallError` variant.
/// Extracted for unit testing — avoids spawning real subprocesses.
fn classify_install_error(exit_code: i32, stderr: &str) -> InstallError {
    // osascript exits with code 1 and stderr "User cancelled." when the user
    // clicks Cancel on the admin prompt.
    if exit_code == 1 && stderr.to_lowercase().contains("user cancelled") {
        return InstallError::UserCancelled;
    }
    InstallError::CopyFailed(stderr.to_string())
}

// ────────────────────────────────────────────────────────────────────────────
// Uninstall (dev/QA only — not exposed in UI)
// ────────────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn uninstall_driver() -> Result<(), String> {
    // `killall coreaudiod` (SIGTERM) — NOT `killall -9`. See the rationale
    // on `build_install_script` above.
    let script = format!(
        "rm -rf {dest} && killall coreaudiod",
        dest = KLAAR_DRIVER_PATH
    );

    let invocation = build_osascript_invocation(&script, UNINSTALL_PROMPT);
    let output = tokio::task::spawn_blocking(move || {
        Command::new("osascript").arg("-e").arg(invocation).output()
    })
    .await
    .map_err(|e| format!("osascript task join failed: {e}"))?
    .map_err(|e| format!("osascript spawn failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("uninstall failed: {stderr}"));
    }
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // ── InstallError JSON shape ──────────────────────────────────────────────

    #[test]
    fn install_error_user_cancelled_json_shape() {
        let json = serde_json::to_value(InstallError::UserCancelled).unwrap();
        assert_eq!(json, serde_json::json!({ "kind": "UserCancelled" }));
    }

    #[test]
    fn install_error_copy_failed_json_shape() {
        let json =
            serde_json::to_value(InstallError::CopyFailed("perm denied".to_string())).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "kind": "CopyFailed", "message": "perm denied" })
        );
    }

    // ── classify_install_error ──────────────────────────────────────────────

    #[test]
    fn classify_user_cancelled_maps_to_variant() {
        let err = classify_install_error(1, "User cancelled.");
        assert!(matches!(err, InstallError::UserCancelled));
    }

    #[test]
    fn classify_user_cancelled_case_insensitive() {
        let err = classify_install_error(1, "osascript: user CANCELLED");
        assert!(matches!(err, InstallError::UserCancelled));
    }

    /// After `drop-coreaudiod-restart-from-install`, coreaudiod-related stderr
    /// can no longer reach `classify_install_error` because the install script
    /// no longer touches coreaudiod. Such stderr should fall through to
    /// `CopyFailed` rather than being specially mapped.
    #[test]
    fn classify_does_not_have_special_coreaudiod_branch() {
        let err = classify_install_error(137, "killall: coreaudiod: No such process");
        assert!(matches!(err, InstallError::CopyFailed(_)));
    }

    #[test]
    fn classify_generic_failure_maps_to_copy_failed() {
        let err = classify_install_error(42, "cp: /Library/Audio: Permission denied");
        match err {
            InstallError::CopyFailed(msg) => {
                assert!(msg.contains("Permission denied"));
            }
            other => panic!("expected CopyFailed, got {other:?}"),
        }
    }

    // ── escape_applescript ──────────────────────────────────────────────────

    #[test]
    fn escape_applescript_escapes_backslash_and_quote() {
        assert_eq!(escape_applescript(r#"a"b\c"#), r#"a\"b\\c"#);
    }

    #[test]
    fn escape_applescript_passes_through_plain_text() {
        assert_eq!(escape_applescript("mkdir -p /foo/bar"), "mkdir -p /foo/bar");
    }

    // ── build_install_script ────────────────────────────────────────────────

    #[test]
    fn build_install_script_contains_all_required_steps() {
        let script = build_install_script("/tmp/src", "/Library/Audio/Plug-Ins/HAL/Klaar.driver");
        assert!(script.contains("mkdir -p /Library/Audio/Plug-Ins/HAL"));
        assert!(script.contains("rm -rf /Library/Audio/Plug-Ins/HAL/Klaar.driver"));
        assert!(script.contains("cp -R '/tmp/src' /Library/Audio/Plug-Ins/HAL/Klaar.driver"));
        assert!(script.contains("xattr -dr com.apple.quarantine"));
        assert!(script.contains("chown -R root:wheel"));
        assert!(script.contains("chmod -R 755"));
    }

    /// Regression guard: install_driver must not invoke launchctl/killall
    /// against coreaudiod. The install script MUST NOT touch `coreaudiod` in
    /// any form (no `killall`, no `launchctl kickstart`, no equivalent).
    /// Activation requires a system reboot via the `reboot-required`
    /// onboarding screen.
    #[test]
    fn build_install_script_does_not_touch_coreaudiod() {
        let script = build_install_script("/tmp/src", "/Library/Audio/Plug-Ins/HAL/Klaar.driver");
        assert!(
            !script.contains("coreaudiod"),
            "install script must not reference coreaudiod (got: {script})"
        );
        assert!(
            !script.contains("launchctl"),
            "install script must not invoke launchctl (got: {script})"
        );
        assert!(
            !script.contains("killall"),
            "install script must not invoke killall (got: {script})"
        );
    }

    // ── build_osascript_invocation ──────────────────────────────────────────

    #[test]
    fn build_osascript_invocation_includes_with_prompt_clause() {
        // The `with prompt "..."` clause is what re-brands the macOS auth
        // dialog headline from "osascript wants to make changes" to our own
        // copy. Locking it here so a future refactor cannot silently regress
        // the install UX.
        let inv = build_osascript_invocation("ls /tmp", INSTALL_PROMPT);
        assert!(inv.starts_with("do shell script \""));
        assert!(inv.contains(" with prompt \""));
        assert!(inv.ends_with(" with administrator privileges"));
        assert!(inv.contains(INSTALL_PROMPT));
    }

    #[test]
    fn build_osascript_invocation_escapes_quotes_in_command() {
        let inv = build_osascript_invocation(r#"echo "hi""#, INSTALL_PROMPT);
        // Embedded `"` inside the command must be escaped so AppleScript
        // doesn't see it as the closing quote of the outer string literal.
        assert!(inv.contains(r#"echo \"hi\""#));
    }

    #[test]
    fn install_and_uninstall_prompts_mention_klaar() {
        // Tests our branding contract — the prompt user sees must not say
        // "osascript". Both prompts must reference Klaar so the dialog
        // headline carries our name.
        assert!(INSTALL_PROMPT.contains("Klaar"));
        assert!(UNINSTALL_PROMPT.contains("Klaar"));
        assert!(!INSTALL_PROMPT.to_lowercase().contains("osascript"));
        assert!(!UNINSTALL_PROMPT.to_lowercase().contains("osascript"));
    }

    // ── read_bundle_version ─────────────────────────────────────────────────

    const FIXTURE_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>com.klaar.driver</string>
    <key>CFBundleVersion</key>
    <string>1.2.3</string>
</dict>
</plist>
"#;

    #[test]
    fn read_bundle_version_parses_valid_plist() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Klaar.driver");
        fs::create_dir_all(bundle.join("Contents")).unwrap();
        fs::write(bundle.join("Contents/Info.plist"), FIXTURE_PLIST).unwrap();

        let version = read_bundle_version(&bundle);
        assert_eq!(version.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn read_bundle_version_missing_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("DoesNotExist.driver");
        assert_eq!(read_bundle_version(&bundle), None);
    }

    #[test]
    fn read_bundle_version_malformed_plist_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Bad.driver");
        fs::create_dir_all(bundle.join("Contents")).unwrap();
        fs::write(bundle.join("Contents/Info.plist"), "not a plist {{{").unwrap();

        assert_eq!(read_bundle_version(&bundle), None);
    }

    #[test]
    fn read_bundle_version_missing_key_returns_none() {
        const NO_VERSION: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>com.klaar.driver</string>
</dict>
</plist>
"#;
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("NoVersion.driver");
        fs::create_dir_all(bundle.join("Contents")).unwrap();
        fs::write(bundle.join("Contents/Info.plist"), NO_VERSION).unwrap();

        assert_eq!(read_bundle_version(&bundle), None);
    }
}
