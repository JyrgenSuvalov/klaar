/**
 * Frontend bridge for the Rust-owned GitHub release update check.
 *
 * The pipeline (HTTP fetch, JSON parse, semver compare, on-disk cache) lives
 * in `src-tauri/src/update_check.rs`. The frontend's only responsibilities
 * are:
 *
 *   1. Subscribing to the `update_status` Tauri event and rendering the banner
 *      when state is `Available` (and the tag isn't in the dismissed-tags
 *      store).
 *   2. Optionally invoking `check_for_app_update { force: true }` from a
 *      dev/debug surface (the user-facing trigger lives in the tray menu).
 *   3. Seeding initial state via `getUpdateStatus()` on mount, so a banner
 *      that mounts after the launch-grace check still picks up the result.
 *
 * The webview no longer makes any HTTP request to `https://api.github.com`;
 * the CSP `connect-src` directive in `tauri.conf.json` does not include it.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/**
 * Compile-time mirror of Rust `update_check::ENABLED`. Frontend short-circuits
 * for defence-in-depth — the Rust IPC also returns the canonical
 * `"update-check disabled"` error when invoked while disabled.
 *
 * MUST stay aligned with `src-tauri/src/update_check.rs`. When the kill-switch
 * is flipped in Rust, flip this too in the same commit (see the
 * `update-check-kill-switch` capability spec).
 */
export const UPDATE_CHECK_ENABLED: boolean = true;

/**
 * Tagged union mirroring the Rust `UpdateStatus` enum. `kind` is the
 * `#[serde(tag = "kind")]` discriminator; the per-variant fields use snake
 * case to match `serde(rename_all = "snake_case")`.
 */
export type UpdateStatus =
  | { kind: "idle" }
  | { kind: "checking"; manual: boolean }
  | { kind: "up_to_date"; checked_at: string }
  | { kind: "available"; tag: string; url: string; checked_at: string }
  | { kind: "error"; error: ErrorKind; checked_at: string };

export type ErrorKind = "network" | "rate_limited" | "parse" | "disabled";

/** Tauri event channel name; mirrors `update_check::UPDATE_STATUS_EVENT`. */
export const UPDATE_STATUS_EVENT = "update_status";

/**
 * Subscribe to `update_status` events emitted by the Rust pipeline. The
 * returned promise resolves to an unlisten function — call it from the
 * effect cleanup to detach.
 *
 * Suppressed entirely while `UPDATE_CHECK_ENABLED` is false (returns a no-op
 * unlistener) so a disabled build never opens an event listener.
 */
export async function subscribeUpdateStatus(
  handler: (status: UpdateStatus) => void,
): Promise<UnlistenFn> {
  if (!UPDATE_CHECK_ENABLED) {
    // Return a no-op so callers can unconditionally `void unlisten()`.
    return () => {};
  }
  return await listen<UpdateStatus>(UPDATE_STATUS_EVENT, (event) => {
    handler(event.payload);
  });
}

/**
 * Imperative manual trigger. The tray menu invokes this with `force: true`
 * to bypass the 24h freshness cache. Returns `Err("update-check disabled")`
 * from Rust when the kill-switch is off — frontend surfaces silently.
 */
export async function invokeCheck(force: boolean): Promise<void> {
  if (!UPDATE_CHECK_ENABLED) return;
  try {
    await invoke("check_for_app_update", { force });
  } catch (e) {
    console.warn("[updateCheck] invoke failed:", e);
  }
}

/**
 * Snapshot of the current state. Frontend calls this once on mount to seed
 * initial state before the event subscription kicks in (e.g., the user opens
 * the main window after the launch-grace check has already resolved).
 */
export async function getUpdateStatus(): Promise<UpdateStatus> {
  if (!UPDATE_CHECK_ENABLED) return { kind: "idle" };
  try {
    return await invoke<UpdateStatus>("get_update_status");
  } catch (e) {
    console.warn("[updateCheck] get_update_status failed:", e);
    return { kind: "idle" };
  }
}
