import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  UPDATE_CHECK_ENABLED,
  checkForAppUpdate,
  dismissUpdateTag,
  type UpdateAvailable,
} from "@/lib/updateCheck";
import { a11y } from "@/i18n/a11yStrings";

interface Props {
  currentVersion: string;
}

/**
 * Shown only in the main UI, never during onboarding. Dismissal persists per
 * tag in `update-check.json`; a newer release un-dismisses.
 */
export function UpdateAvailableBanner({ currentVersion }: Props) {
  const [update, setUpdate] = useState<UpdateAvailable | null>(null);

  useEffect(() => {
    // Kill-switch: while disabled, do not fire the IPC / fetch chain at all.
    if (!UPDATE_CHECK_ENABLED) return;
    let cancelled = false;
    void checkForAppUpdate(currentVersion).then((u) => {
      if (!cancelled) setUpdate(u);
    });
    return () => {
      cancelled = true;
    };
  }, [currentVersion]);

  if (!UPDATE_CHECK_ENABLED) return null;

  if (!update) return null;

  const handleDismiss = () => {
    const tag = update.tag;
    setUpdate(null);
    void dismissUpdateTag(tag);
  };

  const handleOpen = () => {
    void openUrl(update.url).catch((e) => {
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
        A new Klaar release is available ({update.tag}).{" "}
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
