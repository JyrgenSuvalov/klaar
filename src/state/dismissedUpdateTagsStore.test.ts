/**
 * Tests for `useDismissedUpdateTagsStore`. Covers the round-trip through
 * localStorage and the idempotent dismiss semantics.
 */

import { describe, it, expect, beforeEach, vi } from "vitest";

// We exercise the real store against a fresh module per test so the
// `loadFromStorage()` call at module-evaluation time picks up whatever we
// pre-populated in localStorage.

describe("useDismissedUpdateTagsStore", () => {
  beforeEach(() => {
    globalThis.localStorage?.clear();
    vi.resetModules();
  });

  it("starts empty when localStorage has no entry", async () => {
    const { useDismissedUpdateTagsStore } = await import(
      "./dismissedUpdateTagsStore"
    );
    expect(useDismissedUpdateTagsStore.getState().dismissedTags).toEqual([]);
  });

  it("persists a dismissed tag across module reloads", async () => {
    {
      const { useDismissedUpdateTagsStore } = await import(
        "./dismissedUpdateTagsStore"
      );
      useDismissedUpdateTagsStore.getState().dismiss("v1.4.0");
      expect(useDismissedUpdateTagsStore.getState().dismissedTags).toEqual([
        "v1.4.0",
      ]);
    }
    // New module instance — should rehydrate from localStorage.
    vi.resetModules();
    {
      const { useDismissedUpdateTagsStore } = await import(
        "./dismissedUpdateTagsStore"
      );
      expect(useDismissedUpdateTagsStore.getState().dismissedTags).toEqual([
        "v1.4.0",
      ]);
      expect(useDismissedUpdateTagsStore.getState().isDismissed("v1.4.0")).toBe(
        true,
      );
      expect(useDismissedUpdateTagsStore.getState().isDismissed("v1.5.0")).toBe(
        false,
      );
    }
  });

  it("dismiss() is idempotent (duplicates do not grow the set)", async () => {
    const { useDismissedUpdateTagsStore } = await import(
      "./dismissedUpdateTagsStore"
    );
    useDismissedUpdateTagsStore.getState().dismiss("v1.4.0");
    useDismissedUpdateTagsStore.getState().dismiss("v1.4.0");
    useDismissedUpdateTagsStore.getState().dismiss("v1.4.0");
    expect(useDismissedUpdateTagsStore.getState().dismissedTags).toEqual([
      "v1.4.0",
    ]);
  });

  it("tolerates corrupt JSON in localStorage by starting fresh", async () => {
    globalThis.localStorage?.setItem(
      "klaar.dismissedUpdateTags",
      "not json{{{",
    );
    const { useDismissedUpdateTagsStore } = await import(
      "./dismissedUpdateTagsStore"
    );
    expect(useDismissedUpdateTagsStore.getState().dismissedTags).toEqual([]);
  });

  it("tolerates an unexpected shape (non-array dismissedTags) by starting fresh", async () => {
    globalThis.localStorage?.setItem(
      "klaar.dismissedUpdateTags",
      JSON.stringify({ dismissedTags: "v1.4.0" }),
    );
    const { useDismissedUpdateTagsStore } = await import(
      "./dismissedUpdateTagsStore"
    );
    expect(useDismissedUpdateTagsStore.getState().dismissedTags).toEqual([]);
  });

  it("filters out non-string entries from a partially valid persisted payload", async () => {
    globalThis.localStorage?.setItem(
      "klaar.dismissedUpdateTags",
      JSON.stringify({ dismissedTags: ["v1.4.0", 42, null, "v1.5.0"] }),
    );
    const { useDismissedUpdateTagsStore } = await import(
      "./dismissedUpdateTagsStore"
    );
    expect(useDismissedUpdateTagsStore.getState().dismissedTags).toEqual([
      "v1.4.0",
      "v1.5.0",
    ]);
  });
});
