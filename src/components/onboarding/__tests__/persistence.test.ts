import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  STORAGE_KEY_ACTIVE_SCREEN,
  STORAGE_KEY_MOUNT_COUNT,
  persistScreen,
  clearPersistedScreen,
  readPersistedScreen,
  incrementMountCount,
} from "../persistence";

describe("onboarding/persistence", () => {
  beforeEach(() => {
    sessionStorage.clear();
  });

  describe("persistScreen", () => {
    it("writes install-failure with full error payload", () => {
      persistScreen({
        kind: "install-failure",
        error: { kind: "CopyFailed", message: "disk full" },
      });
      const raw = sessionStorage.getItem(STORAGE_KEY_ACTIVE_SCREEN);
      expect(raw).not.toBeNull();
      const parsed = JSON.parse(raw!) as Record<string, unknown>;
      expect(parsed).toEqual({
        kind: "install-failure",
        error: { kind: "CopyFailed", message: "disk full" },
      });
    });

    it("writes update-required without payload", () => {
      persistScreen({ kind: "update-required" });
      const raw = sessionStorage.getItem(STORAGE_KEY_ACTIVE_SCREEN);
      expect(JSON.parse(raw!)).toEqual({ kind: "update-required" });
    });

    it("does NOT persist install-needed", () => {
      persistScreen({ kind: "install-needed" });
      expect(sessionStorage.getItem(STORAGE_KEY_ACTIVE_SCREEN)).toBeNull();
    });

    it("does NOT persist installing", () => {
      persistScreen({ kind: "installing" });
      expect(sessionStorage.getItem(STORAGE_KEY_ACTIVE_SCREEN)).toBeNull();
    });

    it("does NOT persist post-install-apps", () => {
      persistScreen({ kind: "post-install-apps" });
      expect(sessionStorage.getItem(STORAGE_KEY_ACTIVE_SCREEN)).toBeNull();
    });

    it("does NOT persist load-pending", () => {
      persistScreen({ kind: "load-pending", entry: "post-install" });
      expect(sessionStorage.getItem(STORAGE_KEY_ACTIVE_SCREEN)).toBeNull();
    });
  });

  describe("clearPersistedScreen", () => {
    it("removes the persisted entry", () => {
      sessionStorage.setItem(
        STORAGE_KEY_ACTIVE_SCREEN,
        JSON.stringify({ kind: "update-required" }),
      );
      clearPersistedScreen();
      expect(sessionStorage.getItem(STORAGE_KEY_ACTIVE_SCREEN)).toBeNull();
    });

    it("is a no-op when nothing is persisted", () => {
      expect(() => clearPersistedScreen()).not.toThrow();
    });
  });

  describe("readPersistedScreen", () => {
    it("returns null when nothing is persisted", () => {
      expect(readPersistedScreen()).toBeNull();
    });

    it("returns a valid install-failure shape", () => {
      sessionStorage.setItem(
        STORAGE_KEY_ACTIVE_SCREEN,
        JSON.stringify({
          kind: "install-failure",
          error: { kind: "CopyFailed", message: "boom" },
        }),
      );
      expect(readPersistedScreen()).toEqual({
        kind: "install-failure",
        error: { kind: "CopyFailed", message: "boom" },
      });
    });

    it("returns null and clears on malformed JSON", () => {
      sessionStorage.setItem(STORAGE_KEY_ACTIVE_SCREEN, "{ not json");
      const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
      expect(readPersistedScreen()).toBeNull();
      expect(sessionStorage.getItem(STORAGE_KEY_ACTIVE_SCREEN)).toBeNull();
      expect(warn).toHaveBeenCalled();
      warn.mockRestore();
    });

    it("returns null and clears on unknown kind", () => {
      sessionStorage.setItem(
        STORAGE_KEY_ACTIVE_SCREEN,
        JSON.stringify({ kind: "garbage" }),
      );
      const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
      expect(readPersistedScreen()).toBeNull();
      expect(sessionStorage.getItem(STORAGE_KEY_ACTIVE_SCREEN)).toBeNull();
      warn.mockRestore();
    });

    it("rejects non-terminal kinds even if syntactically valid", () => {
      // A tampered storage entry should not let `install-needed` rehydrate
      // and skip the gate's reevaluate.
      sessionStorage.setItem(
        STORAGE_KEY_ACTIVE_SCREEN,
        JSON.stringify({ kind: "install-needed" }),
      );
      const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
      expect(readPersistedScreen()).toBeNull();
      expect(sessionStorage.getItem(STORAGE_KEY_ACTIVE_SCREEN)).toBeNull();
      warn.mockRestore();
    });

    it("rejects install-failure with missing error payload", () => {
      sessionStorage.setItem(
        STORAGE_KEY_ACTIVE_SCREEN,
        JSON.stringify({ kind: "install-failure" }),
      );
      const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
      expect(readPersistedScreen()).toBeNull();
      warn.mockRestore();
    });

    it("rejects retired install-error kinds (RestartFailed/DeviceNotAppeared)", () => {
      // Migration coverage: an entry written by an older build must be wiped,
      // not rehydrated, so the user lands on a fresh gate evaluation rather
      // than a dead screen kind.
      for (const legacy of ["RestartFailed", "DeviceNotAppeared"]) {
        sessionStorage.setItem(
          STORAGE_KEY_ACTIVE_SCREEN,
          JSON.stringify({
            kind: "install-failure",
            error: { kind: legacy },
          }),
        );
        const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
        expect(readPersistedScreen()).toBeNull();
        expect(sessionStorage.getItem(STORAGE_KEY_ACTIVE_SCREEN)).toBeNull();
        warn.mockRestore();
      }
    });

    it("rejects reboot-required from sessionStorage (file-store-only kind)", () => {
      // `reboot-required` lives in `onboarding-state.json`, not sessionStorage.
      // A tampered entry must not bypass the file-backed flow.
      sessionStorage.setItem(
        STORAGE_KEY_ACTIVE_SCREEN,
        JSON.stringify({ kind: "reboot-required" }),
      );
      const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
      expect(readPersistedScreen()).toBeNull();
      expect(sessionStorage.getItem(STORAGE_KEY_ACTIVE_SCREEN)).toBeNull();
      warn.mockRestore();
    });

    it("rejects install-failure with unknown error kind", () => {
      sessionStorage.setItem(
        STORAGE_KEY_ACTIVE_SCREEN,
        JSON.stringify({
          kind: "install-failure",
          error: { kind: "WhoDis" },
        }),
      );
      const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
      expect(readPersistedScreen()).toBeNull();
      warn.mockRestore();
    });

    it("rejects CopyFailed with non-string message", () => {
      sessionStorage.setItem(
        STORAGE_KEY_ACTIVE_SCREEN,
        JSON.stringify({
          kind: "install-failure",
          error: { kind: "CopyFailed", message: 42 },
        }),
      );
      const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
      expect(readPersistedScreen()).toBeNull();
      warn.mockRestore();
    });
  });

  describe("incrementMountCount", () => {
    it("returns 1 on first call in a fresh session", () => {
      expect(incrementMountCount()).toBe(1);
      expect(sessionStorage.getItem(STORAGE_KEY_MOUNT_COUNT)).toBe("1");
    });

    it("monotonically increments on subsequent calls", () => {
      expect(incrementMountCount()).toBe(1);
      expect(incrementMountCount()).toBe(2);
      expect(incrementMountCount()).toBe(3);
    });

    it("recovers from a corrupt counter value", () => {
      sessionStorage.setItem(STORAGE_KEY_MOUNT_COUNT, "not-a-number");
      expect(incrementMountCount()).toBe(1);
    });
  });
});
