import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ModalShell, ModalHeader, ModalBody, ModalFooter, ModalButton } from "./ModalShell";
import { setRebootPending } from "@/state/onboardingPersistedStore";

/**
 * Structured error returned by the `request_reboot` Tauri command. Mirrors
 * `src-tauri/src/commands/system.rs::RebootError`.
 */
type RebootError =
  | { kind: "UserCancelled" }
  | { kind: "OsascriptFailed"; message: string };

interface Props {
  /**
   * Called when the user clicks **Later**. The store flag is already set
   * (the install-success transition writes it), but we re-affirm idempotently
   * before invoking. The parent is responsible for routing the gate back to
   * the main UI / minimised tray state.
   */
  onLater: () => void;
}

/**
 * Post-install reboot prompt. Rendered by `OnboardingSurface` when the gate
 * detects a pending reboot via `getRebootPending()` (file-backed store).
 *
 * Two affordances:
 *   - **Reboot Now** → invokes `request_reboot` (osascript via System Events).
 *     On `UserCancelled` the dialog stays open; on `OsascriptFailed` we surface
 *     the stderr inline so the user can either retry or reboot manually from
 *     the Apple menu (the file-backed flag will auto-clear on next launch
 *     after a real reboot — see `getRebootPending`).
 *   - **Later** → persists the reboot-pending flag (idempotent — install
 *     success already wrote it) and dismisses to the yellow-tray steady state.
 *
 * The dialog never closes on success — `request_reboot` either reboots the
 * Mac (process dies) or returns an error.
 */
export function RebootRequiredDialog({ onLater }: Props) {
  const [rebooting, setRebooting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleReboot = async () => {
    if (rebooting) return;
    setRebooting(true);
    setError(null);
    try {
      await invoke("request_reboot");
      // If `request_reboot` succeeds, the system reboot is initiated. The app
      // process is about to die. We deliberately do NOT call `onLater` here.
    } catch (raw) {
      const e = normalize(raw);
      if (e.kind === "UserCancelled") {
        // User dismissed macOS's confirmation dialog — keep the modal open
        // so they can retry without losing context.
        return;
      }
      setError(e.message);
    } finally {
      setRebooting(false);
    }
  };

  const handleLater = async () => {
    // Idempotent re-affirm. If the install transition already set this, the
    // file write is a no-op from the user's perspective.
    try {
      await setRebootPending();
    } catch (e) {
      console.warn("[RebootRequiredDialog] setRebootPending failed:", e);
    }
    onLater();
  };

  return (
    <ModalShell>
      <ModalHeader>Klaar driver installed</ModalHeader>
      <ModalBody>
        <p>
          The audio driver was copied to your Mac. To finish activating{" "}
          <span style={{ color: "var(--color-text-primary)" }}>Klaar</span>,
          your Mac needs to restart.
        </p>
        <p className="mt-3">
          Until you reboot, Klaar can't process audio — communication apps won't see the
          virtual mic. The menu bar icon will stay yellow as a reminder.
        </p>
        {error !== null && (
          <div
            role="alert"
            className="mt-4 rounded border px-3 py-2 text-xs"
            style={{
              backgroundColor: "var(--color-background)",
              borderColor: "var(--color-border)",
              color: "var(--color-text-primary)",
            }}
          >
            <p>
              <strong>Couldn't initiate restart automatically.</strong> Please restart from the
              Apple menu instead.
            </p>
            <pre
              className="mt-2 max-h-32 overflow-auto whitespace-pre-wrap break-all font-mono text-[10px]"
              style={{ color: "var(--color-text-secondary)" }}
            >
              {error}
            </pre>
          </div>
        )}
      </ModalBody>
      <ModalFooter>
        <ModalButton variant="secondary" onClick={handleLater} disabled={rebooting}>
          Later
        </ModalButton>
        <ModalButton onClick={handleReboot} disabled={rebooting}>
          {rebooting ? "Restarting…" : "Reboot Now"}
        </ModalButton>
      </ModalFooter>
    </ModalShell>
  );
}

function normalize(raw: unknown): RebootError {
  if (raw && typeof raw === "object" && "kind" in raw) {
    const e = raw as { kind: string; message?: string };
    switch (e.kind) {
      case "UserCancelled":
        return { kind: "UserCancelled" };
      case "OsascriptFailed":
        return { kind: "OsascriptFailed", message: e.message ?? "" };
    }
  }
  return { kind: "OsascriptFailed", message: String(raw) };
}