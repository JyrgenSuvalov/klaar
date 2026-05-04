import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { OnboardingSurface } from "./OnboardingSurface";
import { OnboardingErrorBoundary } from "./OnboardingErrorBoundary";
import { UpdateAvailableBanner } from "./UpdateAvailableBanner";
import { isOlderThan } from "@/lib/semver";
import {
  clearPersistedScreen,
  incrementMountCount,
  readPersistedScreen,
} from "./persistence";
import { reportFrontendError } from "@/lib/reportFrontendError";
import {
  getPendingPostInstallApps,
  getRebootPending,
} from "@/state/onboardingPersistedStore";
import type { OnboardingScreen } from "./types";

interface Props {
  children: React.ReactNode;
}

interface GateState {
  /** null while the first check is in flight. */
  screen: OnboardingScreen | null | "main";
  installedVersion: string | null;
  minVersion: string;
  appVersion: string;
}

/**
 * Fail-hard enumeration gate (design D1).
 *
 * Rules:
 *   - On mount, on `audio://devices-changed`, and on window focus, call
 *     `is_driver_installed`. If false, consult `is_driver_file_present` to
 *     pick the install-needed vs. load-pending copy.
 *   - On `audio://device-disconnected`, re-evaluate — the disconnected
 *     device may be the Klaar virtual mic (engineStore emits this
 *     event when any selected device goes missing; we don't need to match
 *     UIDs here because the same `is_driver_installed` check tells us).
 *   - After the enumeration check passes, run the version gate against
 *     `MIN_DRIVER_VERSION`. Older → update-required screen. Newer-or-equal
 *     → main UI.
 *   - Dismissal ("I'll Restart Later", Done) clears the onboarding state
 *     for the current session; the next event-triggered re-evaluation will
 *     re-fire if the driver is still missing.
 */
export function EnumerationGate({ children }: Props) {
  // Synchronously rehydrate any persisted terminal screen. This
  // runs in the useState initialiser so first paint already shows the
  // recovered dialog — no flicker of `install-needed` before the
  // re-evaluation lands.
  const [state, setState] = useState<GateState>(() => {
    const persisted = readPersistedScreen();
    return {
      screen: persisted ?? null,
      installedVersion: null,
      minVersion: "1.0.0",
      appVersion: "0.0.0",
    };
  });

  // Track whether the initial state was rehydrated from sessionStorage so we
  // can skip the first reevaluate (which would otherwise race and clobber
  // the persisted screen). Subsequent event-driven re-evaluations still fire.
  const rehydratedOnMountRef = useRef(state.screen !== null);

  // Snapshot of `state.screen` at mount, captured for the mount
  // heartbeat. Held in a ref so the mount effect closes over a stable read
  // instead of taking `state.screen` as a dep (which would make the effect
  // re-run on every screen change — wrong, the heartbeat is a one-shot
  // diagnostic).
  const persistedScreenAtMountRef = useRef<string | null>(
    state.screen && state.screen !== "main" ? state.screen.kind : null,
  );

  // Suppress re-triggering after an explicit dismiss until the next focus /
  // devices-changed event fires.
  const dismissedUntilEventRef = useRef(false);

  /**
   * File-backed `rebootPending` takes precedence over every other gate
   * state. When active, we transition to `reboot-required` and wipe any
   * stale sessionStorage (`install-failure` / `update-required`) to
   * prevent it from resurrecting on a subsequent remount.
   *
   * Returns `true` when the rebootPending screen was applied (callers
   * should bail out of any further evaluation).
   */
  const applyRebootPendingIfActive = useCallback(async (): Promise<boolean> => {
    try {
      if (await getRebootPending()) {
        clearPersistedScreen();
        setState((s) => ({ ...s, screen: { kind: "reboot-required" } }));
        return true;
      }
    } catch (e) {
      // Fail open — if the file-backed store itself breaks, fall through to
      // the normal enumeration path so the user is not trapped in a blank
      // screen.
      console.warn("[EnumerationGate] getRebootPending failed:", e);
    }
    return false;
  }, []);

  const reevaluate = useCallback(async () => {
    if (dismissedUntilEventRef.current) return;
    // Every event-driven re-evaluation re-checks rebootPending FIRST.
    // If still true, we do not transition out of the reboot-required screen.
    if (await applyRebootPendingIfActive()) return;
    try {
      const installed = await invoke<boolean>("is_driver_installed");
      if (!installed) {
        const filePresent = await invoke<boolean>("is_driver_file_present");
        setState((s) => ({
          ...s,
          screen: filePresent
            ? { kind: "load-pending", entry: "launch-time" }
            : { kind: "install-needed" },
        }));
        return;
      }

      // Enumeration passed — now the version gate.
      const [installedVersion, minVersion] = await Promise.all([
        invoke<string | null>("get_installed_driver_version"),
        invoke<string>("get_min_driver_version"),
      ]);

      if (installedVersion && isOlderThan(installedVersion, minVersion)) {
        setState((s) => ({
          ...s,
          installedVersion,
          minVersion,
          screen: { kind: "update-required" },
        }));
        return;
      }

      // First-install + reboot lifecycle: when the user finishes a fresh
      // install, `OnboardingSurface` writes both `rebootPending` AND
      // `pendingPostInstallApps`. The reboot auto-clears `rebootPending`,
      // but `pendingPostInstallApps` survives until the user clicks Done on
      // the conferencing-apps screen. Route through the screen exactly once
      // before landing them on main UI.
      //
      // Failure of `getPendingPostInstallApps` falls through to main — the
      // post-install screen is helpful, not load-bearing, so a disk error
      // here should not trap the user out of their app.
      let routeToPostInstallApps = false;
      try {
        routeToPostInstallApps = await getPendingPostInstallApps();
      } catch (e) {
        console.warn("[EnumerationGate] getPendingPostInstallApps failed:", e);
      }
      if (routeToPostInstallApps) {
        setState((s) => ({
          ...s,
          installedVersion,
          minVersion,
          screen: { kind: "post-install-apps" },
        }));
        return;
      }

      // Successful transition into the main UI clears any stale persisted
      // failure state — the user has reached steady state.
      clearPersistedScreen();
      setState((s) => ({ ...s, installedVersion, minVersion, screen: "main" }));
    } catch (e) {
      console.error("[EnumerationGate] re-evaluation failed:", e);
      // Fail open — if the gate itself breaks, let the user into the main
      // UI; the existing engineStore banners surface the problem.
      clearPersistedScreen();
      setState((s) => ({ ...s, screen: "main" }));
    }
  }, [applyRebootPendingIfActive]);

  const reevaluateFromEvent = useCallback(async () => {
    dismissedUntilEventRef.current = false;
    await reevaluate();
  }, [reevaluate]);

  useEffect(() => {
    void getVersion().then((v) => setState((s) => ({ ...s, appVersion: v })));

    // Mount-count heartbeat (diagnostics). If the gate is mounting
    // again in the same session AND we have a persisted onboarding screen,
    // report it as evidence of a webview reload mid-onboarding. Fire and
    // forget — diagnostics must not block render.
    const mountCount = incrementMountCount();
    if (mountCount > 1 && rehydratedOnMountRef.current) {
      void reportFrontendError({
        component: "EnumerationGate",
        message: "webview remounted with active onboarding screen",
        context: {
          mountCount,
          persistedScreenKind: persistedScreenAtMountRef.current,
        },
      });
    }

    // File-backed rebootPending check runs unconditionally on mount and
    // takes precedence over both sessionStorage rehydration AND the
    // standard reevaluate path. When inactive:
    //   - rehydrated sessionStorage screen → keep showing it (skip first
    //     reevaluate, same as before).
    //   - cold mount → kick off the normal reevaluate.
    //
    // Tolerates a brief null-render on cold start (matches existing
    // in-flight branch at the bottom of this component). Event-driven
    // re-evaluations are wired below and re-check rebootPending themselves
    // via `reevaluate()`.
    void (async () => {
      if (await applyRebootPendingIfActive()) return;
      if (!rehydratedOnMountRef.current) {
        void reevaluate();
      }
    })();

    const focusHandler = () => {
      void reevaluateFromEvent();
    };
    window.addEventListener("focus", focusHandler);

    let unlistenChanged: UnlistenFn | null = null;
    let unlistenDisconnect: UnlistenFn | null = null;
    void (async () => {
      unlistenChanged = await listen("audio://devices-changed", () => {
        void reevaluateFromEvent();
      });
      unlistenDisconnect = await listen("audio://device-disconnected", () => {
        void reevaluateFromEvent();
      });
    })();

    return () => {
      window.removeEventListener("focus", focusHandler);
      unlistenChanged?.();
      unlistenDisconnect?.();
    };
  }, [reevaluate, reevaluateFromEvent, applyRebootPendingIfActive]);

  const handleDismiss = () => {
    dismissedUntilEventRef.current = true;
    setState((s) => ({ ...s, screen: "main" }));
  };

  if (state.screen === null) {
    // First check in flight — render nothing. This is sub-second; a blank
    // window for 100 ms is less jarring than flashing main → onboarding.
    return null;
  }

  if (state.screen === "main") {
    return (
      <>
        <UpdateAvailableBanner currentVersion={state.appVersion} />
        {children}
      </>
    );
  }

  return (
    <OnboardingErrorBoundary>
      <OnboardingSurface
        initialScreen={state.screen}
        onDismiss={handleDismiss}
        installedVersion={state.installedVersion}
        minVersion={state.minVersion}
      />
    </OnboardingErrorBoundary>
  );
}
