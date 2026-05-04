import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ModalShell, ModalHeader, ModalBody, ModalFooter, ModalButton } from "./ModalShell";
import type { InstallError } from "./types";

interface Props {
  onSuccess: () => void;
  onFailure: (err: InstallError) => void;
}

/**
 * Progressive copy shown while `install_driver` is in flight.
 *
 * Historical context: the backend used to poll `coreaudiod` for up to 60 s
 * after copying the bundle, hence the staggered 5 / 20 / 45 s thresholds.
 * Post `drop-coreaudiod-restart-from-install`, install completes as soon as
 * the bundle is on disk + Info.plist verified — typically under 2 s. The
 * later buckets are kept as a defence-in-depth against pathological Mac
 * configurations (e.g., very slow disks) where the file copy itself drags.
 *
 * The dialog also explicitly tells the user a reboot will be required next,
 * so the success path transitions to `RebootRequiredDialog` rather than
 * waiting for the device to materialise.
 */
const PROGRESS_BUCKETS: Array<{ thresholdMs: number; message: string }> = [
  { thresholdMs: 0, message: "Installing driver…" },
  { thresholdMs: 5_000, message: "Finalising install…" },
  {
    thresholdMs: 20_000,
    message: "Taking longer than usual. This is normal on Macs with other audio apps running.",
  },
  { thresholdMs: 45_000, message: "Almost done — copying the driver bundle." },
];

/**
 * First-launch / repair install dialog. Single primary action, no cancel
 * affordance — the gate is fail-hard. On `UserCancelled` we silently reset
 * and let the user click again (amendment A8).
 *
 * On success the parent transitions to `RebootRequiredDialog`. The driver
 * does not become an active CoreAudio device until the user reboots.
 */
export function DriverInstallDialog({ onSuccess, onFailure }: Props) {
  const [installing, setInstalling] = useState(false);
  const [progressBucket, setProgressBucket] = useState(0);

  useEffect(() => {
    if (!installing) {
      setProgressBucket(0);
      return;
    }
    // Schedule one timeout per non-zero threshold; clear on resolve/reject.
    const timeouts = PROGRESS_BUCKETS.slice(1).map((bucket, idx) =>
      window.setTimeout(() => setProgressBucket(idx + 1), bucket.thresholdMs),
    );
    return () => {
      for (const id of timeouts) window.clearTimeout(id);
    };
  }, [installing]);

  const handleInstall = async () => {
    if (installing) return;
    setInstalling(true);
    try {
      await invoke("install_driver");
      onSuccess();
    } catch (raw) {
      const err = normalizeInstallError(raw);
      if (err.kind === "UserCancelled") {
        // Silent — reset button and wait for the user to try again.
        return;
      }
      onFailure(err);
    } finally {
      setInstalling(false);
    }
  };

  const progressMessage = PROGRESS_BUCKETS[progressBucket].message;

  return (
    <ModalShell>
      <ModalHeader>Install the Klaar audio driver</ModalHeader>
      <ModalBody>
        <p>
          Klaar needs to install a small audio driver so communication apps like Zoom,
          Google Meet, FaceTime and Slack can hear your processed microphone as{" "}
          <span style={{ color: "var(--color-text-primary)" }}>Klaar</span>.
        </p>
        <p className="mt-3">
          You'll be asked for your Mac password once — that's macOS confirming the driver is
          allowed to install.
        </p>
        <p className="mt-3">
          After install,{" "}
          <span style={{ color: "var(--color-text-primary)" }}>your Mac will need a restart</span>{" "}
          before the driver becomes active. Klaar will offer a one-click <em>Reboot Now</em>{" "}
          button; you can also choose <em>Later</em> and reboot on your own schedule.
        </p>
        {/*
         * Live region is rendered unconditionally so screen readers attach to a
         * stable node — we just blank the text when idle. `aria-live="polite"`
         * + `aria-atomic` means each bucket transition is announced once
         * without spamming the user.
         */}
        <p
          role="status"
          aria-live="polite"
          aria-atomic="true"
          className="mt-4 min-h-[1.25rem]"
          style={{ color: "var(--color-text-primary)" }}
        >
          {installing ? progressMessage : ""}
        </p>
      </ModalBody>
      <ModalFooter>
        <ModalButton onClick={handleInstall} disabled={installing}>
          {installing ? "Installing…" : "Install Driver"}
        </ModalButton>
      </ModalFooter>
    </ModalShell>
  );
}

function normalizeInstallError(raw: unknown): InstallError {
  if (raw && typeof raw === "object" && "kind" in raw) {
    const e = raw as { kind: string; message?: string };
    switch (e.kind) {
      case "UserCancelled":
        return { kind: "UserCancelled" };
      case "CopyFailed":
        return { kind: "CopyFailed", message: e.message ?? "" };
      // `RestartFailed` and `DeviceNotAppeared` were retired. If an older
      // backend somehow still emits them (downgrade scenario), surface them as
      // a CopyFailed with the legacy kind in the message so QA notices.
      default:
        return { kind: "CopyFailed", message: `Unknown error: ${e.kind}` };
    }
  }
  return { kind: "CopyFailed", message: String(raw) };
}
