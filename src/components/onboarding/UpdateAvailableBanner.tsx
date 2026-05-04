import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  UPDATE_CHECK_ENABLED,
  getUpdateStatus,
  subscribeUpdateStatus,
  type UpdateStatus,
} from "@/lib/updateCheck";
import { useDismissedUpdateTagsStore } from "@/state/dismissedUpdateTagsStore";
import { a11y } from "@/i18n/a11yStrings";

/**
 * Shown only in the main UI, never during onboarding. The banner is a
 * passive observer of the Rust-owned `update_status` event channel — it
 * never initiates HTTP requests or invokes the manual check IPC. The
 * tray menu remains the user-facing manual trigger.
 *
 * Dismissal is **banner-only** (per the `app-update-check` capability spec).
 * The dismissed tag is persisted to `useDismissedUpdateTagsStore`, NOT to
 * the Rust cache file — so the tray continues to surface the update until a
 * newer release supersedes it or the user installs.
 *
 * The `currentVersion` prop is retained for backwards-compat with existing
 * call sites but is no longer used for comparison: the Rust side has
 * already done the semver compare before emitting `Available`.
 */
interface Props {
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  currentVersion: string;
}

export function UpdateAvailableBanner({ currentVersion: _ }: Props) {
  const [status, setStatus] = useState<UpdateStatus>({ kind: "idle" });
  const dismissedTags = useDismissedUpdateTagsStore((s) => s.dismissedTags);
  const dismiss = useDismissedUpdateTagsStore((s) => s.dismiss);

  useEffect(() => {
    // Defence-in-depth — Rust returns `Err("update-check disabled")` from
    // both IPCs when the kill-switch is off, but skipping subscription
    // entirely keeps the event listener count to zero.
    if (!UPDATE_CHECK_ENABLED) return;

    let cancelled = false;
    let unlisten: (() => void) | null = null;

    // 1. Subscribe FIRST so we don't miss a transition while seeding.
    void subscribeUpdateStatus((next) => {
      if (!cancelled) setStatus(next);
    }).then((u) => {
      if (cancelled) {
        u();
        return;
      }
      unlisten = u;
    });

    // 2. Seed initial state once. If the launch-grace check has already
    // resolved by the time the user opens the window, this catches us up.
    void getUpdateStatus().then((s) => {
      if (!cancelled) setStatus(s);
    });

    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  if (!UPDATE_CHECK_ENABLED) return null;
  if (status.kind !== "available") return null;
  if (dismissedTags.includes(status.tag)) return null;

  const { tag, url } = status;

  const handleDismiss = () => dismiss(tag);

  const handleOpen = () => {
    void openUrl(url).catch((e) => {
      console.warn("[UpdateAvailableBanner] openUrl failed:", e);
    });
  };

  return (
    <div
      className="flex items-center justify-between gap-3 px-4 py-2 text-xs shrink-0"
      style={{
        backgroundColor: "#0e1a1a",
        borderBottom: "1px solid #164e63",
        color: "#67e8f9",
      }}
    >
      <span>
        A new Klaar release is available ({tag}).{" "}
        <button
          className="underline font-medium cursor-pointer focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--color-accent)] rounded-sm"
          style={{ color: "#67e8f9", background: "transparent", border: "none", padding: 0 }}
          onClick={handleOpen}
        >
          Open release page
        </button>
      </span>
      <button
        className="opacity-60 hover:opacity-100 transition-opacity cursor-pointer shrink-0 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--color-accent)] rounded-sm"
        onClick={handleDismiss}
        aria-label={a11y.dismissUpdateNotification()}
      >
        ✕
      </button>
    </div>
  );
}
