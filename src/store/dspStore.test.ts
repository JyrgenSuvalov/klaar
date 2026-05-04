import { vi, beforeEach, afterEach, type MockInstance } from "vitest";

// Mock Tauri IPC before any store imports
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));

import { invoke } from "@tauri-apps/api/core";
import { useDspStore, DEFAULT_DSP_STATE } from "./dspStore";

// Helper to get current noiseGate threshold
const threshold = () => useDspStore.getState().noiseGate.threshold;
// Helper to get current EQ bands
const bands = () => useDspStore.getState().eq.bands;

// Silence console.error globally for this suite — the store intentionally
// logs IPC rejections, which would produce noise in expected error-path tests.
let consoleErrorSpy: MockInstance;

beforeEach(() => {
  // Reset store data to defaults before each test
  useDspStore.setState(structuredClone(DEFAULT_DSP_STATE));
  vi.clearAllMocks();
  // Default: invoke resolves immediately
  vi.mocked(invoke).mockResolvedValue(undefined);
  consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
});

afterEach(() => {
  consoleErrorSpy.mockRestore();
});

// ── setParam ──────────────────────────────────────────────────────────────────

describe("dspStore.setParam", () => {
  it("optimistically updates the store synchronously before IPC resolves", async () => {
    // invoke returns a never-resolving promise to hold the async path open
    let resolveInvoke!: () => void;
    vi.mocked(invoke).mockReturnValueOnce(
      new Promise<void>((r) => {
        resolveInvoke = r;
      }),
    );

    const promise = useDspStore.getState().setParam("noiseGate", "threshold", -30);

    // The set() call before `await invoke` is synchronous — value is updated immediately
    expect(threshold()).toBe(-30);

    resolveInvoke();
    await promise;
    expect(threshold()).toBe(-30); // unchanged after resolution
  });

  it("rolls back the optimistic update when IPC rejects", async () => {
    const prevThreshold = threshold(); // -80 (DEFAULT_DSP_STATE, matches Rust)

    vi.mocked(invoke).mockRejectedValueOnce(new Error("backend exploded"));

    await useDspStore.getState().setParam("noiseGate", "threshold", -30);

    expect(threshold()).toBe(prevThreshold);
  });

  it("leaves value in place when IPC succeeds", async () => {
    await useDspStore.getState().setParam("noiseGate", "threshold", -20);
    expect(threshold()).toBe(-20);
  });
});

// ── setEqBand ─────────────────────────────────────────────────────────────────

describe("dspStore.setEqBand", () => {
  it("applies partial updates to a valid band index", async () => {
    await useDspStore.getState().setEqBand(0, { gain: 6 });
    expect(bands()[0].gain).toBe(6);
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("rejects band index -1 without touching state or IPC", async () => {
    const snapshot = structuredClone(bands());
    await useDspStore.getState().setEqBand(-1, { gain: 6 });
    expect(invoke).not.toHaveBeenCalled();
    expect(bands()).toEqual(snapshot);
  });

  it("rejects band index 8 without touching state or IPC", async () => {
    const snapshot = structuredClone(bands());
    await useDspStore.getState().setEqBand(8, { gain: 6 });
    expect(invoke).not.toHaveBeenCalled();
    expect(bands()).toEqual(snapshot);
  });

  it("rolls back EQ band on IPC rejection", async () => {
    const prevGain = bands()[3].gain;
    vi.mocked(invoke).mockRejectedValueOnce(new Error("fail"));
    await useDspStore.getState().setEqBand(3, { gain: 12 });
    expect(bands()[3].gain).toBe(prevGain);
  });
});
