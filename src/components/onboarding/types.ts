/**
 * Structured error returned by `install_driver`. Mirrors
 * `src-tauri/src/commands/driver.rs::InstallError`.
 *
 * **History:** the
 * old `RestartFailed` and `DeviceNotAppeared` variants were removed when
 * `install_driver` stopped touching `coreaudiod` and stopped polling for the
 * device to appear. Post-install device materialisation is now deferred until
 * the user reboots — see `RebootRequiredDialog` and
 * `state/onboardingPersistedStore.ts`.
 */
export type InstallError =
  | { kind: "UserCancelled" }
  | { kind: "CopyFailed"; message: string };

/**
 * Entry point that led us into the load-pending fallback. Determines what
 * Retry does on success (conferencing-apps screen vs. straight dismiss).
 *
 * Note: `post-install` is no longer reachable from `install_driver` itself
 * (DeviceNotAppeared was removed) but the variant is retained because the
 * launch-time gate path still uses `LoadPendingEntryPoint` and a future
 * non-reboot recovery flow may resurrect the post-install entry point.
 */
export type LoadPendingEntryPoint = "post-install" | "launch-time";

export type OnboardingScreen =
  | { kind: "install-needed" }
  | { kind: "installing" }
  | { kind: "post-install-apps" }
  | { kind: "load-pending"; entry: LoadPendingEntryPoint }
  | { kind: "install-failure"; error: InstallError }
  | { kind: "update-required" }
  // Post-install reboot prompt. Backed by the file-based onboarding store
  // (not sessionStorage) because it must survive app quit-and-relaunch but
  // be cleared by a real system reboot. See
  // `state/onboardingPersistedStore.ts`.
  | { kind: "reboot-required" };