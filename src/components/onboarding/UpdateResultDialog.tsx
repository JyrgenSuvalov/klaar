/**
 * Manual-update-check result dialog.
 *
 * Mounts in response to the `update_manual_result` Tauri event (Rust emits
 * it whenever a check launched with `manual: true` resolves to a terminal
 * `UpdateStatus`). The frontend's event listener (in `main.tsx`) routes the
 * payload into `useUpdateResultDialogStore`; this component reads the store
 * and renders the per-state copy/controls.
 *
 * Layering mirrors `<AboutDialog>`: shadcn `Dialog` primitive, dark
 * translucent backdrop, Escape + backdrop-click dismissal. The dialog is
 * mounted at the React tree root so its Radix portal renders above any
 * active `ModalShell` gate.
 *
 * Auto-dismiss policy:
 *   - `up_to_date` and `error` auto-dismiss after ~5 s (informational —
 *     the user has no decision to make).
 *   - `available` does NOT auto-dismiss — it has actionable buttons.
 *
 * Coalescing: the dialog's auto-dismiss `useEffect` keys on the payload
 * identity. When the store swaps the payload (rapid re-clicks of the tray
 * item), the effect re-runs and the timer resets — see
 * `updateResultDialogStore.ts` for the swap rule.
 *
 * Persistence note: dismissing this dialog does NOT affect the
 * `<UpdateAvailableBanner>` or the tray "Update available: vX.Y.Z →"
 * sub-item. Those are the persistent signals; this dialog is only the
 * "you asked, here's the answer" feedback.
 */

import { useEffect } from "react";

import { openUrl } from "@tauri-apps/plugin-opener";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { ModalButton } from "@/components/onboarding/ModalShell";
import type { ErrorKind } from "@/lib/updateCheck";
import { useAboutStore } from "@/state/aboutStore";
import {
  useUpdateResultDialogStore,
  type UpdateManualResultPayload,
} from "@/state/updateResultDialogStore";

/**
 * Auto-dismiss timeout for `up_to_date` and `error` payloads. Matches the
 * existing tray transient-revert window (`MANUAL_REVERT_AFTER` in
 * `update_check.rs`).
 */
const AUTO_DISMISS_MS = 5_000;

interface DialogCopy {
  /** Heading rendered as the `DialogTitle`. */
  title: string;
  /** Body copy below the title. May be empty. */
  body: string;
}

/**
 * Per-error-kind copy. Centralised so future additions (or wording tweaks)
 * are a single edit.
 */
function errorCopy(kind: ErrorKind): DialogCopy {
  switch (kind) {
    case "network":
      return {
        title: "Update check failed",
        body: "Couldn't reach GitHub. Check your connection and try again.",
      };
    case "rate_limited":
      return {
        title: "Update check rate-limited",
        body: "GitHub is rate-limiting requests. Try again in a few minutes.",
      };
    case "parse":
      return {
        title: "Update check failed",
        body: "Couldn't parse the latest release info. Please report this.",
      };
    case "disabled":
      // Defensive — the kill-switch path never fires `update_manual_result`,
      // but we keep the branch exhaustive so the type system enforces it.
      return {
        title: "Update check disabled",
        body: "Update checks are disabled in this build.",
      };
  }
}

/**
 * Resolve the title + body for any payload. `available` titles include the
 * tag; `up_to_date` titles include the running version.
 */
function copyFor(
  payload: UpdateManualResultPayload,
  currentVersion: string | null,
): DialogCopy {
  switch (payload.kind) {
    case "up_to_date": {
      const versionStr = currentVersion ? ` (v${currentVersion.replace(/^v/, "")})` : "";
      return {
        title: "You're up to date.",
        body: `Klaar${versionStr} is the current version.`,
      };
    }
    case "available":
      return {
        title: `Klaar ${payload.tag} is available.`,
        body: "A newer release is available on GitHub.",
      };
    case "error":
      return errorCopy(payload.error);
  }
}

export function UpdateResultDialog() {
  const payload = useUpdateResultDialogStore((s) => s.payload);
  const dismiss = useUpdateResultDialogStore((s) => s.dismiss);
  // Reuse the version cached by the About dialog (see `aboutStore.ts`).
  // We deliberately do NOT issue our own `get_app_version` IPC — the spec
  // says "use the existing version source".
  const version = useAboutStore((s) => s.version);

  // Auto-dismiss timer — keyed on payload identity so swaps reset it.
  // `available` is sticky (no timer) because it has actionable buttons.
  useEffect(() => {
    if (!payload) return;
    if (payload.kind === "available") return;
    const id = window.setTimeout(() => {
      dismiss();
    }, AUTO_DISMISS_MS);
    return () => {
      window.clearTimeout(id);
    };
  }, [payload, dismiss]);

  if (!payload) return null;

  const { title, body } = copyFor(payload, version);

  const handleOpenReleasePage = () => {
    if (payload.kind !== "available") return;
    void openUrl(payload.url).catch((e) => {
      console.warn("[UpdateResultDialog] openUrl failed:", e);
    });
    dismiss();
  };

  return (
    <Dialog
      open={true}
      onOpenChange={(open) => {
        if (!open) dismiss();
      }}
    >
      <DialogContent
        className="max-w-sm"
        // Match `<AboutDialog>` — suppress Radix's default focus-first-tabbable
        // behaviour so a button doesn't look pre-selected on open.
        onOpenAutoFocus={(e) => e.preventDefault()}
      >
        <div className="flex flex-col items-center text-center gap-3">
          <DialogTitle className="text-xl">{title}</DialogTitle>

          <DialogDescription className="text-sm text-[var(--color-text-secondary)]">
            {body}
          </DialogDescription>

          <div className="flex items-center justify-center gap-2 pt-2">
            {payload.kind === "available" ? (
              <>
                {/* Primary action — same `<ModalButton variant="primary">` the
                    driver install flow uses, which gets the contrast right
                    (near-black text on the teal accent). */}
                <ModalButton variant="primary" onClick={handleOpenReleasePage}>
                  Open release page
                </ModalButton>
                <ModalButton variant="secondary" onClick={dismiss}>
                  Later
                </ModalButton>
              </>
            ) : (
              <ModalButton variant="secondary" onClick={dismiss}>
                OK
              </ModalButton>
            )}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
