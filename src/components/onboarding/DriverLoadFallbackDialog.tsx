import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ModalShell, ModalHeader, ModalBody, ModalFooter, ModalButton } from "./ModalShell";
import type { LoadPendingEntryPoint } from "./types";

interface Props {
  entry: LoadPendingEntryPoint;
  /** Retry succeeded (driver now enumerable). */
  onDriverReady: (entry: LoadPendingEntryPoint) => void;
  /** User clicked "I'll Restart Later". */
  onDismiss: () => void;
}

/**
 * Shown when the driver file is on disk but CoreAudio hasn't picked it up —
 * the launch-time gate path observes a stale bundle that survived a previous
 * crash without coreaudiod re-scanning.
 *
 * Note: the post-install entry point is no longer reached from
 * `install_driver` itself — the install flow transitions to
 * `RebootRequiredDialog` instead of polling for device materialisation.
 */
export function DriverLoadFallbackDialog({ entry, onDriverReady, onDismiss }: Props) {
  const [restarting, setRestarting] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const [toast, setToast] = useState<string | null>(null);

  const handleRestart = async () => {
    if (restarting) return;
    setRestarting(true);
    setToast(null);
    try {
      await invoke("restart_mac");
      // OS is rebooting — nothing more to do.
    } catch (e) {
      console.error("[onboarding] restart_mac failed:", e);
      setToast("Please restart your Mac via the Apple menu.");
      setRestarting(false);
    }
  };

  const handleRetry = async () => {
    if (retrying) return;
    setRetrying(true);
    setToast(null);
    try {
      const installed = await invoke<boolean>("is_driver_installed");
      if (installed) {
        onDriverReady(entry);
        return;
      }
      // Still not enumerable — give the user explicit feedback rather than
      // a silent no-op.
      setToast(
        "Driver still not loaded — try restarting your Mac, or wait a moment and retry.",
      );
    } catch (e) {
      console.warn("[onboarding] is_driver_installed failed:", e);
      setToast("Couldn't check the driver — try again, or restart your Mac.");
    } finally {
      setRetrying(false);
      // Auto-clear so a stale message doesn't sit forever if the user walks
      // away. Mirrors the InstallFailureDialog toast behaviour.
      setTimeout(() => setToast(null), 5000);
    }
  };

  return (
    <ModalShell>
      <ModalHeader>Restart your Mac to finish setting up Klaar</ModalHeader>
      <ModalBody>
        <p>
          The audio driver is installed but hasn't loaded yet. This can usually be fixed by
          restarting your Mac.
        </p>
        {toast && (
          <p className="mt-3" style={{ color: "#fca5a5" }}>
            {toast}
          </p>
        )}
      </ModalBody>
      <ModalFooter>
        <ModalButton variant="ghost" onClick={onDismiss}>
          I'll Restart Later
        </ModalButton>
        <ModalButton variant="secondary" onClick={handleRetry} disabled={retrying}>
          {retrying ? "Checking…" : "Retry"}
        </ModalButton>
        <ModalButton onClick={handleRestart} disabled={restarting}>
          {restarting ? "Restarting…" : "Restart Now"}
        </ModalButton>
      </ModalFooter>
    </ModalShell>
  );
}
