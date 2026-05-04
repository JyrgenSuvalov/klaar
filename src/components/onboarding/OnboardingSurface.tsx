import { useCallback, useState } from "react";
import { DriverInstallDialog } from "./DriverInstallDialog";
import { DriverLoadFallbackDialog } from "./DriverLoadFallbackDialog";
import { InstallFailureDialog } from "./InstallFailureDialog";
import { ConfigureConferencingAppsScreen } from "./ConfigureConferencingAppsScreen";
import { DriverUpdateRequiredDialog } from "./DriverUpdateRequiredDialog";
import { RebootRequiredDialog } from "./RebootRequiredDialog";
import { clearPersistedScreen, persistScreen } from "./persistence";
import {
  clearPendingPostInstallApps,
  setPendingPostInstallApps,
  setRebootPending,
} from "@/state/onboardingPersistedStore";
import type { InstallError, LoadPendingEntryPoint, OnboardingScreen } from "./types";

interface Props {
  /** Initial screen selected by the enumeration gate (D2 in the design). */
  initialScreen: OnboardingScreen;
  /**
   * Called when the onboarding flow completes and the user should be dropped
   * into the main UI. The gate re-evaluates regardless, so dismissal is
   * idempotent — the gate will re-trigger if the driver isn't actually there.
   */
  onDismiss: () => void;
  /** For the update-required screen. */
  installedVersion?: string | null;
  minVersion?: string;
}

/**
 * State machine owner for the onboarding / repair surface.
 *
 * Screens:
 *   install-needed   — first-launch install prompt (D2 scenario A)
 *   post-install-apps — configure-conferencing-apps screen (amendment A7)
 *   load-pending     — driver on disk, not enumerable (D2 scenario C / D3 timeout)
 *   install-failure  — CopyFailed dialog (D5; RestartFailed retired when
 *                      install stopped touching coreaudiod)
 *   update-required  — MIN_DRIVER_VERSION gate (D6)
 *   reboot-required  — post-install reboot prompt; backed by file-store, not
 *                      sessionStorage
 *
 * Transitions are transient state — no Zustand for the in-memory machine,
 * but `reboot-required` writes through to `state/onboardingPersistedStore`
 * so it survives quit-and-relaunch.
 */
export function OnboardingSurface({
  initialScreen,
  onDismiss,
  installedVersion,
  minVersion,
}: Props) {
  const [screen, setScreen] = useState<OnboardingScreen>(initialScreen);

  // Wrap setScreen so every transition keeps sessionStorage in sync.
  // `persistScreen` writes only for terminal screens; `clearPersistedScreen`
  // is called explicitly on transitions out of those (Try Again, success).
  // See `onboarding-resilience` spec for rationale.
  const setScreenAndPersist = useCallback((next: OnboardingScreen) => {
    persistScreen(next);
    setScreen(next);
  }, []);

  // Defensive double-clear on every dismiss path — the gate also clears on
  // transition to "main", but emitting it here means surface correctness
  // doesn't depend on parent semantics.
  const dismissAndClear = useCallback(() => {
    clearPersistedScreen();
    onDismiss();
  }, [onDismiss]);

  // Post-install: write the file-backed reboot-pending flag, then transition
  // to the reboot prompt. Install-success no longer routes to
  // `post-install-apps` directly — the user must reboot before Klaar
  // Virtual Mic becomes an active CoreAudio device.
  //
  // First-install vs update split:
  //   - First-install also sets `pendingPostInstallApps`, so the enumeration
  //     gate routes the user through the conferencing-apps screen exactly
  //     once after they come back from the reboot. Without this flag the
  //     screen is unreachable on the modern install path (the rebootPending
  //     auto-clear lands the user on main UI directly).
  //   - The update path skips `pendingPostInstallApps` — repeat users have
  //     already configured their apps and don't need the screen again.
  const handleFirstInstallSuccess = () => {
    clearPersistedScreen();
    // Sequential, NOT parallel: both helpers do read-modify-write on the same
    // `onboarding-state.json`. Firing them as two concurrent `void`s races —
    // each reads the file before the other writes, and the slower writer
    // (rebootPending, which does extra IPCs for the boot timestamp + engine
    // sync) clobbers the faster one (pendingPostInstallApps). After reboot
    // the flag is missing, the user lands on main UI directly, and the
    // conferencing-apps screen is silently skipped. The whole IIFE is still
    // fire-and-forget — a write failure logs inside the store, the dialog
    // still renders, and the user clicking Later/Reboot re-attempts.
    void (async () => {
      await setRebootPending();
      await setPendingPostInstallApps();
    })();
    setScreenAndPersist({ kind: "reboot-required" });
  };
  const handleUpdateSuccess = () => {
    clearPersistedScreen();
    void setRebootPending();
    setScreenAndPersist({ kind: "reboot-required" });
  };
  // Done on the conferencing-apps screen: clear the one-shot flag, then
  // dismiss as normal. Failure is best-effort — if the disk write fails the
  // user just sees the screen once more on next launch, which is benign.
  const handlePostInstallAppsDone = () => {
    void clearPendingPostInstallApps();
    dismissAndClear();
  };
  const handleFailure = (error: InstallError) =>
    setScreenAndPersist({ kind: "install-failure", error });
  const handleRetryAfterFailure = () => {
    // Clear before transitioning so a remount during the transient
    // `install-needed` render doesn't rehydrate the just-cleared failure.
    clearPersistedScreen();
    setScreenAndPersist({ kind: "install-needed" });
  };

  const handleDriverReadyFromFallback = (entry: LoadPendingEntryPoint) => {
    if (entry === "post-install") {
      setScreenAndPersist({ kind: "post-install-apps" });
    } else {
      // Defensive double-clear: gate also clears on transition to main, but
      // emitting it here too means the surface doesn't depend on parent
      // semantics for cleanup correctness.
      dismissAndClear();
    }
  };

  switch (screen.kind) {
    case "install-needed":
      return (
        <DriverInstallDialog
          onSuccess={handleFirstInstallSuccess}
          onFailure={handleFailure}
        />
      );

    case "installing":
      // `DriverInstallDialog` owns its own in-flight state; we don't render a
      // separate "installing" screen. This case exists in the type union for
      // future extensibility (e.g. a standalone progress screen).
      return (
        <DriverInstallDialog
          onSuccess={handleFirstInstallSuccess}
          onFailure={handleFailure}
        />
      );

    case "post-install-apps":
      return <ConfigureConferencingAppsScreen onDone={handlePostInstallAppsDone} />;

    case "load-pending":
      return (
        <DriverLoadFallbackDialog
          entry={screen.entry}
          onDriverReady={handleDriverReadyFromFallback}
          onDismiss={dismissAndClear}
        />
      );

    case "install-failure":
      return (
        <InstallFailureDialog
          error={screen.error}
          onTryAgain={handleRetryAfterFailure}
        />
      );

    case "update-required":
      // Per task 8.1: update routes through `install_driver` and so inherits
      // the new reboot-required transition automatically. On success we set
      // the file-backed flag and render the reboot prompt rather than
      // immediately dismissing to the main UI. The update path uses
      // `handleUpdateSuccess` (no `pendingPostInstallApps`) — repeat users
      // already configured their apps the first time around.
      return (
        <DriverUpdateRequiredDialog
          installedVersion={installedVersion ?? "unknown"}
          minVersion={minVersion ?? "required"}
          onUpdated={handleUpdateSuccess}
          onFailure={handleFailure}
        />
      );

    case "reboot-required":
      return <RebootRequiredDialog onLater={dismissAndClear} />;
  }
}
