/**
 * In-memory store backing the About dialog.
 *
 * The store deliberately has NO persistence layer — opening the About
 * dialog across sessions makes no sense, and the cached `version` is only
 * meaningful for the lifetime of the running binary anyway. The shape is
 * intentionally minimal:
 *
 *   - `aboutOpen`     — visibility flag the `<AboutDialog>` consumes
 *   - `version`       — null until the first `get_app_version` IPC resolves;
 *                       the dialog caches it for the session so subsequent
 *                       opens never re-invoke
 *   - `openAbout()`   — flips `aboutOpen` to true; called from the App-root
 *                       listener when the tray emits
 *                       `klaar://about-requested`
 *   - `closeAbout()`  — flips `aboutOpen` to false; bound to Escape, the
 *                       backdrop click, and the explicit Close button
 *   - `setVersion(v)` — caches the IPC result on first open
 */

import { create } from "zustand";

interface AboutState {
  aboutOpen: boolean;
  version: string | null;
  openAbout: () => void;
  closeAbout: () => void;
  setVersion: (v: string) => void;
}

export const useAboutStore = create<AboutState>((set) => ({
  aboutOpen: false,
  version: null,
  openAbout: () => set({ aboutOpen: true }),
  closeAbout: () => set({ aboutOpen: false }),
  setVersion: (v) => set({ version: v }),
}));
