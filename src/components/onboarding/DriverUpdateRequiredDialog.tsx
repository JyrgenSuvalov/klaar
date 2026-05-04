import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ModalShell, ModalHeader, ModalBody, ModalFooter, ModalButton } from "./ModalShell";
import type { InstallError } from "./types";

interface Props {
  installedVersion: string;
  minVersion: string;
  /**
   * Called when `install_driver` resolves successfully. The parent transitions
   * to the `reboot-required` screen on success — the updated bundle does not
   * become an active CoreAudio device until the user reboots.
   */
  onUpdated: () => void;
  onFailure: (err: InstallError) => void;
}

/**
 * Blocks the main UI when the installed driver is older than
 * `MIN_DRIVER_VERSION`. The Update action re-runs `install_driver`, which
 * overwrites the on-disk bundle.
 */
export function DriverUpdateRequiredDialog({
  installedVersion,
  minVersion,
  onUpdated,
  onFailure,
}: Props) {
  const [updating, setUpdating] = useState(false);

  const handleUpdate = async () => {
    if (updating) return;
    setUpdating(true);
    try {
      await invoke("install_driver");
      onUpdated();
    } catch (raw) {
      const err = normalize(raw);
      if (err.kind === "UserCancelled") {
        return;
      }
      onFailure(err);
    } finally {
      setUpdating(false);
    }
  };

  return (
    <ModalShell>
      <ModalHeader>Update the Klaar audio driver</ModalHeader>
      <ModalBody>
        <p>
          Klaar has an updated audio driver. Your Mac has version{" "}
          <span style={{ color: "var(--color-text-primary)" }}>{installedVersion}</span>; this
          build requires at least{" "}
          <span style={{ color: "var(--color-text-primary)" }}>{minVersion}</span>.
        </p>
        <p className="mt-3">
          You'll be asked for your Mac password once to let macOS replace the driver.
        </p>
      </ModalBody>
      <ModalFooter>
        <ModalButton onClick={handleUpdate} disabled={updating}>
          {updating ? "Updating…" : "Update Driver"}
        </ModalButton>
      </ModalFooter>
    </ModalShell>
  );
}

function normalize(raw: unknown): InstallError {
  if (raw && typeof raw === "object" && "kind" in raw) {
    const e = raw as { kind: string; message?: string };
    switch (e.kind) {
      case "UserCancelled":
        return { kind: "UserCancelled" };
      case "CopyFailed":
        return { kind: "CopyFailed", message: e.message ?? "" };
      // `RestartFailed` / `DeviceNotAppeared` were retired. Unknown kinds
      // fall through to the generic CopyFailed below.
    }
  }
  return { kind: "CopyFailed", message: String(raw) };
}
