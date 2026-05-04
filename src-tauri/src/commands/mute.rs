//! Global mute — shared toggle/set/get logic and IPC commands.
//!
//! This module is the single chokepoint that the hotkey, the tray menu, and
//! the IPC layer all funnel through. It owns:
//!
//!   1. The `0.0`↔`1.0` mapping over `DspParams::mute_target` (kept here so
//!      callers never duplicate it).
//!   2. The `klaar://mute-changed` event emission — every state-changing
//!      entry point fires it so subscribers (tray, frontend) stay in sync
//!      without polling.
//!
//! The audio thread reads `DspParams::mute_target` directly and one-pole-
//! smooths a private `mute_current` toward it (see `dsp::ProcessorChain`).
//! Nothing in this module touches the audio thread.
//!
//! Mute is **not** persisted across launches and **not** part of profile
//! save/load — see `DspParams::get_all_params` / `set_all_params`. Always
//! launches unmuted.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Runtime, State};

use crate::commands::EngineHandle;
use crate::dsp::DspParams;

/// Event name emitted on every mute state change. Payload: `{ muted: bool }`.
pub const MUTE_CHANGED_EVENT: &str = "klaar://mute-changed";

/// JSON payload for [`MUTE_CHANGED_EVENT`]. The `muted` field is the post-
/// change boolean — `true` = silenced.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct MuteChangedPayload {
    pub muted: bool,
}

// ────────────────────────────────────────────────────────────────────────────
// Shared toggle logic — used by IPC, hotkey, and tray
// ────────────────────────────────────────────────────────────────────────────

/// Flip the mute intent and emit `klaar://mute-changed`. Returns the new
/// boolean state (`true` = muted).
///
/// Safe to call from any non-audio thread. Atomic write is `Relaxed`; the
/// audio thread sees the new target on its next sample-loop iteration.
pub fn toggle_mute_shared<R: Runtime>(app: &AppHandle<R>, dsp: &Arc<DspParams>) -> bool {
    let new_muted = dsp.toggle_muted();
    emit_mute_changed(app, new_muted);
    new_muted
}

/// Write a specific mute state and emit `klaar://mute-changed` (always —
/// idempotent calls still notify so a subscriber that missed the original
/// event resyncs cleanly).
pub fn set_mute_shared<R: Runtime>(app: &AppHandle<R>, dsp: &Arc<DspParams>, muted: bool) -> bool {
    dsp.set_muted(muted);
    emit_mute_changed(app, muted);
    muted
}

/// Current mute boolean — pure read, no side effects.
#[inline]
pub fn get_mute_shared(dsp: &Arc<DspParams>) -> bool {
    dsp.is_muted()
}

fn emit_mute_changed<R: Runtime>(app: &AppHandle<R>, muted: bool) {
    if let Err(e) = app.emit(MUTE_CHANGED_EVENT, MuteChangedPayload { muted }) {
        // Emission failure is non-fatal — log and carry on. The atomic state
        // is already updated; subscribers will resync on the next event or
        // next `get_mute_state` poll.
        log::warn!("failed to emit {MUTE_CHANGED_EVENT}: {e}");
    }
}

// ────────────────────────────────────────────────────────────────────────────
// IPC commands
// ────────────────────────────────────────────────────────────────────────────

/// Flip mute. Returns the new state (`true` = muted).
#[tauri::command]
pub fn toggle_mute<R: Runtime>(
    app: AppHandle<R>,
    handle: State<'_, EngineHandle>,
) -> Result<bool, String> {
    Ok(toggle_mute_shared(&app, &handle.dsp_params))
}

/// Set mute to a specific state. Returns the (echoed) state.
#[tauri::command]
pub fn set_mute<R: Runtime>(
    muted: bool,
    app: AppHandle<R>,
    handle: State<'_, EngineHandle>,
) -> Result<bool, String> {
    Ok(set_mute_shared(&app, &handle.dsp_params, muted))
}

/// Read current mute state (`true` = muted).
#[tauri::command]
pub fn get_mute_state(handle: State<'_, EngineHandle>) -> Result<bool, String> {
    Ok(get_mute_shared(&handle.dsp_params))
}

// ────────────────────────────────────────────────────────────────────────────
// Hotkey-registration warning surfacing
// ────────────────────────────────────────────────────────────────────────────

/// User-facing copy for the hotkey-registration warning. Kept as a `const`
/// so tests can assert the exact string and so the frontend doesn't need
/// to duplicate the wording.
///
/// **Why we don't say "another app is using it":** on macOS the global-shortcut
/// plugin uses Carbon `RegisterEventHotKey`, which silently allows duplicate
/// registrations — the OS just dispatches the event to whichever process was
/// most recently registered. We literally cannot detect a "another app
/// already owns this chord" conflict at registration time. So the warning
/// only surfaces on *hard* registration failures (Accessibility denied,
/// invalid keycode, kernel resource exhaustion) — and in those cases
/// "another app" is the wrong story. Verified empirically with Hammerspoon
/// holding ⌃⇧M: Klaar's `register()` returned `Ok(())` and no banner appeared.
pub const HOTKEY_WARNING_MESSAGE: &str =
    "Couldn't register ⌃⇧M — the mute button still works.";

/// Read-and-clear the hotkey-registration warning flag. Returns
/// `Some(message)` once if the registration failed at startup, then `None`
/// on every subsequent call (within the same process lifetime).
///
/// Atomic semantics: `swap(false, AcqRel)` returns the previous value and
/// resets it in one step — racing window mounts can't both see the warning.
#[tauri::command]
pub fn get_hotkey_warning(handle: State<'_, EngineHandle>) -> Result<Option<&'static str>, String> {
    let was_set = handle
        .hotkey_registration_failed
        .swap(false, std::sync::atomic::Ordering::AcqRel);
    Ok(if was_set { Some(HOTKEY_WARNING_MESSAGE) } else { None })
}

// ────────────────────────────────────────────────────────────────────────────
// Tests — exercise the toggle/set/get helpers against a real `DspParams`.
// Real `DspParams` is the smallest stub we need; constructing it is cheap
// and lets us hit the same atomic paths the audio thread reads.
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_unmuted() {
        let dsp = Arc::new(DspParams::new());
        assert!(!get_mute_shared(&dsp));
    }

    #[test]
    fn set_muted_true_then_false() {
        let dsp = Arc::new(DspParams::new());
        dsp.set_muted(true);
        assert!(get_mute_shared(&dsp));
        dsp.set_muted(false);
        assert!(!get_mute_shared(&dsp));
    }

    #[test]
    fn toggle_flips_state() {
        let dsp = Arc::new(DspParams::new());
        assert!(dsp.toggle_muted()); // false → true
        assert!(get_mute_shared(&dsp));
        assert!(!dsp.toggle_muted()); // true → false
        assert!(!get_mute_shared(&dsp));
    }

    #[test]
    fn mute_target_atomic_holds_exact_bits() {
        // Audio thread reads `mute_target` directly, not `is_muted()`. Lock
        // in that the helpers write exact 0.0 / 1.0 — anything else would
        // break the one-pole snap-to-target invariant in the DSP stage.
        let dsp = Arc::new(DspParams::new());
        dsp.set_muted(true);
        assert_eq!(DspParams::get_f32(&dsp.mute_target), 0.0);
        dsp.set_muted(false);
        assert_eq!(DspParams::get_f32(&dsp.mute_target), 1.0);
    }

    #[test]
    fn mute_changed_event_name_is_stable() {
        // The frontend listens on this exact string. If it ever drifts,
        // every subscriber silently goes deaf — lock it down here.
        assert_eq!(MUTE_CHANGED_EVENT, "klaar://mute-changed");
    }

    #[test]
    fn hotkey_warning_message_is_stable() {
        // The frontend and the spec both pin this exact wording. If anyone
        // edits the copy, this test fails and forces a docs/spec update too.
        assert_eq!(
            HOTKEY_WARNING_MESSAGE,
            "Couldn't register ⌃⇧M — the mute button still works."
        );
    }

    #[test]
    fn hotkey_warning_flag_swap_is_one_shot() {
        // Mirror the swap-on-read semantics that `get_hotkey_warning` relies
        // on. We can't easily construct a `tauri::State<EngineHandle>` in a
        // unit test, so this asserts the flag's atomic contract directly —
        // if `swap` ever stops being read-and-clear, the IPC command's
        // "warn once, then never again" guarantee silently breaks.
        use std::sync::atomic::{AtomicBool, Ordering};
        let flag = AtomicBool::new(true);
        assert!(flag.swap(false, Ordering::AcqRel));
        assert!(!flag.swap(false, Ordering::AcqRel));
        assert!(!flag.load(Ordering::Acquire));
    }

    #[test]
    fn payload_serialises_to_documented_shape() {
        let json = serde_json::to_value(MuteChangedPayload { muted: true }).unwrap();
        assert_eq!(json, serde_json::json!({ "muted": true }));
        let json = serde_json::to_value(MuteChangedPayload { muted: false }).unwrap();
        assert_eq!(json, serde_json::json!({ "muted": false }));
    }
}
