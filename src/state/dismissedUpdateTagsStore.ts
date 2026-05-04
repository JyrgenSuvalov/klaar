/**
 * Frontend persistence for tags the user has dismissed from the
 * `<UpdateAvailableBanner>`.
 *
 * Dismissal is **banner-only** — the tray menu still surfaces
 * "Update available: vX.Y.Z →" after dismissal, because the tray is the
 * always-available signal for a menu-bar app. That's why this store lives
 * on the frontend rather than in the Rust update-check pipeline: keeping
 * dismissed tags out of the Rust state by construction means the tray can
 * never accidentally honour a dismissal. (See decision 5 of the
 * `background-update-check` design.)
 *
 * Persistence backend is `localStorage` — the data is small, ephemeral by
 * nature, and survives quit-and-relaunch within the WebKit profile (which
 * is what we want). No Tauri-fs round-trip needed.
 */

import { create } from "zustand";

const STORAGE_KEY = "klaar.dismissedUpdateTags";

/** Strict shape persisted to localStorage. Extra fields are ignored. */
interface PersistedShape {
  /** Tags the user has explicitly closed via the banner ✕ button. */
  dismissedTags: string[];
}

/**
 * Read the persisted set tolerantly. Any parse failure (corrupt JSON,
 * unexpected shape, localStorage unavailable) returns an empty array — the
 * worst-case effect is a stale banner re-appearing once, which is strictly
 * better than crashing the surface.
 */
function loadFromStorage(): string[] {
  try {
    const raw = globalThis.localStorage?.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as Partial<PersistedShape>;
    if (!parsed || !Array.isArray(parsed.dismissedTags)) return [];
    return parsed.dismissedTags.filter((t): t is string => typeof t === "string");
  } catch {
    return [];
  }
}

function saveToStorage(tags: string[]): void {
  try {
    const payload: PersistedShape = { dismissedTags: tags };
    globalThis.localStorage?.setItem(STORAGE_KEY, JSON.stringify(payload));
  } catch (e) {
    console.warn("[dismissedUpdateTagsStore] persist failed:", e);
  }
}

interface DismissedUpdateTagsState {
  dismissedTags: string[];
  /** Add a tag to the dismissed set. Idempotent — duplicates are no-ops. */
  dismiss: (tag: string) => void;
  /** True iff the tag is currently in the dismissed set. */
  isDismissed: (tag: string) => boolean;
  /**
   * Test-only helper. Wipes the in-memory and persisted state. Not used by
   * production code — the only ways tags leave the set in production are a
   * future "show all updates" affordance (out of scope) or a localStorage
   * clear by the user.
   */
  _resetForTests: () => void;
}

export const useDismissedUpdateTagsStore = create<DismissedUpdateTagsState>(
  (set, get) => ({
    dismissedTags: loadFromStorage(),
    dismiss: (tag: string) => {
      const current = get().dismissedTags;
      if (current.includes(tag)) return;
      const next = [...current, tag];
      saveToStorage(next);
      set({ dismissedTags: next });
    },
    isDismissed: (tag: string) => get().dismissedTags.includes(tag),
    _resetForTests: () => {
      try {
        globalThis.localStorage?.removeItem(STORAGE_KEY);
      } catch {
        // ignore
      }
      set({ dismissedTags: [] });
    },
  }),
);
