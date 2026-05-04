import { describe, it, expect, vi, beforeEach } from "vitest";

// Mock Tauri IPC + event listener before importing the module under test.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(),
}));

import {
  UPDATE_CHECK_ENABLED,
  UPDATE_STATUS_EVENT,
  subscribeUpdateStatus,
  invokeCheck,
  getUpdateStatus,
  type UpdateStatus,
} from "./updateCheck";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const invokeMock = invoke as unknown as ReturnType<typeof vi.fn>;
const listenMock = listen as unknown as ReturnType<typeof vi.fn>;

beforeEach(() => {
  invokeMock.mockReset();
  listenMock.mockReset();
});

describe("UPDATE_CHECK_ENABLED kill-switch", () => {
  it("is enabled now that JyrgenSuvalov/klaar is public", () => {
    // The kill-switch flipped to `true` when the source repo went public.
    // Re-flipping to `false` would suppress all triggers and IPC; only do
    // that intentionally (e.g., if rate-limit issues recur).
    expect(UPDATE_CHECK_ENABLED).toBe(true);
  });
});

describe("subscribeUpdateStatus", () => {
  it("attaches a listener on the update_status channel and returns the unlisten fn", async () => {
    const fakeUnlisten = vi.fn();
    listenMock.mockResolvedValueOnce(fakeUnlisten);
    const handler = vi.fn();

    const unlisten = await subscribeUpdateStatus(handler);

    expect(listenMock).toHaveBeenCalledTimes(1);
    expect(listenMock).toHaveBeenCalledWith(
      UPDATE_STATUS_EVENT,
      expect.any(Function),
    );
    expect(unlisten).toBe(fakeUnlisten);

    // Verify the wrapped callback unwraps `event.payload` before invoking
    // the user's handler — we want the public surface to deal in payloads,
    // not Tauri Event<T> objects.
    const wrapped = listenMock.mock.calls[0]?.[1] as (e: {
      payload: UpdateStatus;
    }) => void;
    const payload: UpdateStatus = { kind: "idle" };
    wrapped({ payload });
    expect(handler).toHaveBeenCalledWith(payload);
  });
});

describe("invokeCheck", () => {
  it("invokes check_for_app_update with the force flag", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    await invokeCheck(true);
    expect(invokeMock).toHaveBeenCalledWith("check_for_app_update", {
      force: true,
    });
  });

  it("swallows IPC errors so a tray click never throws", async () => {
    invokeMock.mockRejectedValueOnce(new Error("boom"));
    // Should resolve, not throw.
    await expect(invokeCheck(false)).resolves.toBeUndefined();
  });
});

describe("getUpdateStatus", () => {
  it("returns the Rust snapshot on success", async () => {
    const snapshot: UpdateStatus = {
      kind: "available",
      tag: "v1.4.0",
      url: "https://example/v1.4.0",
      checked_at: "2026-04-01T00:00:00Z",
    };
    invokeMock.mockResolvedValueOnce(snapshot);
    expect(await getUpdateStatus()).toEqual(snapshot);
    expect(invokeMock).toHaveBeenCalledWith("get_update_status");
  });

  it("falls back to idle when the IPC errors", async () => {
    invokeMock.mockRejectedValueOnce(new Error("boom"));
    expect(await getUpdateStatus()).toEqual({ kind: "idle" });
  });
});

describe("UpdateStatus shape (IPC contract)", () => {
  it("has the five tagged variants matching the Rust enum", () => {
    const samples: UpdateStatus[] = [
      { kind: "idle" },
      { kind: "checking", manual: true },
      { kind: "up_to_date", checked_at: "2026-04-01T00:00:00Z" },
      {
        kind: "available",
        tag: "v1.4.0",
        url: "https://example/v1.4.0",
        checked_at: "2026-04-01T00:00:00Z",
      },
      { kind: "error", error: "network", checked_at: "2026-04-01T00:00:00Z" },
    ];
    for (const s of samples) {
      const json = JSON.stringify(s);
      const parsed = JSON.parse(json) as UpdateStatus;
      expect(parsed.kind).toBe(s.kind);
    }
  });

  it("event channel name matches the Rust constant", () => {
    expect(UPDATE_STATUS_EVENT).toBe("update_status");
  });
});
