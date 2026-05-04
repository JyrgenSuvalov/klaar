import { vi, beforeEach, afterEach, type MockInstance } from "vitest";

// Mock Tauri IPC and event API before store import
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

import { useEngineStore, _resetSmoothingForTest } from "./engineStore";
import { invoke } from "@tauri-apps/api/core";

// ── Meter polling ─────────────────────────────────────────────────────────────

describe("engineStore.startMeterPolling", () => {
  beforeEach(() => {
    // Stub RAF/CAF so the poll callback never actually fires during tests
    vi.stubGlobal("requestAnimationFrame", vi.fn().mockReturnValue(42));
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
    // Reset polling state
    useEngineStore.setState({ _pollingRafId: null });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    // Clean up any lingering polling state
    useEngineStore.setState({ _pollingRafId: null });
  });

  it("starts a single RAF loop on first call", () => {
    useEngineStore.getState().startMeterPolling();
    expect(requestAnimationFrame).toHaveBeenCalledTimes(1);
    expect(useEngineStore.getState()._pollingRafId).not.toBeNull();
  });

  it("second call is a no-op when polling is already active", () => {
    useEngineStore.getState().startMeterPolling();
    useEngineStore.getState().startMeterPolling();
    // requestAnimationFrame should still have been called only once
    expect(requestAnimationFrame).toHaveBeenCalledTimes(1);
  });

  it("stopMeterPolling cancels the RAF and clears the ID", () => {
    useEngineStore.getState().startMeterPolling();
    useEngineStore.getState().stopMeterPolling();
    expect(cancelAnimationFrame).toHaveBeenCalledWith(42);
    expect(useEngineStore.getState()._pollingRafId).toBeNull();
  });

  it("can restart after stop", () => {
    useEngineStore.getState().startMeterPolling();
    useEngineStore.getState().stopMeterPolling();
    useEngineStore.getState().startMeterPolling();
    expect(requestAnimationFrame).toHaveBeenCalledTimes(2);
  });
});

// ── setProcessingEnabled ──────────────────────────────────────────────────────

describe("engineStore.setProcessingEnabled", () => {
  beforeEach(() => {
    useEngineStore.setState({ processingEnabled: true, engineError: null });
    vi.clearAllMocks();
    vi.mocked(invoke).mockResolvedValue(undefined);
  });

  it("optimistically updates processingEnabled before IPC resolves", async () => {
    let resolve!: () => void;
    vi.mocked(invoke).mockReturnValueOnce(new Promise<void>((r) => { resolve = r; }));

    const promise = useEngineStore.getState().setProcessingEnabled(false);
    expect(useEngineStore.getState().processingEnabled).toBe(false);

    resolve();
    await promise;
    expect(useEngineStore.getState().processingEnabled).toBe(false);
  });

  it("rolls back and sets engineError on IPC rejection", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("engine not started"));

    await useEngineStore.getState().setProcessingEnabled(false);

    expect(useEngineStore.getState().processingEnabled).toBe(true);
    expect(useEngineStore.getState().engineError).not.toBeNull();
    expect(useEngineStore.getState().engineError?.type).toBe("generic");
  });
});

// Silence console.error for this suite — fetchMeters logs IPC errors which
// would produce noise when the mock returns an unexpected value.
let consoleErrorSpy: MockInstance;

// ── Meter smoothing (attack / decay) ─────────────────────────────────────────

/** Minimal valid meter payload matching the MeterData shape expected by fetchMeters. */
const silenceMeterData = {
  inputLevel: -96,
  outputLevel: -96,
  gateReduction: 0,
  deEsserReduction: 0,
  deEsserSidechainDb: -96,
  compressorReduction: 0,
  limiterReduction: 0,
};

describe("engineStore meter smoothing", () => {
  beforeEach(() => {
    // Reset module-level smoothing state to -96 silence so each test is independent
    _resetSmoothingForTest();
    useEngineStore.setState({ _pollingRafId: null });
    vi.clearAllMocks();
    // Return valid meter data for get_meters; undefined for everything else.
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "get_meters") return Promise.resolve(silenceMeterData);
      return Promise.resolve(undefined);
    });
    consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    useEngineStore.setState({ _pollingRafId: null });
    consoleErrorSpy.mockRestore();
  });

  it("level meter: instant attack — smoothed snaps to louder raw value in one tick", () => {
    // Pre-condition: smoothed starts at -96 (silence). Raw becomes -20 (loud signal).
    // Expected: attack path fires → smoothedMeters.inputLevel === -20 after one poll tick.
    useEngineStore.setState({
      meters: {
        inputLevel: -20,
        outputLevel: -96,
        gateReduction: 0,
        deEsserReduction: 0,
        deEsserSidechainDb: -96,
        compressorReduction: 0,
        limiterReduction: 0,
      },
    });

    // Make RAF call the callback exactly once, then stop to avoid infinite recursion
    let ticked = false;
    vi.stubGlobal(
      "requestAnimationFrame",
      vi.fn().mockImplementation((cb: FrameRequestCallback) => {
        if (!ticked) {
          ticked = true;
          cb(0);
        }
        return 42;
      }),
    );
    vi.stubGlobal("cancelAnimationFrame", vi.fn());

    useEngineStore.getState().startMeterPolling();

    expect(useEngineStore.getState().smoothedMeters.inputLevel).toBe(-20);
  });

  it("level meter: slow decay — smoothed interpolates toward softer raw value in one tick", () => {
    // Pre-condition: set smoothing state to -20 (loud). Raw becomes -60 (softer).
    // Expected: decay path fires → smoothedMeters.inputLevel is between -20 and -60 (not snapped).
    _resetSmoothingForTest();
    // Bring _smoothed to -20 by running one attack tick first
    useEngineStore.setState({
      meters: {
        inputLevel: -20,
        outputLevel: -96,
        gateReduction: 0,
        deEsserReduction: 0,
        deEsserSidechainDb: -96,
        compressorReduction: 0,
        limiterReduction: 0,
      },
    });
    {
      let ticked = false;
      vi.stubGlobal(
        "requestAnimationFrame",
        vi.fn().mockImplementation((cb: FrameRequestCallback) => {
          if (!ticked) { ticked = true; cb(0); }
          return 42;
        }),
      );
      vi.stubGlobal("cancelAnimationFrame", vi.fn());
      useEngineStore.getState().startMeterPolling();
      useEngineStore.getState().stopMeterPolling();
      vi.unstubAllGlobals();
    }

    // Now _smoothed.inputLevel === -20. Set raw to -60 (softer) and run one more tick.
    useEngineStore.setState({
      _pollingRafId: null,
      meters: {
        inputLevel: -60,
        outputLevel: -96,
        gateReduction: 0,
        deEsserReduction: 0,
        deEsserSidechainDb: -96,
        compressorReduction: 0,
        limiterReduction: 0,
      },
    });

    let ticked = false;
    vi.stubGlobal(
      "requestAnimationFrame",
      vi.fn().mockImplementation((cb: FrameRequestCallback) => {
        if (!ticked) { ticked = true; cb(0); }
        return 42;
      }),
    );
    vi.stubGlobal("cancelAnimationFrame", vi.fn());

    useEngineStore.getState().startMeterPolling();

    const smoothed = useEngineStore.getState().smoothedMeters.inputLevel;
    // Decay: value should be between prev (-20) and raw (-60), exclusive
    expect(smoothed).toBeLessThan(-20);
    expect(smoothed).toBeGreaterThan(-60);
  });
});
