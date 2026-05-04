/**
 * Hotkey-registration warning banner.
 *
 * Backend contract:
 *   `get_hotkey_warning` is a *read-and-clear* IPC. The backend's
 *   `swap(false, AcqRel)` returns `Some(message)` once if `⌃⇧M` failed to
 *   bind at startup, and `None` on every subsequent call within the same
 *   process lifetime. The wording is owned by `HOTKEY_WARNING_MESSAGE` in
 *   `commands::mute` and locked by a Rust test — we render whatever the IPC
 *   hands us rather than duplicate the literal here.
 *
 * Render-once contract (mirrors `AboutDialog`'s `get_app_version` cache):
 *   The fetched-ness is held in a *module-level* flag so two mount/unmount
 *   cycles in the same render lifecycle (e.g. React StrictMode dev
 *   double-mount, or a future code path that remounts the App shell) only
 *   call the IPC once. A `useRef` would not survive remounts, and the
 *   backend's swap-on-read means a second call would always return `None`
 *   even if the warning was real — tolerable, but the contract is "warn at
 *   most once per process," and the frontend should reflect that intent.
 *
 *   Test isolation: `__resetHotkeyWarningCheckForTest` resets the flag.
 *
 * Visual: matches the `driver_missing` palette already used by the engine-
 * error banner in `App.tsx` (`#1a1a0e` bg / `#854d0e` border / `#fbbf24`
 * text) — the existing dark-theme warning tokens. Dismiss is local-only;
 * the backend flag was already cleared on read, so there's no second IPC.
 */
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { a11y } from "@/i18n/a11yStrings";

// Module-level guard — see file docstring. Mutable on purpose.
let alreadyChecked = false;

/**
 * Test-only: reset the module-level "already fetched" flag between tests.
 *
 * Co-located with the flag rather than extracted to a sibling so the reset
 * remains physically next to the state it controls. The cost is that Vite's
 * fast-refresh treats this file as "mixed" and reloads instead of HMR-ing
 * — acceptable for a small banner component that rarely changes.
 */
// eslint-disable-next-line react-refresh/only-export-components
export function __resetHotkeyWarningCheckForTest(): void {
  alreadyChecked = false;
}

export function HotkeyWarningBanner() {
  const [message, setMessage] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    if (alreadyChecked) return;
    alreadyChecked = true;
    void invoke<string | null>("get_hotkey_warning")
      .then((m) => {
        if (m) setMessage(m);
      })
      .catch((e) => {
        // Non-fatal — the warning is informational. Log and carry on so a
        // transient IPC hiccup never blocks app startup.
        console.warn("get_hotkey_warning failed:", e);
      });
  }, []);

  if (!message || dismissed) return null;

  return (
    <div
      role="alert"
      className="app-wrapper px-4 py-2 text-xs shrink-0"
      style={{
        backgroundColor: "#1a1a0e",
        borderBottom: "1px solid #854d0e",
        color: "#fbbf24",
      }}
    >
      <div className="flex items-start justify-between gap-3">
        <span>{message}</span>
        <button
          className="opacity-60 hover:opacity-100 transition-opacity cursor-pointer shrink-0"
          onClick={() => setDismissed(true)}
          aria-label={a11y.hotkeyWarningDismiss()}
        >
          ✕
        </button>
      </div>
    </div>
  );
}
