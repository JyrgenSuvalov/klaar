// Bootstrap entry point. The `Root` component lives here because this file
// is also the React-DOM mount site — splitting it out would buy nothing
// (HMR doesn't apply to the bootstrap module either way).
/* eslint-disable react-refresh/only-export-components */
import React, { useEffect } from "react";
import ReactDOM from "react-dom/client";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
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
