import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// Mock Tauri IPC before importing the module under test.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { checkForAppUpdate, UPDATE_CHECK_ENABLED } from "./updateCheck";
import { invoke } from "@tauri-apps/api/core";

const invokeMock = invoke as unknown as ReturnType<typeof vi.fn>;

describe("checkForAppUpdate — kill-switch behavior", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    // Stub global fetch so an accidental network call would be visible.
    vi.stubGlobal(
      "fetch",
      vi.fn().mockRejectedValue(new Error("fetch should not be called while disabled")),
    );
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("is disabled by default while the source repo is private", () => {
    // SAFETY: this MUST stay false until JyrgenSuvalov/klaar is public.
    expect(UPDATE_CHECK_ENABLED).toBe(false);
  });

  it("returns null without performing any IPC or network when disabled", async () => {
    const result = await checkForAppUpdate("0.0.0");

    expect(result).toBeNull();
    expect(invokeMock).not.toHaveBeenCalled();
    expect(globalThis.fetch as unknown as ReturnType<typeof vi.fn>).not.toHaveBeenCalled();
  });
});
