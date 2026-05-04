/**
 * Session-scoped persistence for terminal onboarding screens.
 *
 * Background: `OnboardingSurface` uses `useState(initialScreen)`
 * with no `key` prop, so a normal gate re-render cannot reset its inner state.
 * The only way the user lands back on `install-needed` after seeing
 * `install-failure` is a full React tree re-mount — i.e. a webview reload.
 * Persisting the failure screen in `sessionStorage` lets us rehydrate it on
 * the next mount so the user never silently loses an actionable error.
 *
 * Only `install-failure` and `update-required` are persisted — the other
 * screens are recomputable from the gate's IPC checks and need not survive a
 * remount.
 */
import type { OnboardingScreen, InstallError } from "./types";

export const STORAGE_KEY_ACTIVE_SCREEN = "klaar.onboarding.activeScreen";
export const STORAGE_KEY_MOUNT_COUNT = "klaar.onboarding.mountCount";

const TERMINAL_KINDS = new Set<OnboardingScreen["kind"]>([
  "install-failure",
  "update-required",
]);

const ALL_KINDS = new Set<OnboardingScreen["kind"]>([
  "install-needed",
  "installing",
  "post-install-apps",
  "load-pending",
  "install-failure",
  "update-required",
  // `reboot-required` is intentionally NOT listed: it lives in the file-
  // backed `onboarding-state.json`, not sessionStorage, so any sessionStorage
  // entry claiming `reboot-required` is malformed and must be rejected.
]);

// `RestartFailed` and `DeviceNotAppeared` were removed when `install_driver`
// stopped touching `coreaudiod`. Storage entries from older builds claiming
// those kinds now fail validation and are wiped — that is the intended
// migration: the user lands on `install-needed` on next mount.
const INSTALL_ERROR_KINDS = new Set<InstallError["kind"]>([
  "UserCancelled",
  "CopyFailed",
]);

/**
 * Persist a terminal screen to sessionStorage. No-op for non-terminal
 * screens (`install-needed`, `installing`, `post-install-apps`,
 * `load-pending`) — they are recomputable from the gate's IPC checks.
 */
export function persistScreen(screen: OnboardingScreen): void {
  if (!TERMINAL_KINDS.has(screen.kind)) return;
  try {
    sessionStorage.setItem(STORAGE_KEY_ACTIVE_SCREEN, JSON.stringify(screen));
  } catch (e) {
    // Quota exceeded / disabled storage — degrade silently. The gate's
    // event-driven re-evaluation will still recover the user.
    console.warn("[onboarding/persistence] persistScreen failed:", e);
  }
}

/**
 * Clear the persisted screen. Called on Try Again, on successful transition
 * out of the terminal state, and on dismiss.
 */
export function clearPersistedScreen(): void {
  try {
    sessionStorage.removeItem(STORAGE_KEY_ACTIVE_SCREEN);
  } catch {
    // ignore
  }
}

/**
 * Read and validate the persisted screen. On parse error or invalid shape,
 * the entry is removed and `null` is returned.
 */
export function readPersistedScreen(): OnboardingScreen | null {
  let raw: string | null;
  try {
    raw = sessionStorage.getItem(STORAGE_KEY_ACTIVE_SCREEN);
  } catch {
    return null;
  }
  if (raw === null) return null;
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!isValidPersistedScreen(parsed)) {
      console.warn(
        "[onboarding/persistence] discarding malformed persisted screen:",
        parsed,
      );
      clearPersistedScreen();
      return null;
    }
    return parsed;
  } catch (e) {
    console.warn("[onboarding/persistence] JSON.parse failed:", e);
    clearPersistedScreen();
    return null;
  }
}

/**
 * Increment and return the gate mount count for the current session.
 * On any storage failure, returns 1 (treats every mount as the first).
 */
export function incrementMountCount(): number {
  try {
    const raw = sessionStorage.getItem(STORAGE_KEY_MOUNT_COUNT);
    const prev = raw ? Number.parseInt(raw, 10) : 0;
    const next = Number.isFinite(prev) && prev > 0 ? prev + 1 : 1;
    sessionStorage.setItem(STORAGE_KEY_MOUNT_COUNT, String(next));
    return next;
  } catch {
    return 1;
  }
}

// ────────────────────────────────────────────────────────────────────────────
// Type guards
// ────────────────────────────────────────────────────────────────────────────

function isValidPersistedScreen(v: unknown): v is OnboardingScreen {
  if (!v || typeof v !== "object") return false;
  const obj = v as Record<string, unknown>;
  if (typeof obj.kind !== "string") return false;
  if (!ALL_KINDS.has(obj.kind as OnboardingScreen["kind"])) return false;
  // Only terminal screens are valid persisted shapes; reject if someone
  // tampered the storage to inject e.g. `install-needed`.
  if (!TERMINAL_KINDS.has(obj.kind as OnboardingScreen["kind"])) return false;
  if (obj.kind === "install-failure") {
    return isValidInstallError(obj.error);
  }
  // update-required has no payload.
  return true;
}

function isValidInstallError(v: unknown): v is InstallError {
  if (!v || typeof v !== "object") return false;
  const obj = v as Record<string, unknown>;
  if (typeof obj.kind !== "string") return false;
  if (!INSTALL_ERROR_KINDS.has(obj.kind as InstallError["kind"])) return false;
  if (obj.kind === "CopyFailed") {
    return typeof obj.message === "string";
  }
  return true;
}
