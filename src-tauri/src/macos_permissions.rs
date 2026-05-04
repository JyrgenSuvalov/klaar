//! macOS TCC permission gate for the audio engine.
//!
//! This module exposes the proactive microphone authorization check invoked
//! by `try_start_engine` before the input `AudioUnit` is opened. It fixes the
//! release-only bug where, on macOS 12+ (especially Sonoma/Sequoia), an
//! un-prompted app gets `AudioUnit.open`/`start` → `noErr` and the render
//! callback silently delivers zero-filled buffers.
//!
//! ## Architecture
//!
//! Two layers, kept separate on purpose:
//!
//! 1. **FFI layer** (`check_status`, `request_access`) — wraps AVFoundation.
//!    Not unit-tested (would require mocking Objective-C runtime); covered
//!    by manual fresh-install QA scenarios.
//! 2. **Pure decision layer** (`decide_action`, `classify_request_result`) —
//!    plain Rust functions, fully unit-tested. Every branch the orchestrator
//!    can take is exercised here without invoking AVFoundation.
//!
//! The orchestrator (`ensure_microphone_permission`) composes the two.
//!
//! ## Threading
//!
//! `requestAccessForMediaType:completionHandler:` invokes its completion
//! block on an arbitrary dispatch queue. We bridge it to Tokio via a
//! `tokio::sync::oneshot` channel — the async call site awaits the receiver.
//! Callable from any Tokio task; Tauri already owns the `NSApplication` main
//! thread that AVFoundation requires for prompt presentation.

/// Mirror of `AVAuthorizationStatus` for audio media.
///
/// Kept as a plain Rust enum so the pure decision layer can be tested without
/// depending on the AVFoundation bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicPermissionStatus {
    /// `AVAuthorizationStatusAuthorized` — the user has granted access.
    Authorized,
    /// `AVAuthorizationStatusNotDetermined` — no decision yet; prompting is
    /// possible.
    NotDetermined,
    /// `AVAuthorizationStatusDenied` — the user has explicitly denied; only
    /// recoverable via System Settings.
    Denied,
    /// `AVAuthorizationStatusRestricted` — denied by policy (e.g. parental
    /// controls / MDM); not user-recoverable from within the app.
    Restricted,
}

/// What the gate should do given a status. Keeps the orchestrator trivial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicGateAction {
    /// Permission is already granted — proceed with engine start.
    Proceed,
    /// Permission has not yet been decided — request access and re-evaluate.
    Prompt,
    /// Permission is denied/restricted — fail fast with the persistent error.
    Deny,
}

/// Error string returned when permission is denied. Matches the
/// `EngineError` variant surfaced to the frontend (`parseEngineError` in
/// `src/store/engineStore.ts`).
pub const PERMISSION_DENIED_PERSISTENT: &str = "microphone_permission_denied_persistent";

// ────────────────────────────────────────────────────────────────────────────
// Pure decision layer
// ────────────────────────────────────────────────────────────────────────────

/// Map an authorization status to a gate action.
pub fn decide_action(status: MicPermissionStatus) -> MicGateAction {
    match status {
        MicPermissionStatus::Authorized => MicGateAction::Proceed,
        MicPermissionStatus::NotDetermined => MicGateAction::Prompt,
        MicPermissionStatus::Denied | MicPermissionStatus::Restricted => MicGateAction::Deny,
    }
}

/// Map the boolean result of `requestAccess` to a gate action.
pub fn classify_request_result(granted: bool) -> MicGateAction {
    if granted {
        MicGateAction::Proceed
    } else {
        MicGateAction::Deny
    }
}

// ────────────────────────────────────────────────────────────────────────────
// FFI layer (macOS only)
// ────────────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod ffi {
    use super::MicPermissionStatus;
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
    use tokio::sync::oneshot;

    /// `AVMediaTypeAudio` is declared as `Option<&NSString>` in the bindings.
    /// In practice the constant is always non-null on every supported macOS
    /// version (it has existed since 10.7). `expect` would be acceptable; we
    /// prefer a defensive fallback so a corrupt AVFoundation runtime does not
    /// panic the engine-start path.
    fn audio_media_type() -> &'static objc2_foundation::NSString {
        // SAFETY: `AVMediaTypeAudio` is an AVFoundation extern static. It is
        // initialised by dyld on framework load and is immutable for the
        // lifetime of the process. Safe to read from any thread.
        let value = unsafe { AVMediaTypeAudio };
        value.expect("AVMediaTypeAudio framework constant missing")
    }

    /// Query `AVCaptureDevice.authorizationStatus(for: .audio)`. Cheap —
    /// synchronous, no I/O. Safe to call on any thread.
    pub fn check_status() -> MicPermissionStatus {
        // SAFETY: `AVMediaTypeAudio` is a static framework constant, and
        // `authorizationStatusForMediaType:` is a class-level query with no
        // thread affinity. Return value is a plain `NSInteger`.
        let status = unsafe {
            AVCaptureDevice::authorizationStatusForMediaType(audio_media_type())
        };
        map_status(status)
    }

    /// Request audio media access, awaiting the completion block on a Tokio
    /// oneshot channel. Resolves to `true` on grant, `false` on denial.
    pub async fn request_access() -> bool {
        let (tx, rx) = oneshot::channel::<bool>();
        // Block construction + FFI call is scoped so `RcBlock` (which is
        // `!Send`) is dropped before any await. AVFoundation internally
        // `Block_copy`s the handler on registration, so our local reference
        // can be released immediately after the call returns — the copy it
        // holds keeps the closure alive until the completion handler fires.
        // This is what lets the outer `Future` be `Send`, which Tauri async
        // commands require.
        {
            // Move the sender into a Mutex<Option<_>> so the Fn closure
            // (called at most once by AVFoundation) can take it by value on
            // first fire.
            let tx_cell = std::sync::Mutex::new(Some(tx));
            let block = RcBlock::new(move |granted: Bool| {
                if let Some(sender) = tx_cell.lock().ok().and_then(|mut g| g.take()) {
                    // Receiver may have been dropped if the caller bailed —
                    // ignore the send error in that case.
                    let _ = sender.send(granted.as_bool());
                }
            });
            // SAFETY: `AVMediaTypeAudio` is a static framework constant.
            // AVFoundation retains the block via `Block_copy` internally;
            // dropping our `RcBlock` at scope end is safe.
            unsafe {
                AVCaptureDevice::requestAccessForMediaType_completionHandler(
                    audio_media_type(),
                    &block,
                );
            }
        }
        // If the channel closes without a value (shouldn't happen —
        // AVFoundation always invokes the handler), treat as denial to
        // fail safely.
        rx.await.unwrap_or(false)
    }

    fn map_status(status: AVAuthorizationStatus) -> MicPermissionStatus {
        // AVAuthorizationStatus is a raw integer enum. Match by discriminant
        // to avoid coupling to objc2's Debug formatting.
        match status.0 {
            0 => MicPermissionStatus::NotDetermined,
            1 => MicPermissionStatus::Restricted,
            2 => MicPermissionStatus::Denied,
            3 => MicPermissionStatus::Authorized,
            // Unknown variant — treat defensively as Denied so the user sees
            // the recovery banner rather than silent zeros.
            _ => MicPermissionStatus::Denied,
        }
    }
}

#[cfg(target_os = "macos")]
pub use ffi::{check_status, request_access};

// Non-macOS stubs so `cargo test` / `cargo clippy` work on other hosts in CI.
// Production builds only target macOS.
#[cfg(not(target_os = "macos"))]
pub fn check_status() -> MicPermissionStatus {
    MicPermissionStatus::Authorized
}

#[cfg(not(target_os = "macos"))]
pub async fn request_access() -> bool {
    true
}

// ────────────────────────────────────────────────────────────────────────────
// Orchestrator
// ────────────────────────────────────────────────────────────────────────────

/// Ensure microphone permission is granted before engine start. Called at the
/// top of `try_start_engine` before any CoreAudio work.
///
/// - `Ok(())` — permission is granted; proceed to open the input AudioUnit.
/// - `Err(PERMISSION_DENIED_PERSISTENT)` — denied or restricted; caller must
///   surface the persistent error variant and NOT open any CoreAudio
///   resources.
///
/// `on_prompt_resolved` is invoked exactly once iff the TCC prompt was
/// actually shown to the user (i.e. the `NotDetermined` branch was taken),
/// regardless of whether the user clicked Allow or Don't Allow. Used by the
/// caller to restore window focus — macOS gives focus back to the previously
/// frontmost app once the system prompt closes, leaving Klaar buried behind
/// other windows. Not invoked on already-Authorized or
/// already-Denied paths because no prompt is shown there and the window
/// keeps focus naturally.
pub async fn ensure_microphone_permission<F: FnOnce()>(
    on_prompt_resolved: F,
) -> Result<(), String> {
    match decide_action(check_status()) {
        MicGateAction::Proceed => {
            log::debug!("mic_permission: already authorized");
            Ok(())
        }
        MicGateAction::Deny => {
            log::warn!("mic_permission: proactive gate denied (persistent)");
            Err(PERMISSION_DENIED_PERSISTENT.to_string())
        }
        MicGateAction::Prompt => {
            log::info!("mic_permission: prompting user (TCC request)");
            let granted = request_access().await;
            on_prompt_resolved();
            match classify_request_result(granted) {
                MicGateAction::Proceed => {
                    log::info!("mic_permission: user granted access");
                    Ok(())
                }
                // Only Deny is reachable here; Prompt would loop forever.
                _ => {
                    log::warn!("mic_permission: user denied access (persistent)");
                    Err(PERMISSION_DENIED_PERSISTENT.to_string())
                }
            }
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests — pure decision layer only
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decide_action_authorized_proceeds() {
        assert_eq!(
            decide_action(MicPermissionStatus::Authorized),
            MicGateAction::Proceed
        );
    }

    #[test]
    fn decide_action_not_determined_prompts() {
        assert_eq!(
            decide_action(MicPermissionStatus::NotDetermined),
            MicGateAction::Prompt
        );
    }

    #[test]
    fn decide_action_denied_is_deny() {
        assert_eq!(
            decide_action(MicPermissionStatus::Denied),
            MicGateAction::Deny
        );
    }

    #[test]
    fn decide_action_restricted_is_deny() {
        assert_eq!(
            decide_action(MicPermissionStatus::Restricted),
            MicGateAction::Deny
        );
    }

    #[test]
    fn classify_request_result_true_proceeds() {
        assert_eq!(classify_request_result(true), MicGateAction::Proceed);
    }

    #[test]
    fn classify_request_result_false_denies() {
        assert_eq!(classify_request_result(false), MicGateAction::Deny);
    }

    #[test]
    fn permission_denied_persistent_matches_frontend_contract() {
        // This string is the exact IPC error payload the frontend's
        // `parseEngineError` looks for. Breaking it silently would lose the
        // UI banner. Lock the contract here.
        assert_eq!(
            PERMISSION_DENIED_PERSISTENT,
            "microphone_permission_denied_persistent"
        );
    }
}
