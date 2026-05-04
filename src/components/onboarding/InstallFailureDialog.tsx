import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { ModalShell, ModalHeader, ModalBody, ModalFooter, ModalButton } from "./ModalShell";
import type { InstallError } from "./types";

interface Props {
  error: InstallError;
  onTryAgain: () => void;
}

/**
 * Shown when `install_driver` returned `CopyFailed` (the only failure kind
 * remaining after `RestartFailed` and `DeviceNotAppeared` were retired).
 * Gives the user a single actionable affordance (retry) plus a diagnostic-copy
 * button that grabs the last 5 min of `com.apple.audio` logs.
 */
export function InstallFailureDialog({ error, onTryAgain }: Props) {
  const [showDetails, setShowDetails] = useState(false);
  const [collecting, setCollecting] = useState(false);
  const [toast, setToast] = useState<string | null>(null);

  const handleCopyDiagnostics = async () => {
    if (collecting) return;
    setCollecting(true);
    setToast(null);
    try {
      const text = await invoke<string>("collect_coreaudio_diagnostics");
      await writeText(text);
      setToast("Copied to clipboard");
    } catch (e) {
      console.error("[onboarding] collect_coreaudio_diagnostics failed:", e);
      setToast("Couldn't copy diagnostics — see the log file instead.");
    } finally {
      setCollecting(false);
      // Auto-clear toast after a few seconds so the dialog doesn't get stuck.
      setTimeout(() => setToast(null), 4000);
    }
  };

  const payload = error.kind === "CopyFailed" ? error.message : null;

  return (
    <ModalShell>
      <ModalHeader>Something went wrong installing the driver</ModalHeader>
      <ModalBody>
        <p>
          The driver couldn't be installed. This is almost always something we can figure out
          from the system audio log.
        </p>
        <p className="mt-2">
          Click <strong>Copy diagnostic info</strong>, then paste the output into an email or
          GitHub issue.
        </p>

        {payload && (
          <div className="mt-3">
            <button
              className="text-xs underline cursor-pointer focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--color-accent)] rounded-sm"
              style={{ color: "var(--color-text-secondary)" }}
              onClick={() => setShowDetails((v) => !v)}
            >
              {showDetails ? "Hide details" : "Show details"}
            </button>
            {showDetails && (
              <pre
                className="mt-2 max-h-40 overflow-auto rounded px-2 py-1.5 text-[10px] font-mono whitespace-pre-wrap break-all"
                style={{
                  backgroundColor: "var(--color-background)",
                  color: "var(--color-text-primary)",
                  border: "1px solid var(--color-border)",
                }}
              >
                {payload}
              </pre>
            )}
          </div>
        )}

        {toast && (
          <p className="mt-3 text-xs" style={{ color: "var(--color-accent)" }}>
            {toast}
          </p>
        )}
      </ModalBody>
      <ModalFooter>
        <ModalButton
          variant="secondary"
          onClick={handleCopyDiagnostics}
          disabled={collecting}
          title="Runs `log show --predicate 'subsystem == com.apple.audio' --last 5m`"
        >
          {collecting ? "Collecting…" : "Copy diagnostic info"}
        </ModalButton>
        <ModalButton onClick={onTryAgain}>Try Again</ModalButton>
      </ModalFooter>
    </ModalShell>
  );
}
