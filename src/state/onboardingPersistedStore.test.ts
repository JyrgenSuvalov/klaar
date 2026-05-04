/**
 * Tests for `onboardingPersistedStore`.
 *
 * Coverage:
 *   - fresh state (no file → `getRebootPending()` is false)
 *   - match-case (boot timestamps equal → true)
 *   - reboot-detected (timestamps differ → false AND clears the file)
 *   - corrupt-JSON (`read_onboarding_state` rejects → false AND clears)
 *   - `setRebootPending` records the current boot timestamp
 *   - `clearRebootPending` removes the key without touching siblings
 *   - `get_boot_timestamp === 0` (sysctl unavailable) → conservative `true`
 */

import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
import {
  getRebootPending,
  setRebootPending,
  clearRebootPending,
  type OnboardingPersistedState,
} from "./onboardingPersistedStore";

const invokeMock = vi.mocked(invoke);

/**
 * Build an `invoke` mock that simulates the Rust IPC pair on top of an
 * in-memory file. `bootTimestamp` is what `get_boot_timestamp` returns.
 * If `corruptOnRead` is true, `read_onboarding_state` rejects on the first
 * call (and only the first call) — modelling JSON parse failure.
 */
function mockBackend(opts: {
  initialFile?: OnboardingPersistedState | null;
  bootTimestamp: number;
  corruptOnRead?: boolean;
}) {
  let file: OnboardingPersistedState | null = opts.initialFile ?? null;
  let firstReadDone = false;
  // Track the in-memory engine atomic so tests can assert that the store
  // keeps it in lock-step with the file.
  let engineRebootPending: boolean | null = null;

  invokeMock.mockImplementation((cmd: string, args?: unknown) => {
    switch (cmd) {
      case "read_onboarding_state":
        if (opts.corruptOnRead && !firstReadDone) {
          firstReadDone = true;
          return Promise.reject(
            new Error("parse onboarding state: expected `,` or `}`"),
          );
        }
        firstReadDone = true;
        return Promise.resolve(file);
      case "write_onboarding_state":
        file = (args as { state: OnboardingPersistedState }).state;
        return Promise.resolve(undefined);
      case "get_boot_timestamp":
        return Promise.resolve(opts.bootTimestamp);
      case "set_reboot_pending":
        engineRebootPending = (args as { pending: boolean }).pending;
        return Promise.resolve(undefined);
      default:
        return Promise.reject(new Error(`unexpected IPC: ${cmd}`));
    }
  });

  return {
    get file() {
      return file;
    },
    get engineRebootPending() {
      return engineRebootPending;
    },
  };
}

beforeEach(() => {
  invokeMock.mockReset();
});

describe("getRebootPending", () => {
  it("returns false when no file exists (fresh install)", async () => {
    mockBackend({ initialFile: null, bootTimestamp: 1_700_000_000 });
    expect(await getRebootPending()).toBe(false);
  });

  it("returns false when the file exists but rebootPending is unset", async () => {
    mockBackend({ initialFile: {}, bootTimestamp: 1_700_000_000 });
    expect(await getRebootPending()).toBe(false);
  });

  it("returns true when persisted bootTimestamp matches the current OS value", async () => {
    mockBackend({
      initialFile: {
        rebootPending: { value: true, bootTimestamp: 1_700_000_000 },
      },
      bootTimestamp: 1_700_000_000,
    });
    expect(await getRebootPending()).toBe(true);
  });

  it("returns false AND clears the file when timestamps differ (reboot detected)", async () => {
    const backend = mockBackend({
      initialFile: {
        rebootPending: { value: true, bootTimestamp: 1_700_000_000 },
      },
      bootTimestamp: 1_700_010_000, // newer boottime → user did reboot
    });
    expect(await getRebootPending()).toBe(false);
    // File should now be clear of the rebootPending key.
    expect(backend.file?.rebootPending).toBeUndefined();
  });

  it("returns false AND clears the file when read_onboarding_state errors (corrupt JSON)", async () => {
    const backend = mockBackend({
      initialFile: {
        rebootPending: { value: true, bootTimestamp: 1_700_000_000 },
      },
      bootTimestamp: 1_700_000_000,
      corruptOnRead: true,
    });
    expect(await getRebootPending()).toBe(false);
    // The corrupt-read path wrote `{}` to wipe the file.
    expect(backend.file).toEqual({});
  });

  it("conservatively returns true when get_boot_timestamp returns 0 (sysctl unavailable)", async () => {
    mockBackend({
      initialFile: {
        rebootPending: { value: true, bootTimestamp: 1_700_000_000 },
      },
      bootTimestamp: 0,
    });
    // Cannot verify reboot occurred → keep the flag, don't silently re-enable
    // the audio engine.
    expect(await getRebootPending()).toBe(true);
  });
});

describe("setRebootPending", () => {
  it("writes the current boot timestamp to the file", async () => {
    const backend = mockBackend({ initialFile: null, bootTimestamp: 1_700_000_000 });
    await setRebootPending();
    expect(backend.file?.rebootPending).toEqual({
      value: true,
      bootTimestamp: 1_700_000_000,
    });
  });

  it("preserves unrelated keys when extending an existing file", async () => {
    // Forward-compat guard: when future onboarding flags share this file,
    // setRebootPending must not stomp on them.
    const backend = mockBackend({
      initialFile: { unknownFutureKey: "preserved" } as unknown as OnboardingPersistedState,
      bootTimestamp: 1_700_000_000,
    });
    await setRebootPending();
    expect(
      (backend.file as unknown as Record<string, unknown>)["unknownFutureKey"],
    ).toBe("preserved");
    expect(backend.file?.rebootPending?.bootTimestamp).toBe(1_700_000_000);
  });
});

describe("clearRebootPending", () => {
  it("removes the rebootPending key without touching siblings", async () => {
    const backend = mockBackend({
      initialFile: {
        rebootPending: { value: true, bootTimestamp: 1_700_000_000 },
      },
      bootTimestamp: 1_700_000_000,
    });
    await clearRebootPending();
    expect(backend.file?.rebootPending).toBeUndefined();
    // Field is omitted entirely (not serialised as `undefined`).
    expect(Object.prototype.hasOwnProperty.call(backend.file, "rebootPending")).toBe(false);
  });

  it("is a no-op when rebootPending is already absent", async () => {
    const backend = mockBackend({ initialFile: {}, bootTimestamp: 1_700_000_000 });
    await clearRebootPending();
    // No write_onboarding_state call should have fired.
    expect(invokeMock).not.toHaveBeenCalledWith(
      "write_onboarding_state",
      expect.anything(),
    );
    expect(backend.file).toEqual({});
    // Still nudge the engine atomic to false so a stale in-memory value
    // can't outlive a missing file.
    expect(backend.engineRebootPending).toBe(false);
  });
});

// The file-backed store must keep the backend's in-memory `reboot_pending`
// atomic in lock-step with every write/clear. These assertions guard the
// contract documented in `set_reboot_pending` IPC.
describe("engine reboot_pending sync", () => {
  it("setRebootPending invokes set_reboot_pending(true)", async () => {
    const backend = mockBackend({
      initialFile: {},
      bootTimestamp: 1_700_000_000,
    });
    await setRebootPending();
    expect(backend.engineRebootPending).toBe(true);
    expect(invokeMock).toHaveBeenCalledWith("set_reboot_pending", {
      pending: true,
    });
  });

  it("clearRebootPending invokes set_reboot_pending(false) when removing the key", async () => {
    const backend = mockBackend({
      initialFile: {
        rebootPending: { value: true, bootTimestamp: 1_700_000_000 },
      },
      bootTimestamp: 1_700_000_000,
    });
    await clearRebootPending();
    expect(backend.engineRebootPending).toBe(false);
  });

  it("getRebootPending → reboot detected path syncs the engine atomic to false", async () => {
    const backend = mockBackend({
      initialFile: {
        rebootPending: { value: true, bootTimestamp: 1_700_000_000 },
      },
      bootTimestamp: 1_700_010_000, // newer boot → reboot detected
    });
    expect(await getRebootPending()).toBe(false);
    expect(backend.engineRebootPending).toBe(false);
  });
});
