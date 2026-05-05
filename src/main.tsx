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
import { useAboutStore } from "./state/aboutStore";
import "./index.css";

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
    </>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);
