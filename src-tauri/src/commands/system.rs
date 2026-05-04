//! System-level Tauri commands used by the onboarding/update UX:
//!
//!   - `restart_mac`          — reboot the machine via AppleScript
//!   - `collect_coreaudio_diagnostics` — grab the last 5 min of the audio subsystem log
//!   - `read_update_cache` / `write_update_cache` — disk-backed cache for the
//!     GitHub release update check
//!   - `get_min_driver_version` — expose `MIN_DRIVER_VERSION` to the frontend
//!
//! None of these touch the real-time audio path.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager, Runtime};

use crate::commands::driver::last_osascript_exit;
use crate::commands::EngineHandle;
use crate::constants::MIN_DRIVER_VERSION;

// ────────────────────────────────────────────────────────────────────────────
// restart_mac
// ────────────────────────────────────────────────────────────────────────────

/// Trigger a system restart via AppleScript. Used by the driver-load-pending
/// fallback dialog when the user clicks **Restart Now**.
///
/// The `«event aevtrrst»` four-character code is the classic `loginwindow`
/// restart event. Apple has kept it working but it's documented-but-unofficial
/// — if it ever breaks, the frontend falls back to a toast instructing the
/// user to restart from the Apple menu.
#[tauri::command]
pub async fn restart_mac() -> Result<(), String> {
    let output = tokio::task::spawn_blocking(|| {
        Command::new("osascript")
            .arg("-e")
            .arg(r#"tell application "loginwindow" to «event aevtrrst»"#)
            .output()
    })
    .await
    .map_err(|e| format!("osascript task join failed: {e}"))?
    .map_err(|e| format!("osascript spawn failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("osascript exited {}", output.status.code().unwrap_or(-1))
        } else {
            stderr
        });
    }
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// request_reboot — invoked from the `reboot-required` onboarding screen
// ────────────────────────────────────────────────────────────────────────────

/// Structured error returned by `request_reboot`. Mirrors the shape of
/// [`crate::commands::driver::InstallError`] so the frontend can `switch`
/// on `kind` without parsing free-form strings.
#[derive(Debug, Serialize, thiserror::Error)]
#[serde(tag = "kind", content = "message")]
pub enum RebootError {
    #[error("User cancelled the reboot prompt")]
    UserCancelled,
    #[error("osascript failed: {0}")]
    OsascriptFailed(String),
}

/// Initiate a graceful system reboot via `System Events`. Used by the
/// `reboot-required` onboarding screen's **Reboot Now** button.
///
/// Unlike [`restart_mac`] (which uses the `loginwindow` `aevtrrst` event),
/// this routes through `System Events`, which presents the standard macOS
/// reboot confirmation dialog and gives other apps a chance to save state.
/// No admin password is required — `System Events` runs in the user context.
#[tauri::command]
pub async fn request_reboot() -> Result<(), RebootError> {
    let output = tokio::task::spawn_blocking(|| {
        Command::new("osascript")
            .args([
                "-e",
                "tell application \"System Events\" to restart",
            ])
            .output()
    })
    .await
    .map_err(|e| RebootError::OsascriptFailed(format!("task join failed: {e}")))?
    .map_err(|e| RebootError::OsascriptFailed(format!("spawn failed: {e}")))?;

    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let lower = stderr.to_lowercase();
    // osascript localises the message; cover both spellings ("canceled" and
    // "cancelled") to be safe across locales / Apple wording changes.
    if lower.contains("user canceled") || lower.contains("user cancelled") {
        return Err(RebootError::UserCancelled);
    }
    Err(RebootError::OsascriptFailed(if stderr.is_empty() {
        format!("osascript exited {}", output.status.code().unwrap_or(-1))
    } else {
        stderr
    }))
}

// ────────────────────────────────────────────────────────────────────────────
// get_boot_timestamp — sysctl kern.boottime, used by the reboot-detection
// guard in the file-backed onboarding store.
// ────────────────────────────────────────────────────────────────────────────

/// Return the seconds component of `kern.boottime`. This value changes only
/// when the system reboots, so the frontend can detect "did we reboot since
/// the user clicked Later?" by comparing the recorded timestamp to a fresh
/// reading on every gate evaluation.
///
/// Returns `0` on failure (non-macOS targets, or sysctl error). Frontend
/// treats `0` as "unable to read" and conservatively keeps the persisted
/// reboot-pending state.
#[tauri::command]
pub fn get_boot_timestamp() -> i64 {
    boot_timestamp_secs()
}

#[cfg(target_os = "macos")]
fn boot_timestamp_secs() -> i64 {
    use std::ffi::CString;
    use std::mem;

    let name = match CString::new("kern.boottime") {
        Ok(n) => n,
        Err(_) => return 0,
    };
    let mut tv: libc::timeval = unsafe { mem::zeroed() };
    let mut size = mem::size_of::<libc::timeval>();
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            &mut tv as *mut _ as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc == 0 {
        tv.tv_sec as i64
    } else {
        0
    }
}

#[cfg(not(target_os = "macos"))]
fn boot_timestamp_secs() -> i64 {
    0
}

// ────────────────────────────────────────────────────────────────────────────
// collect_coreaudio_diagnostics
// ────────────────────────────────────────────────────────────────────────────

/// Command-line arguments for `log show`. Split out so tests can assert the
/// invocation without spawning a subprocess.
pub(crate) const LOG_SHOW_ARGS: &[&str] = &[
    "show",
    "--predicate",
    "subsystem == \"com.apple.audio\"",
    "--last",
    "5m",
    "--style",
    "compact",
];

/// Collect a diagnostics bundle for the install-failure dialog's
/// **Copy diagnostic info** button.
///
/// Captures the last 5 minutes of the `com.apple.audio` subsystem log and
/// appends the last known osascript exit code from `install_driver`. No PII
/// beyond device names/UIDs and process paths already present in the log.
#[tauri::command]
pub async fn collect_coreaudio_diagnostics() -> Result<String, String> {
    let log_output = tokio::task::spawn_blocking(|| {
        Command::new("log").args(LOG_SHOW_ARGS).output()
    })
    .await
    .map_err(|e| format!("log show task join failed: {e}"))?
    .map_err(|e| format!("log show spawn failed: {e}"))?;

    let stdout = String::from_utf8_lossy(&log_output.stdout);
    let stderr = String::from_utf8_lossy(&log_output.stderr);

    let mut out = String::new();
    out.push_str("=== Klaar diagnostic info ===\n");
    out.push_str(&format!(
        "last osascript exit code: {}\n",
        match last_osascript_exit() {
            Some(code) => code.to_string(),
            None => "n/a".to_string(),
        }
    ));
    out.push_str(&format!("log show exit: {}\n", log_output.status));
    out.push_str("\n=== log show --predicate 'subsystem == \"com.apple.audio\"' --last 5m ===\n");
    out.push_str(&stdout);
    if !stderr.trim().is_empty() {
        out.push_str("\n=== stderr ===\n");
        out.push_str(&stderr);
    }
    Ok(out)
}

// ────────────────────────────────────────────────────────────────────────────
// (read_update_cache / write_update_cache removed — the GitHub release
//  update check is now Rust-owned via `update_check.rs`. The cache file
//  `update-check.json` is read/written from there directly; the webview
//  no longer participates. See the `app-update-check` capability spec.)
// ────────────────────────────────────────────────────────────────────────────
// read_onboarding_state / write_onboarding_state
//
// File-backed persistence for onboarding flags that must survive app
// quit-and-relaunch but be cleared by an actual system reboot. Currently the
// only key is `rebootPending`, populated by the post-install RebootRequired
// dialog. The frontend pairs this store with `get_boot_timestamp` to detect
// reboots — see `src/state/onboardingPersistedStore.ts`.
//
// Mirrors the `read_update_cache` / `write_update_cache` shape so the
// frontend can use the same IPC idiom (no `tauri-plugin-fs` dependency).
// ────────────────────────────────────────────────────────────────────────────

/// Persisted record of "user clicked Later in the RebootRequired dialog".
/// Cleared automatically when `bootTimestamp` no longer matches the current
/// `kern.boottime` (i.e., the user actually rebooted).
///
/// `value` is always `true` when present — the field is retained for forward
/// compatibility (room to add `false`-with-reason states later) and to match
/// the IPC contract enforced by the integration tests in this module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebootPending {
    pub value: bool,
    #[serde(rename = "bootTimestamp")]
    pub boot_timestamp: i64,
}

/// Marker for one-shot UI flags persisted across launches but not tied to OS
/// boot state (unlike `RebootPending`). Currently used for
/// `pendingPostInstallApps`, which fires the conferencing-apps screen exactly
/// once after a fresh install + reboot cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneShotFlag {
    pub value: bool,
}

/// Top-level shape of `onboarding-state.json`. New keys can be added without
/// breaking older builds because every field is `#[serde(default)]`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OnboardingPersistedState {
    #[serde(
        default,
        rename = "rebootPending",
        skip_serializing_if = "Option::is_none"
    )]
    pub reboot_pending: Option<RebootPending>,
    #[serde(
        default,
        rename = "pendingPostInstallApps",
        skip_serializing_if = "Option::is_none"
    )]
    pub pending_post_install_apps: Option<OneShotFlag>,
}

fn onboarding_state_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    app.path()
        .resolve("onboarding-state.json", BaseDirectory::AppData)
        .map_err(|e| format!("resolve app data dir failed: {e}"))
}

/// Read `onboarding-state.json`. Returns `Ok(None)` when the file does not
/// exist (fresh install / never written) and `Err` for I/O or JSON errors so
/// the frontend can surface a corrupt-state condition rather than silently
/// fall through to "no reboot pending".
#[tauri::command]
pub fn read_onboarding_state<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<OnboardingPersistedState>, String> {
    let path = onboarding_state_path(&app)?;
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("read onboarding state: {e}"))?;
    let state: OnboardingPersistedState =
        serde_json::from_slice(&bytes).map_err(|e| format!("parse onboarding state: {e}"))?;
    Ok(Some(state))
}

/// Atomically replace `onboarding-state.json` with `state`. The parent app-
/// data directory is created if missing (matches `write_update_cache`).
#[tauri::command]
pub fn write_onboarding_state<R: Runtime>(
    app: AppHandle<R>,
    state: OnboardingPersistedState,
) -> Result<(), String> {
    let path = onboarding_state_path(&app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir app data dir: {e}"))?;
    }
    let bytes =
        serde_json::to_vec_pretty(&state).map_err(|e| format!("serialize onboarding state: {e}"))?;
    std::fs::write(&path, bytes).map_err(|e| format!("write onboarding state: {e}"))?;
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// set_reboot_pending — engine-start gate sync
// ────────────────────────────────────────────────────────────────────────────

/// Toggle the in-memory `reboot_pending` flag on `AudioEngineState`.
///
/// The frontend invokes this from `state/onboardingPersistedStore.ts` every
/// time it writes / clears the file-backed store, so the next
/// `try_start_engine` call gates correctly without re-reading the file.
///
/// Idempotent — repeated calls with the same value are no-ops. Stays in lock-
/// step with the file-backed store: the file is the source of truth at startup
/// (see `seed_reboot_pending_from_disk`), the atomic is the source of truth at
/// runtime.
#[tauri::command]
pub fn set_reboot_pending<R: Runtime>(
    pending: bool,
    app: AppHandle<R>,
    handle: tauri::State<'_, EngineHandle>,
) -> Result<(), String> {
    handle.state.set_reboot_pending(pending);
    log::debug!("set_reboot_pending({pending})");
    // Re-resolve the tray immediately so the yellow reboot-pending overlay
    // (and its tooltip) appears / disappears without waiting for the next
    // engine-state-driven update.
    crate::tray::refresh_tray_for_reboot_pending_change(&app);
    Ok(())
}

/// On app startup, read `onboarding-state.json` and seed the in-memory
/// `reboot_pending` atomic so the engine gate fires before the frontend has
/// a chance to round-trip through `set_reboot_pending`.
///
/// Resolution rules (mirror `getRebootPending` in
/// `src/state/onboardingPersistedStore.ts`):
///   - File missing / corrupt / no `rebootPending` key → seed `false`.
///   - `rebootPending` present AND boot timestamp matches `kern.boottime`
///     → seed `true`.
///   - `rebootPending` present AND boot timestamp differs (real reboot
///     happened) → seed `false`. The frontend's `getRebootPending()` will
///     wipe the stale file entry on its first call; we don't touch the file
///     here to keep the responsibilities single-owner.
///   - `kern.boottime` unreadable (returns 0) → seed `true` conservatively,
///     matching the documented frontend contract.
pub fn seed_reboot_pending_from_disk<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Ok(Some(state)) = read_onboarding_state(app.clone()) else {
        return false;
    };
    let Some(rp) = state.reboot_pending else {
        return false;
    };
    if !rp.value {
        return false;
    }
    let current_boot = boot_timestamp_secs();
    if current_boot == 0 {
        // sysctl unavailable — be conservative.
        return true;
    }
    rp.boot_timestamp == current_boot
}

// ────────────────────────────────────────────────────────────────────────────
// get_min_driver_version
// ────────────────────────────────────────────────────────────────────────────

/// Exposes [`MIN_DRIVER_VERSION`] to the frontend so the version-gate can
/// compare without hard-coding the constant in two places.
#[tauri::command]
pub fn get_min_driver_version() -> &'static str {
    MIN_DRIVER_VERSION
}

// ────────────────────────────────────────────────────────────────────────────
// get_app_version — running binary version, sourced from package_info
// ────────────────────────────────────────────────────────────────────────────

/// Return the running binary's version string from `tauri::PackageInfo`.
///
/// The About dialog calls this once on first open and caches the result for
/// the lifetime of the session. Sourcing it from `package_info` (rather than
/// importing `package.json` at build time) means the value reflects what the
/// user is actually running — frontend and backend can never drift.
///
/// Independent of engine state — never blocks, never errors.
#[tauri::command]
pub fn get_app_version<R: Runtime>(app: AppHandle<R>) -> String {
    app.package_info().version.to_string()
}

// Silence unused-import warning on non-mac builds (the Duration import is
// only needed when we later wire a timeout for `log show`).
#[allow(dead_code)]
const _LOG_SHOW_TIMEOUT: Duration = Duration::from_secs(15);

// ────────────────────────────────────────────────────────────────────────────
// TCC microphone recovery — Privacy Settings deep-link + status probe
// ────────────────────────────────────────────────────────────────────────────

/// `x-apple.systempreferences://` deep-link to the Microphone privacy pane.
///
/// Apple changed the host segment when System Preferences was replaced by
/// System Settings in macOS 13 (Ventura). The legacy
/// `com.apple.preference.security` anchor still parses on modern macOS but
/// silently lands on whichever pane was last open — the symptom reported on
/// macOS 15 / 26. The new
/// `com.apple.settings.PrivacySecurity.extension` host is what System
/// Settings actually exposes; it is documented in the community macOS 26
/// Tahoe URL inventory (paralevel/macos-settings-urls) and works on every
/// release from Ventura onward.
///
/// `tauri.conf.json` still allows `minimumSystemVersion: 12.0` (Monterey),
/// so we keep the legacy URL as a fallback for that one case and pick at
/// runtime via [`macos_major_version`].
const PRIVACY_MICROPHONE_URL_LEGACY: &str =
    "x-apple.systempreferences://com.apple.preference.security?Privacy_Microphone";
const PRIVACY_MICROPHONE_URL_MODERN: &str =
    "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_Microphone";

/// Read the macOS major version (e.g. `13` for Ventura, `15` for Sequoia,
/// `26` for Tahoe) via `sw_vers -productVersion`. Returns `None` if the
/// command is unavailable or the output cannot be parsed — callers then
/// pick the safer default. Cheap to call (a single fork+exec) but never on
/// the audio thread.
fn macos_major_version() -> Option<u32> {
    let output = Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    stdout.trim().split('.').next()?.parse().ok()
}

/// Pick the System Settings deep-link URL that matches the running OS.
/// macOS 13+ uses the new `PrivacySecurity.extension` host; older releases
/// (only 12 / Monterey is in scope per `tauri.conf.json`) fall back to the
/// legacy `preference.security` host. If the version probe fails we prefer
/// the modern URL — Monterey usage is vanishingly rare and the modern URL
/// at worst no-ops, while the legacy URL is the one we know is broken on
/// shipping macOS.
fn privacy_microphone_url() -> &'static str {
    match macos_major_version() {
        Some(v) if v < 13 => PRIVACY_MICROPHONE_URL_LEGACY,
        _ => PRIVACY_MICROPHONE_URL_MODERN,
    }
}

/// Open System Settings directly on the Privacy → Microphone pane. Invoked
/// by the persistent-denied banner's **Open Privacy Settings** button.
///
/// Uses `/usr/bin/open` (same pattern as other macOS helpers in this module).
/// No admin prompt; no `coreaudiod` restart.
#[tauri::command]
pub fn open_privacy_microphone_settings() -> Result<(), String> {
    let url = privacy_microphone_url();
    let status = Command::new("open")
        .arg(url)
        .status()
        .map_err(|e| format!("failed to spawn `open`: {e}"))?;
    if !status.success() {
        return Err(format!(
            "`open` exited with status {}",
            status.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

/// Read-only TCC microphone status probe. Never prompts the user — this is
/// the command the frontend focus-hook invokes to detect a regrant. Prompting
/// (via `requestAccess`) happens only inside `try_start_engine`.
///
/// Returns one of `"authorized"`, `"not_determined"`, `"denied"`,
/// `"restricted"` (lowercase, snake_case, stable IPC contract).
#[tauri::command]
pub fn get_microphone_authorization_status() -> String {
    use crate::macos_permissions::{check_status, MicPermissionStatus};
    match check_status() {
        MicPermissionStatus::Authorized => "authorized",
        MicPermissionStatus::NotDetermined => "not_determined",
        MicPermissionStatus::Denied => "denied",
        MicPermissionStatus::Restricted => "restricted",
    }
    .to_string()
}

// ────────────────────────────────────────────────────────────────────────────
// report_frontend_error — frontend diagnostics
// ────────────────────────────────────────────────────────────────────────────

/// Maximum byte length for any single string field on a `FrontendErrorPayload`.
/// Prevents a runaway error loop from filling the log with multi-MB stack
/// traces. Matches the spec for `onboarding-resilience::report_frontend_error`.
const FRONTEND_ERROR_MAX_FIELD_BYTES: usize = 4096;
const FRONTEND_ERROR_TRUNCATION_SUFFIX: &str = "…[truncated]";

#[derive(Debug, Deserialize)]
pub struct FrontendErrorPayload {
    pub component: String,
    pub message: String,
    #[serde(default)]
    pub stack: Option<String>,
    #[serde(default)]
    pub context: Option<serde_json::Value>,
}

/// Truncate a string to at most `max_bytes` bytes, respecting UTF-8 char
/// boundaries, and append a fixed suffix if any truncation happened. Pure;
/// unit-tested below.
fn clamp_field(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_string();
    }
    // Walk back from the byte cap to the nearest char boundary.
    let mut cut = max_bytes;
    while cut > 0 && !input.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = String::with_capacity(cut + FRONTEND_ERROR_TRUNCATION_SUFFIX.len());
    out.push_str(&input[..cut]);
    out.push_str(FRONTEND_ERROR_TRUNCATION_SUFFIX);
    out
}

/// Receive a structured render-error report from the frontend's
/// `OnboardingErrorBoundary` (or any other component that opts in). Writes a
/// single WARN-level log entry with a stable `frontend-error` prefix so that
/// `log show --predicate 'subsystem == "com.apple.audio"' --last 5m` is not
/// the only diagnostic surface available when the install-failure flow goes
/// wrong.
///
/// Input clamping protects the log file from runaway error loops. Clipped
/// fields are still logged with a `…[truncated]` suffix so the truncation is
/// visible in the bug report.
#[tauri::command]
pub fn report_frontend_error(payload: FrontendErrorPayload) -> Result<(), String> {
    if payload.component.is_empty() {
        return Err("missing required field: component".to_string());
    }
    if payload.message.is_empty() {
        return Err("missing required field: message".to_string());
    }
    let component = clamp_field(&payload.component, FRONTEND_ERROR_MAX_FIELD_BYTES);
    let message = clamp_field(&payload.message, FRONTEND_ERROR_MAX_FIELD_BYTES);
    let stack = payload
        .stack
        .as_deref()
        .map(|s| clamp_field(s, FRONTEND_ERROR_MAX_FIELD_BYTES));
    let context_json = payload
        .context
        .as_ref()
        .map(|v| serde_json::to_string(v).unwrap_or_else(|e| format!("<context-serialise-error: {e}>")));
    let stack_for_log = stack.as_deref().unwrap_or("<none>");
    let context_for_log = context_json.as_deref().unwrap_or("<none>");
    log::warn!(
        target: "frontend-error",
        "component={component:?} message={message:?} stack={stack_for_log:?} context={context_for_log:?}"
    );
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_microphone_urls_match_spec() {
        // These URLs are the IPC contract — the persistent-denied banner's
        // "Open Privacy Settings" button relies on them opening the exact
        // Privacy → Microphone pane on every supported macOS release.
        // Locking them here catches silent typos and regressions like the
        // case where the legacy host stopped routing to the right pane on
        // macOS 13+ and we had to add the `PrivacySecurity.extension` host.
        assert_eq!(
            PRIVACY_MICROPHONE_URL_LEGACY,
            "x-apple.systempreferences://com.apple.preference.security?Privacy_Microphone"
        );
        assert_eq!(
            PRIVACY_MICROPHONE_URL_MODERN,
            "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_Microphone"
        );
    }

    #[test]
    fn privacy_microphone_url_picks_modern_on_macos_13_plus() {
        // Smoke-check the selector: on any test runner shipped after the
        // Ventura cutover (i.e. all supported CI runners) we must pick the
        // modern URL. If `sw_vers` is unavailable we still default modern.
        let url = privacy_microphone_url();
        assert!(
            url == PRIVACY_MICROPHONE_URL_LEGACY || url == PRIVACY_MICROPHONE_URL_MODERN,
            "selector returned an unknown URL: {url}"
        );
        if let Some(v) = macos_major_version() {
            if v >= 13 {
                assert_eq!(url, PRIVACY_MICROPHONE_URL_MODERN);
            } else {
                assert_eq!(url, PRIVACY_MICROPHONE_URL_LEGACY);
            }
        }
    }

    #[test]
    fn log_show_args_match_spec() {
        // Matches the command line in design D5.
        assert_eq!(
            LOG_SHOW_ARGS,
            &[
                "show",
                "--predicate",
                "subsystem == \"com.apple.audio\"",
                "--last",
                "5m",
                "--style",
                "compact",
            ]
        );
    }

    // (update_cache tests removed alongside read_update_cache /
    //  write_update_cache — see `update_check.rs` for the new owner's tests.)

    // ────────────────────────────────────────────────────────────────────
    // report_frontend_error — frontend diagnostics
    // ────────────────────────────────────────────────────────────────────

    #[test]
    fn clamp_field_passthrough_when_under_cap() {
        let s = "short";
        let out = clamp_field(s, FRONTEND_ERROR_MAX_FIELD_BYTES);
        assert_eq!(out, "short");
    }

    #[test]
    fn clamp_field_truncates_long_input_and_adds_suffix() {
        let s = "a".repeat(FRONTEND_ERROR_MAX_FIELD_BYTES + 100);
        let out = clamp_field(&s, FRONTEND_ERROR_MAX_FIELD_BYTES);
        assert!(
            out.len() <= FRONTEND_ERROR_MAX_FIELD_BYTES + FRONTEND_ERROR_TRUNCATION_SUFFIX.len()
        );
        assert!(out.ends_with(FRONTEND_ERROR_TRUNCATION_SUFFIX));
        // Stack content preserved up to the boundary.
        assert!(out.starts_with("aaaa"));
    }

    #[test]
    fn clamp_field_respects_utf8_char_boundaries() {
        // 4 bytes per `🎧` × 1100 = 4400 bytes; cap at 4096 forces a cut
        // that lands mid-char unless the helper walks back.
        let s = "🎧".repeat(1100);
        let out = clamp_field(&s, FRONTEND_ERROR_MAX_FIELD_BYTES);
        // No char boundary panic implied: all bytes before suffix must form
        // a valid prefix of the input.
        let prefix = out
            .strip_suffix(FRONTEND_ERROR_TRUNCATION_SUFFIX)
            .expect("expected truncation suffix");
        assert!(s.starts_with(prefix));
        assert!(prefix.len() <= FRONTEND_ERROR_MAX_FIELD_BYTES);
    }

    #[test]
    fn report_frontend_error_happy_path_accepts_well_formed_payload() {
        let payload = FrontendErrorPayload {
            component: "OnboardingSurface".to_string(),
            message: "Cannot read property 'x' of undefined".to_string(),
            stack: Some("at Foo (foo.tsx:1:1)".to_string()),
            context: Some(serde_json::json!({ "screen": "install-failure" })),
        };
        // Should not panic and should return Ok.
        assert!(report_frontend_error(payload).is_ok());
    }

    #[test]
    fn report_frontend_error_rejects_empty_component() {
        let payload = FrontendErrorPayload {
            component: "".to_string(),
            message: "boom".to_string(),
            stack: None,
            context: None,
        };
        let err = report_frontend_error(payload).unwrap_err();
        assert!(err.contains("component"));
    }

    #[test]
    fn report_frontend_error_rejects_empty_message() {
        let payload = FrontendErrorPayload {
            component: "Foo".to_string(),
            message: "".to_string(),
            stack: None,
            context: None,
        };
        let err = report_frontend_error(payload).unwrap_err();
        assert!(err.contains("message"));
    }

    #[test]
    fn report_frontend_error_clamps_oversized_stack() {
        // 8 KiB stack — should clamp to ≤ 4096 + suffix.
        let big_stack = "x".repeat(8192);
        let payload = FrontendErrorPayload {
            component: "Foo".to_string(),
            message: "msg".to_string(),
            stack: Some(big_stack),
            context: None,
        };
        // We can't easily inspect the log output here without wiring a test
        // logger; the clamp itself is unit-tested via `clamp_field` above.
        // This test asserts the command does not panic on oversized input.
        assert!(report_frontend_error(payload).is_ok());
    }

    #[test]
    fn frontend_error_payload_deserialises_optional_fields() {
        // Round-trip via the same shape the frontend sends.
        let raw = r#"{"component":"X","message":"y"}"#;
        let parsed: FrontendErrorPayload = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.component, "X");
        assert_eq!(parsed.message, "y");
        assert!(parsed.stack.is_none());
        assert!(parsed.context.is_none());
    }

    // ────────────────────────────────────────────────────────────────────
    // RebootError serialisation — IPC contract with the frontend
    // ────────────────────────────────────────────────────────────────────

    #[test]
    fn reboot_error_user_cancelled_json_shape() {
        let json = serde_json::to_value(RebootError::UserCancelled).unwrap();
        assert_eq!(json, serde_json::json!({ "kind": "UserCancelled" }));
    }

    #[test]
    fn reboot_error_osascript_failed_json_shape() {
        let json =
            serde_json::to_value(RebootError::OsascriptFailed("boom".to_string())).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "kind": "OsascriptFailed", "message": "boom" })
        );
    }

    // ────────────────────────────────────────────────────────────────────
    // get_boot_timestamp — sysctl kern.boottime
    // ────────────────────────────────────────────────────────────────────

    /// On macOS the boot timestamp must be a positive integer that does not
    /// drift across consecutive calls (other than by an actual reboot, which
    /// won't happen in the middle of a test run).
    #[cfg(target_os = "macos")]
    #[test]
    fn boot_timestamp_is_positive_and_stable() {
        let a = boot_timestamp_secs();
        assert!(a > 0, "expected positive boot timestamp, got {a}");
        let b = boot_timestamp_secs();
        assert_eq!(a, b, "boot timestamp drifted between calls");
    }

    // ────────────────────────────────────────────────────────────────────
    // OnboardingPersistedState — IPC contract for read/write_onboarding_state
    // ────────────────────────────────────────────────────────────────────

    #[test]
    fn onboarding_state_round_trips_with_reboot_pending() {
        let state = OnboardingPersistedState {
            reboot_pending: Some(RebootPending {
                value: true,
                boot_timestamp: 1_700_000_000,
            }),
            pending_post_install_apps: None,
        };
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"rebootPending\""));
        assert!(json.contains("\"bootTimestamp\":1700000000"));
        assert!(json.contains("\"value\":true"));

        let parsed: OnboardingPersistedState = serde_json::from_str(&json).unwrap();
        let rp = parsed.reboot_pending.expect("rebootPending should round-trip");
        assert!(rp.value);
        assert_eq!(rp.boot_timestamp, 1_700_000_000);
    }

    #[test]
    fn onboarding_state_round_trips_with_pending_post_install_apps() {
        let state = OnboardingPersistedState {
            reboot_pending: None,
            pending_post_install_apps: Some(OneShotFlag { value: true }),
        };
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"pendingPostInstallApps\""));
        assert!(!json.contains("\"rebootPending\""));

        let parsed: OnboardingPersistedState = serde_json::from_str(&json).unwrap();
        let flag = parsed
            .pending_post_install_apps
            .expect("pendingPostInstallApps should round-trip");
        assert!(flag.value);
    }

    #[test]
    fn onboarding_state_empty_object_means_no_reboot_pending() {
        // Forward-compat: an unknown future key should not break parsing, and
        // an empty object means "nothing pending".
        let parsed: OnboardingPersistedState = serde_json::from_str("{}").unwrap();
        assert!(parsed.reboot_pending.is_none());
        assert!(parsed.pending_post_install_apps.is_none());
    }

    #[test]
    fn onboarding_state_omits_reboot_pending_when_none() {
        // skip_serializing_if = Option::is_none — keeps the on-disk file
        // free of `"rebootPending":null` noise.
        let state = OnboardingPersistedState::default();
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, "{}");
    }

    #[test]
    fn onboarding_state_rejects_corrupt_json() {
        // The frontend store treats parse errors as "corrupt — clear and
        // start fresh" (see persistence.test.ts). Lock in the error path
        // here so the IPC contract stays explicit.
        let parsed: Result<OnboardingPersistedState, _> = serde_json::from_str("{not json");
        assert!(parsed.is_err());
    }

    /// Compile-time smoke test that `get_app_version` is a valid Tauri
    /// command with the documented signature `(AppHandle) -> String`. If
    /// the command's signature ever drifts (e.g. someone makes it `async`
    /// or removes the `AppHandle` parameter), this binding fails to type-
    /// check and the test refuses to compile, surfacing the regression
    /// without needing to spin up a `MockRuntime`. The actual `invoke_handler`
    /// wiring lives in `lib.rs::run`.
    #[test]
    fn get_app_version_is_a_valid_tauri_command() {
        fn assert_signature<R: tauri::Runtime>(
            _f: fn(tauri::AppHandle<R>) -> String,
        ) {
        }
        assert_signature::<tauri::Wry>(get_app_version);
    }

    #[test]
    fn frontend_error_payload_rejects_missing_required_field() {
        // No `component` key at all — should fail to deserialise.
        let raw = r#"{"message":"y"}"#;
        let parsed: Result<FrontendErrorPayload, _> = serde_json::from_str(raw);
        assert!(parsed.is_err());
    }
}
