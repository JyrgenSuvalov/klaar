/**
 * File-backed onboarding state store.
 *
 * Persists onboarding flags to
 * `~/Library/Application Support/eu.jyrkki.Klaar/onboarding-state.json`
 * (Tauri's `BaseDirectory::AppData` resolves under the bundle identifier from
 * `tauri.conf.json`, NOT the product name — common gotcha when QA-ing) via
 * the `read_onboarding_state` / `write_onboarding_state` Tauri IPCs (which mirror
 * the `read_update_cache` / `write_update_cache` shape — no `tauri-plugin-fs`
 * dependency).
 *
 * Currently stores a single key: `rebootPending`. The flag survives app
 * quit-and-relaunch but is auto-cleared by a real system reboot, detected by
 * comparing the recorded `kern.boottime` (via `get_boot_timestamp` IPC)
 * against a fresh reading on every gate evaluation.
 *
 * **Failure policy:** every operation fails closed in a way that does not
 * resurrect a stale reboot-pending state.
 *   - Corrupt JSON or read error → treat as "no reboot pending" AND clear
 *     the file so the next launch starts cleanly.
 *   - `get_boot_timestamp` returning `0` (sysctl unavailable) → conservatively
 *     keep the persisted state (per IPC contract documented in `system.rs`).
 *   - Write failures → logged via `console.warn`; the store falls back to
 *     "best effort", matching `updateCheck.ts`.
 */

import { invoke } from "@tauri-apps/api/core";

// ────────────────────────────────────────────────────────────────────────────
// On-disk schema (must stay in sync with Rust `OnboardingPersistedState`)
// ────────────────────────────────────────────────────────────────────────────

interface RebootPendingRecord {
  /**
   * Always `true` when the field is present. Reserved for forward-compat
   * (e.g., a future `false`-with-reason state) without breaking older builds.
   */
  value: true;
  /**
   * `kern.boottime.tv_sec` at the moment the user clicked Later. Compared
   * against a fresh reading to detect actual reboots.
   */
  bootTimestamp: number;
}

interface OneShotFlagRecord {
  value: true;
}

export interface OnboardingPersistedState {
  rebootPending?: RebootPendingRecord;
  /**
   * Set on first-install success (alongside `rebootPending`); cleared by the
   * user clicking Done on the conferencing-apps screen. Survives the post-
   * install reboot — that's the whole point — so the gate can route the user
   * through the screen exactly once after they come back from rebooting.
   *
   * Not set on the update path (`MIN_DRIVER_VERSION` gate); repeat users
   * already configured their conferencing apps the first time around.
   */
  pendingPostInstallApps?: OneShotFlagRecord;
}

// ────────────────────────────────────────────────────────────────────────────
// Internal helpers
// ────────────────────────────────────────────────────────────────────────────

/**
 * Read the on-disk state. Returns an empty object when:
 *   - the file does not exist (fresh install / never written), OR
 *   - the file is corrupt — in which case the file is also wiped so
 *     subsequent reads start cleanly.
 */
async function readState(): Promise<OnboardingPersistedState> {
  try {
    const state = await invoke<OnboardingPersistedState | null>(
      "read_onboarding_state",
    );
    return state ?? {};
  } catch (e) {
    // Most likely cause: malformed JSON on disk. Wipe and start fresh so we
    // don't keep returning a confusing error every gate evaluation.
    console.warn(
      "[onboardingPersistedStore] read_onboarding_state failed, clearing:",
      e,
    );
    try {
      await invoke("write_onboarding_state", { state: {} });
    } catch (clearErr) {
      console.warn(
        "[onboardingPersistedStore] failed to clear corrupt state:",
        clearErr,
      );
    }
    return {};
  }
}

async function writeState(state: OnboardingPersistedState): Promise<void> {
  try {
    await invoke("write_onboarding_state", { state });
  } catch (e) {
    console.warn(
      "[onboardingPersistedStore] write_onboarding_state failed:",
      e,
    );
  }
}

/**
 * Keep the backend's in-memory `reboot_pending` atomic in sync with the
 * file-backed store. Failure is best-effort and logged — the file is still
 * the source of truth at startup (see `seed_reboot_pending_from_disk` in
 * `commands/system.rs`), so a missed sync recovers on next launch.
 */
async function syncEngineRebootPending(pending: boolean): Promise<void> {
  try {
    await invoke("set_reboot_pending", { pending });
  } catch (e) {
    console.warn(
      "[onboardingPersistedStore] set_reboot_pending sync failed:",
      e,
    );
  }
}

async function readBootTimestamp(): Promise<number> {
  try {
    return await invoke<number>("get_boot_timestamp");
  } catch (e) {
    console.warn("[onboardingPersistedStore] get_boot_timestamp failed:", e);
    return 0;
  }
}

// ────────────────────────────────────────────────────────────────────────────
// Public API
// ────────────────────────────────────────────────────────────────────────────

/**
 * Returns `true` only if the user has previously dismissed the
 * RebootRequiredDialog with **Later** AND the system has not rebooted since.
 *
 * Side-effect: when a real reboot is detected (recorded `bootTimestamp`
 * differs from the current OS value), the persisted state is cleared in the
 * same call so the next evaluation reads fresh.
 *
 * If `get_boot_timestamp` returns `0` (sysctl error / non-mac target), the
 * IPC contract says "treat as unable to read" — we conservatively preserve
 * the persisted flag rather than silently re-enable the audio engine.
 */
export async function getRebootPending(): Promise<boolean> {
  const state = await readState();
  const persisted = state.rebootPending;
  if (!persisted) return false;

  const currentBoot = await readBootTimestamp();
  if (currentBoot === 0) {
    // Conservative: cannot verify, assume still pending.
    return true;
  }
  if (currentBoot === persisted.bootTimestamp) {
    return true;
  }

  // Reboot detected — clear the stale flag (also syncs the engine atomic
  // via clearRebootPending → syncEngineRebootPending(false)).
  await clearRebootPending();
  return false;
}

/**
 * Persist that the user has dismissed RebootRequiredDialog with **Later**.
 * Captures the current `kern.boottime` so a real reboot will auto-clear.
 *
 * Idempotent: calling repeatedly with the same boot timestamp is a no-op
 * from the user's perspective. If `get_boot_timestamp` returns `0`, we
 * still write `bootTimestamp: 0`; the next `getRebootPending()` will
 * conservatively keep the flag (since `currentBoot === 0`).
 */
export async function setRebootPending(): Promise<void> {
  const bootTimestamp = await readBootTimestamp();
  const state = await readState();
  await writeState({
    ...state,
    rebootPending: { value: true, bootTimestamp },
  });
  // Keep the engine gate in lock-step with the file.
  await syncEngineRebootPending(true);
}

/**
 * Returns `true` when a fresh install has completed and the user has not yet
 * acknowledged the post-install conferencing-apps screen. Survives reboot —
 * the whole reason this exists is to fire the screen *after* the post-install
 * reboot, since `rebootPending` itself blocks the gate from reaching main UI.
 *
 * Unlike `getRebootPending`, this flag has no boot-timestamp self-clear: it
 * is cleared exclusively by the user clicking Done.
 */
export async function getPendingPostInstallApps(): Promise<boolean> {
  const state = await readState();
  return state.pendingPostInstallApps?.value === true;
}

/**
 * Persist the post-install-apps one-shot flag. Called from
 * `OnboardingSurface.handleInstallSuccess` *only* on the first-install path,
 * not on driver-version updates.
 */
export async function setPendingPostInstallApps(): Promise<void> {
  const state = await readState();
  await writeState({
    ...state,
    pendingPostInstallApps: { value: true },
  });
}

/**
 * Clear the post-install-apps flag. Called when the user clicks Done on the
 * conferencing-apps screen. Idempotent — safe to call when the flag is absent.
 */
export async function clearPendingPostInstallApps(): Promise<void> {
  const state = await readState();
  if (!state.pendingPostInstallApps) return;
  const { pendingPostInstallApps: _omit, ...rest } = state;
  void _omit;
  await writeState(rest);
}

/**
 * Remove the `rebootPending` key. Used both by the gate when it detects an
 * actual reboot and by future flows that want to reset the state explicitly.
 */
export async function clearRebootPending(): Promise<void> {
  const state = await readState();
  if (!state.rebootPending) {
    // Nothing on disk — but still nudge the engine atomic to false in case a
    // prior write succeeded against a now-stale in-memory value.
    await syncEngineRebootPending(false);
    return;
  }
  // Avoid `rebootPending: undefined` ending up in the JSON via spread —
  // construct a fresh object excluding the key.
  const { rebootPending: _omit, ...rest } = state;
  void _omit;
  await writeState(rest);
  // Keep the engine gate in lock-step with the file.
  await syncEngineRebootPending(false);
}
