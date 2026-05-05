// Bootstrap entry point. The `Root` component lives here because this file
// is also the React-DOM mount site — splitting it out would buy nothing
// (HMR doesn't apply to the bootstrap module either way).
/* eslint-disable react-refresh/only-export-components */
import React, { useEffect } from "react";
import ReactDOM from "react-dom/client";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import App from "./App";
import { EnumerationGate } from "./components/onboarding";
import { AboutDialog } from "./components/AboutDialog";
import { UpdateResultDialog } from "./components/onboarding/UpdateResultDialog";
import { useAboutStore } from "./state/aboutStore";
import {
  UPDATE_CHECK_ENABLED,
  type UpdateStatus,
} from "./lib/updateCheck";
import {
  useUpdateResultDialogStore,
  type UpdateManualResultPayload,
} from "./state/updateResultDialogStore";
import "./index.css";

/** Mirrors `update_check::UPDATE_MANUAL_RESULT_EVENT` on the Rust side. */
const UPDATE_MANUAL_RESULT_EVENT = "update_manual_result";

/**
 * Tree-root mount: the `<EnumerationGate>` decides between the onboarding
 * surface and the main `<App>`, while `<AboutDialog>` is mounted as a
 * sibling so its Radix portal layers above any active `ModalShell`.
 * The tray-driven open event is also subscribed at this top level so it
 * survives every gate transition.
 */
function Root() {
  const openAbout = useAboutStore((s) => s.openAbout);

  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    void listen("klaar://about-requested", () => {
      openAbout();
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, [openAbout]);

  // Manual-update-check result dialog: subscribe to `update_manual_result`
  // (sibling of the existing `update_status` channel — see
  // `src-tauri/src/update_check.rs`). Only manual triggers (T5) emit on this
  // channel; background terminals (T1 / T3) stay silent. The store's
  // `show()` swaps the payload in place when a new event arrives, satisfying
  // the rapid-click coalescing rule.
  useEffect(() => {
    if (!UPDATE_CHECK_ENABLED) return;
    let cancelled = false;
    let unlisten: UnlistenFn | null = null;
    void listen<UpdateStatus>(UPDATE_MANUAL_RESULT_EVENT, (event) => {
      // Defensive narrowing — Rust only emits terminal variants, but the
      // shared `UpdateStatus` type covers `Idle` / `Checking` too. Drop
      // anything that isn't a terminal so the dialog never mounts on a
      // non-terminal payload.
      const payload = event.payload;
      if (
        payload.kind === "up_to_date" ||
        payload.kind === "available" ||
        payload.kind === "error"
      ) {
        useUpdateResultDialogStore
          .getState()
          .show(payload as UpdateManualResultPayload);
      }
    }).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Notify the backend that the React tree mounted AND that JS is alive
  // on every subsequent show cycle. This is the proof-of-life signal that
  // defeats the wedged-WKWebView recovery timer in `show_main_window`.
  //
  // Why not just on mount? When the backend's autostart branch keeps the
  // window hidden at login, React still mounts in the auto-created
  // webview during readiness generation 0 (the pre-show sentinel). That
  // ack is correctly dropped (`mark_ready_for(0)` is a no-op). When the
  // user later clicks the tray, `show_main_window` bumps to generation 1
  // and arms a watchdog — but if the existing window is reused (no
  // remount), there is no second mount-time `useEffect` and the watchdog
  // times out incorrectly.
  //
  // Fix: also re-fire on every focus event. `show_main_window` calls
  // `set_focus()` after `show()`, so each surfacing path emits focus
  // changes that re-confirm readiness for the new generation. The
  // backend handler is idempotent within a generation, so extra acks are
  // free.
  useEffect(() => {
    void invoke("frontend_ready");

    let unlistenFocus: UnlistenFn | null = null;
    let cancelled = false;
    void getCurrentWebviewWindow()
      .onFocusChanged(({ payload: focused }) => {
        if (focused) {
          void invoke("frontend_ready");
        }
      })
      .then((fn) => {
        if (cancelled) {
          fn();
        } else {
          unlistenFocus = fn;
        }
      })
      .catch((err) => {
        // Swallow rather than crash mount; the mount-time ack above
        // still covers the common-case (fresh-window) path.
        console.error("frontend_ready focus listener registration failed:", err);
      });

    return () => {
      cancelled = true;
      unlistenFocus?.();
    };
  }, []);

  return (
    <>
      <EnumerationGate>
        <App />
      </EnumerationGate>
      <AboutDialog />
      <UpdateResultDialog />
    </>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);
