import { vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

import { _stepClipState } from "./engineStore";

interface ClipState {
  active: boolean;
  lastClearedAt: number;
}

const fresh = (): ClipState => ({ active: false, lastClearedAt: -Infinity });

describe("_stepClipState — accessibility clip detection", () => {
  it("fires once when peak crosses -1 dBFS upward", () => {
    const s = fresh();
    expect(_stepClipState(s, -2, 0)).toBe(false);
    expect(_stepClipState(s, -0.5, 100)).toBe(true);
    expect(s.active).toBe(true);
  });

  it("does not re-fire while peak stays above the enter threshold", () => {
    const s = fresh();
    expect(_stepClipState(s, -0.5, 0)).toBe(true);
    for (let t = 16; t < 1000; t += 16) {
      expect(_stepClipState(s, -0.5, t)).toBe(false);
    }
  });

  it("hysteresis: oscillation between -2 and -0.5 dBFS does not toggle", () => {
    const s = fresh();
    // Initial entry
    expect(_stepClipState(s, -0.5, 0)).toBe(true);
    // Bouncing inside the dead band [-3, -1] should not exit (need <= -3 to exit)
    expect(_stepClipState(s, -2, 16)).toBe(false);
    expect(s.active).toBe(true);
    expect(_stepClipState(s, -0.5, 32)).toBe(false);
    expect(s.active).toBe(true);
  });

  it("fires a second event after a clean exit (drop below -3 dBFS) plus quiet period", () => {
    const s = fresh();
    expect(_stepClipState(s, -0.5, 0)).toBe(true);
    // Drop below exit threshold
    expect(_stepClipState(s, -10, 100)).toBe(false);
    expect(s.active).toBe(false);
    // Within the quiet period — must not refire
    expect(_stepClipState(s, -0.5, 1000)).toBe(false);
    // After 3 s quiet period elapses — refire allowed
    expect(_stepClipState(s, -0.5, 100 + 3001)).toBe(true);
  });

  it("input and output channels are independent", () => {
    const inp = fresh();
    const out = fresh();
    expect(_stepClipState(inp, -0.5, 0)).toBe(true);
    expect(out.active).toBe(false);
    expect(_stepClipState(out, -0.5, 0)).toBe(true);
    expect(inp.active).toBe(true);
    expect(out.active).toBe(true);
  });
});
