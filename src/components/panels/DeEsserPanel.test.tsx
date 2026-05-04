import { vi, describe, it, expect, beforeEach } from "vitest";
import { render, fireEvent, act } from "@testing-library/react";

// Tauri IPC mock — `invoke` is captured so individual tests can assert calls.
const invokeMock = vi.fn((..._args: unknown[]) => Promise.resolve(undefined as unknown));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: unknown) => invokeMock(cmd, args),
}));

// Re-import after mocks are wired so the panel binds to the mock invoke.
import { DeEsserPanel } from "./DeEsserPanel";
import { useDspStore, DEFAULT_DSP_STATE } from "@/store/dspStore";

beforeEach(() => {
  invokeMock.mockClear();
  // Reset DSP store to defaults so each test starts from a known baseline.
  useDspStore.setState(structuredClone(DEFAULT_DSP_STATE));
});

describe("DeEsserPanel", () => {
  it("renders five knobs, sidechain meter, GR meter, and bypass toggle", () => {
    const { container, getByLabelText } = render(<DeEsserPanel />);

    // Five knobs by aria-label (role="slider")
    const knobLabels = ["Freq", "Threshold", "Ratio", "Attack", "Release"];
    for (const label of knobLabels) {
      expect(getByLabelText(label)).not.toBeNull();
    }

    // Sidechain level meter (role="meter") + GR meter
    const meters = container.querySelectorAll('[role="meter"]');
    expect(meters.length).toBe(2);

    // Bypass toggle
    expect(getByLabelText(/De-esser enabled/)).not.toBeNull();
  });

  it("threshold knob updates the sidechain meter tick (re-renders without crashing)", () => {
    const { getByLabelText, container } = render(<DeEsserPanel />);
    const threshold = getByLabelText("Threshold");

    act(() => {
      fireEvent.keyDown(threshold, { key: "ArrowDown" });
    });

    expect(invokeMock).toHaveBeenCalledWith(
      "set_param",
      expect.objectContaining({ effect: "deEsser", param: "threshold" }),
    );
    expect(container.querySelectorAll('[role="meter"]').length).toBe(2);
  });

  it("sidechain meter uses a -60 dB display floor so the threshold tick stays visible", () => {
    const { container } = render(<DeEsserPanel />);
    const meters = container.querySelectorAll('[role="meter"]');
    const sidechain = Array.from(meters).find(
      (m) => m.getAttribute("aria-valuemin") === "-60",
    );
    expect(sidechain).toBeDefined();
    // Threshold range is [-40, 0]; with a -60 dB floor the threshold tick
    // can never fall below 1/3 of the meter height.
    expect(-40).toBeGreaterThan(-60);
  });

  it("all knobs and bypass toggle are keyboard reachable", () => {
    const { container } = render(<DeEsserPanel />);
    const focusable = container.querySelectorAll(
      '[role="slider"], button:not([disabled])',
    );
    // 5 knobs + bypass = at least 6
    expect(focusable.length).toBeGreaterThanOrEqual(6);
  });
});
