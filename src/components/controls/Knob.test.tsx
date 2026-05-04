import { render, fireEvent } from "@testing-library/react";
import { Knob } from "./Knob";
import {
  applyKeyboardDelta,
  computeKeyboardStep,
  normToValue,
  valueToNorm,
} from "./Knob.helpers";

describe("Knob", () => {
  it("renders an SVG element", () => {
    const { container } = render(
      <Knob label="Gain" value={50} min={0} max={100} defaultValue={0} onChange={() => {}} />,
    );
    expect(container.querySelector("svg")).not.toBeNull();
  });

  it("renders value display text", () => {
    const { getByText } = render(
      <Knob label="Threshold" value={-18} min={-60} max={0} defaultValue={0} onChange={() => {}} unit="dB" />,
    );
    // formatValue: abs(-18) = 18 ≥ 10 → toFixed(0), so "-18 dB"
    expect(getByText("-18 dB")).not.toBeNull();
  });

  it("double-click calls onChange with defaultValue", () => {
    const onChange = vi.fn();
    const { container } = render(
      <Knob label="Gain" value={50} min={0} max={100} defaultValue={25} onChange={onChange} />,
    );
    const svg = container.querySelector("svg");
    if (!svg) throw new Error("SVG not found");
    fireEvent.dblClick(svg);
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith(25);
  });

  it("double-click is a no-op when disabled", () => {
    const onChange = vi.fn();
    const { container } = render(
      <Knob label="Gain" value={50} min={0} max={100} defaultValue={25} onChange={onChange} disabled />,
    );
    const svg = container.querySelector("svg");
    if (!svg) throw new Error("SVG not found");
    fireEvent.dblClick(svg);
    expect(onChange).not.toHaveBeenCalled();
  });

  it("log scale: 50% normalised drag position yields geometric midpoint (~632 Hz)", () => {
    // With scale="log", min=20, max=20000:
    // geometric midpoint = sqrt(20 * 20000) ≈ 632 Hz, NOT the linear midpoint (10010 Hz).
    // DRAG_SENSITIVITY = 200px for full range. 50% drag = 100px upward (dy = -100).
    const onChange = vi.fn();
    const { container } = render(
      <Knob
        label="Freq"
        value={20}
        min={20}
        max={20000}
        defaultValue={1000}
        unit="Hz"
        scale="log"
        onChange={onChange}
      />,
    );
    const svg = container.querySelector("svg");
    if (!svg) throw new Error("SVG not found");

    // Start drag at value=20 (min), drag 100px upward → 50% of log range
    fireEvent.mouseDown(svg, { clientY: 200 });
    fireEvent.mouseMove(window, { clientY: 100 }); // dy = -100

    expect(onChange).toHaveBeenCalled();
    const reported = onChange.mock.calls[onChange.mock.calls.length - 1]?.[0] as number;
    // Geometric midpoint: sqrt(20 * 20000) ≈ 632.5. Accept ±10 Hz tolerance.
    expect(reported).toBeGreaterThan(620);
    expect(reported).toBeLessThan(645);
  });

  it("drag change is clamped to [min, max]", () => {
    const onChange = vi.fn();
    const { container } = render(
      // Start at max so dragging further up cannot go beyond max
      <Knob label="Gain" value={100} min={0} max={100} defaultValue={50} onChange={onChange} />,
    );
    const svg = container.querySelector("svg");
    if (!svg) throw new Error("SVG not found");

    // Mousedown records startValue = 100
    fireEvent.mouseDown(svg, { clientY: 200 });
    // Huge upward drag — delta would push value far above max
    fireEvent.mouseMove(window, { clientY: -5000 });

    // All onChange calls should have been clamped to max (100)
    for (const call of onChange.mock.calls) {
      expect(call[0]).toBeLessThanOrEqual(100);
      expect(call[0]).toBeGreaterThanOrEqual(0);
    }
    // At least one call should have occurred
    expect(onChange).toHaveBeenCalled();
    // The last reported value should be at max
    const lastValue = onChange.mock.calls[onChange.mock.calls.length - 1]?.[0] as number;
    expect(lastValue).toBe(100);
  });

  it("clicking value text enters edit mode with an input field", () => {
    const { getByText, container } = render(
      <Knob label="Gain" value={5.2} min={0} max={10} defaultValue={5} onChange={() => {}} unit="dB" />,
    );
    const valueSpan = getByText("5.2 dB");
    fireEvent.click(valueSpan);
    const input = container.querySelector("input");
    expect(input).not.toBeNull();
    expect(input!.value).toBe("5.2");
  });

  it("pressing Enter in edit mode commits the new value", () => {
    const onChange = vi.fn();
    const { getByText, container } = render(
      <Knob label="Threshold" value={-18} min={-60} max={0} defaultValue={-18} onChange={onChange} unit="dB" />,
    );
    fireEvent.click(getByText("-18 dB"));
    const input = container.querySelector("input")!;
    fireEvent.change(input, { target: { value: "-24" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onChange).toHaveBeenCalledWith(-24);
  });

  it("pressing Escape in edit mode discards changes", () => {
    const onChange = vi.fn();
    const { getByText, container } = render(
      <Knob label="Gain" value={50} min={0} max={100} defaultValue={50} onChange={onChange} />,
    );
    fireEvent.click(getByText("50"));
    const input = container.querySelector("input")!;
    fireEvent.change(input, { target: { value: "999" } });
    fireEvent.keyDown(input, { key: "Escape" });
    // onChange should NOT have been called with the edited value
    expect(onChange).not.toHaveBeenCalled();
  });

  it("edit mode clamps value to [min, max]", () => {
    const onChange = vi.fn();
    const { getByText, container } = render(
      <Knob label="Gain" value={50} min={0} max={100} defaultValue={50} onChange={onChange} />,
    );
    fireEvent.click(getByText("50"));
    const input = container.querySelector("input")!;
    fireEvent.change(input, { target: { value: "200" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onChange).toHaveBeenCalledWith(100);
  });

  it("edit mode ignores non-numeric input", () => {
    const onChange = vi.fn();
    const { getByText, container } = render(
      <Knob label="Gain" value={50} min={0} max={100} defaultValue={50} onChange={onChange} />,
    );
    fireEvent.click(getByText("50"));
    const input = container.querySelector("input")!;
    fireEvent.change(input, { target: { value: "abc" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onChange).not.toHaveBeenCalled();
  });

  it("clicking value is a no-op when disabled", () => {
    const { getByText, container } = render(
      <Knob label="Gain" value={50} min={0} max={100} defaultValue={50} onChange={() => {}} disabled />,
    );
    fireEvent.click(getByText("50"));
    expect(container.querySelector("input")).toBeNull();
  });

  it("blur commits the edit", () => {
    const onChange = vi.fn();
    const { getByText, container } = render(
      <Knob label="Gain" value={50} min={0} max={100} defaultValue={50} onChange={onChange} />,
    );
    fireEvent.click(getByText("50"));
    const input = container.querySelector("input")!;
    fireEvent.change(input, { target: { value: "75" } });
    fireEvent.blur(input);
    expect(onChange).toHaveBeenCalledWith(75);
  });

  // ── Accessibility / keyboard ─────────────────────────────────────────────

  describe("ARIA semantics", () => {
    it("exposes role=slider and ARIA value attributes", () => {
      const { container } = render(
        <Knob label="Threshold" value={-18} min={-60} max={0} defaultValue={0} onChange={() => {}} unit="dB" />,
      );
      const svg = container.querySelector("svg")!;
      expect(svg.getAttribute("role")).toBe("slider");
      expect(svg.getAttribute("aria-label")).toBe("Threshold");
      expect(svg.getAttribute("aria-valuemin")).toBe("-60");
      expect(svg.getAttribute("aria-valuemax")).toBe("0");
      expect(svg.getAttribute("aria-valuenow")).toBe("-18");
      expect(svg.getAttribute("aria-valuetext")).toBe("-18 dB");
      expect(svg.getAttribute("tabindex")).toBe("0");
    });

    it("ariaLabel overrides label for screen readers", () => {
      const { container } = render(
        <Knob
          label="Threshold"
          ariaLabel="Compressor Threshold"
          value={-18}
          min={-60}
          max={0}
          defaultValue={0}
          onChange={() => {}}
        />,
      );
      expect(container.querySelector("svg")!.getAttribute("aria-label")).toBe("Compressor Threshold");
    });

    it("disabled knob has aria-disabled and tabIndex=-1, ignores keyboard", () => {
      const onChange = vi.fn();
      const { container } = render(
        <Knob label="Gain" value={50} min={0} max={100} defaultValue={50} onChange={onChange} disabled />,
      );
      const svg = container.querySelector("svg")!;
      expect(svg.getAttribute("aria-disabled")).toBe("true");
      expect(svg.getAttribute("tabindex")).toBe("-1");
      fireEvent.keyDown(svg, { key: "ArrowUp" });
      expect(onChange).not.toHaveBeenCalled();
    });
  });

  describe("keyboard interaction", () => {
    function renderKnob(props: Partial<React.ComponentProps<typeof Knob>> = {}) {
      const onChange = vi.fn();
      const utils = render(
        <Knob
          label="Gain"
          value={50}
          min={0}
          max={100}
          defaultValue={50}
          onChange={onChange}
          {...props}
        />,
      );
      return { onChange, ...utils };
    }

    it("ArrowUp on linear scale increases by 1% of range", () => {
      const { container, onChange } = renderKnob();
      fireEvent.keyDown(container.querySelector("svg")!, { key: "ArrowUp" });
      expect(onChange).toHaveBeenCalledWith(51);
    });

    it("ArrowRight behaves the same as ArrowUp", () => {
      const { container, onChange } = renderKnob();
      fireEvent.keyDown(container.querySelector("svg")!, { key: "ArrowRight" });
      expect(onChange).toHaveBeenCalledWith(51);
    });

    it("ArrowDown decreases by 1% of range", () => {
      const { container, onChange } = renderKnob();
      fireEvent.keyDown(container.querySelector("svg")!, { key: "ArrowDown" });
      expect(onChange).toHaveBeenCalledWith(49);
    });

    it("Shift+ArrowUp performs 0.1× step (fine)", () => {
      const { container, onChange } = renderKnob();
      fireEvent.keyDown(container.querySelector("svg")!, { key: "ArrowUp", shiftKey: true });
      // 50 + 0.1 * 1 = 50.1
      expect(onChange).toHaveBeenCalledWith(50.1);
    });

    it("PageUp performs 10× step (coarse)", () => {
      const { container, onChange } = renderKnob();
      fireEvent.keyDown(container.querySelector("svg")!, { key: "PageUp" });
      expect(onChange).toHaveBeenCalledWith(60);
    });

    it("PageDown performs -10× step", () => {
      const { container, onChange } = renderKnob();
      fireEvent.keyDown(container.querySelector("svg")!, { key: "PageDown" });
      expect(onChange).toHaveBeenCalledWith(40);
    });

    it("Home jumps to min", () => {
      const { container, onChange } = renderKnob();
      fireEvent.keyDown(container.querySelector("svg")!, { key: "Home" });
      expect(onChange).toHaveBeenCalledWith(0);
    });

    it("End jumps to max", () => {
      const { container, onChange } = renderKnob();
      fireEvent.keyDown(container.querySelector("svg")!, { key: "End" });
      expect(onChange).toHaveBeenCalledWith(100);
    });

    it("clamps at max on PageUp near upper bound", () => {
      const { container, onChange } = renderKnob({ value: 95 });
      fireEvent.keyDown(container.querySelector("svg")!, { key: "PageUp" });
      expect(onChange).toHaveBeenCalledWith(100);
    });

    it("clamps at min on PageDown near lower bound", () => {
      const { container, onChange } = renderKnob({ value: 5 });
      fireEvent.keyDown(container.querySelector("svg")!, { key: "PageDown" });
      expect(onChange).toHaveBeenCalledWith(0);
    });

    it("rounds to `decimals` prop", () => {
      // Range 0..100 → 1% step = 1 → Shift = 0.1 → starting at 0.123 → ~0.223 → rounded to 2 decimals = 0.22
      const onChange = vi.fn();
      const { container } = render(
        <Knob
          label="Q"
          value={0.123}
          min={0}
          max={100}
          defaultValue={0}
          onChange={onChange}
          decimals={2}
        />,
      );
      fireEvent.keyDown(container.querySelector("svg")!, { key: "ArrowUp", shiftKey: true });
      expect(onChange).toHaveBeenCalledWith(0.22);
    });

    it("log scale ArrowUp produces a multiplicative step", () => {
      // For 20..20000: factor = (20000/20)^(1/100) ≈ 1.0715
      // Starting at 1000, ArrowUp → ~1071.5 (within a small tolerance after rounding to 2 decimals)
      const onChange = vi.fn();
      const { container } = render(
        <Knob
          label="Freq"
          value={1000}
          min={20}
          max={20000}
          defaultValue={1000}
          onChange={onChange}
          scale="log"
          unit="Hz"
        />,
      );
      fireEvent.keyDown(container.querySelector("svg")!, { key: "ArrowUp" });
      const reported = onChange.mock.calls[0]?.[0] as number;
      expect(reported).toBeGreaterThan(1065);
      expect(reported).toBeLessThan(1080);
    });

    it("`step` prop is honored when set", () => {
      const onChange = vi.fn();
      const { container } = render(
        <Knob
          label="Gain"
          value={50}
          min={0}
          max={100}
          defaultValue={50}
          onChange={onChange}
          step={5}
        />,
      );
      fireEvent.keyDown(container.querySelector("svg")!, { key: "ArrowUp" });
      expect(onChange).toHaveBeenCalledWith(55);
    });

    it("`keyboardStep` overrides both `step` and the default", () => {
      const onChange = vi.fn();
      const { container } = render(
        <Knob
          label="Gain"
          value={50}
          min={0}
          max={100}
          defaultValue={50}
          onChange={onChange}
          step={5}
          keyboardStep={2}
        />,
      );
      fireEvent.keyDown(container.querySelector("svg")!, { key: "ArrowUp" });
      expect(onChange).toHaveBeenCalledWith(52);
    });
  });

  describe("computeKeyboardStep", () => {
    it("linear default = 1% of range", () => {
      expect(computeKeyboardStep(0, 100, "linear", 0)).toBe(1);
      expect(computeKeyboardStep(-60, 0, "linear", 0)).toBeCloseTo(0.6);
    });
    it("linear honours step prop when > 0", () => {
      expect(computeKeyboardStep(0, 100, "linear", 5)).toBe(5);
    });
    it("linear keyboardStep wins over step", () => {
      expect(computeKeyboardStep(0, 100, "linear", 5, 2)).toBe(2);
    });
    it("log default = (max/min)^(1/100)", () => {
      const f = computeKeyboardStep(20, 20000, "log", 0);
      expect(f).toBeCloseTo(Math.pow(20000 / 20, 1 / 100), 6);
    });
  });

  describe("applyKeyboardDelta", () => {
    it("Home / End sentinels snap to bounds", () => {
      expect(applyKeyboardDelta(50, Number.POSITIVE_INFINITY, 0, 100, "linear", 0, undefined)).toBe(100);
      expect(applyKeyboardDelta(50, Number.NEGATIVE_INFINITY, 0, 100, "linear", 0, undefined)).toBe(0);
    });
    it("clamps to [min, max]", () => {
      expect(applyKeyboardDelta(99, +10, 0, 100, "linear", 0, undefined)).toBe(100);
      expect(applyKeyboardDelta(1, -10, 0, 100, "linear", 0, undefined)).toBe(0);
    });
  });

  // ── Log-scale value/position mapping (task 4.3 of improve-deesser-tweaking) ──
  //
  // The de-esser Frequency knob spans 2000–12000 Hz on a log scale so the
  // perceptually-uniform centre lies at the geometric mean (~4899 Hz), not
  // the arithmetic mean (7000 Hz). Verify the helpers underpinning this.
  describe("log scale mapping (de-esser frequency 2000–12000)", () => {
    const MIN = 2000;
    const MAX = 12000;
    // Geometric mean: sqrt(2000 * 12000) = sqrt(24_000_000) ≈ 4898.979485566356
    const GEOMETRIC_MID = Math.sqrt(MIN * MAX);

    it("normalised position 0 yields min", () => {
      expect(normToValue(0, MIN, MAX, "log")).toBeCloseTo(MIN, 6);
    });

    it("normalised position 1 yields max", () => {
      expect(normToValue(1, MIN, MAX, "log")).toBeCloseTo(MAX, 6);
    });

    it("normalised position 0.5 yields the geometric midpoint (~4899 Hz)", () => {
      const mid = normToValue(0.5, MIN, MAX, "log");
      expect(mid).toBeCloseTo(GEOMETRIC_MID, 3);
      // Sanity-check the rounded human-readable midpoint cited in the spec.
      expect(Math.round(mid)).toBe(4899);
    });

    it("position → value → position is identity across the range", () => {
      for (const norm of [0, 0.1, 0.25, 0.5, 0.6, 0.75, 0.9, 1]) {
        const value = normToValue(norm, MIN, MAX, "log");
        const back = valueToNorm(value, MIN, MAX, "log");
        expect(back).toBeCloseTo(norm, 9);
      }
    });

    it("value → position → value is identity for spec-mentioned anchors", () => {
      for (const value of [2000, 4899, 6000, 8000, 12000]) {
        const norm = valueToNorm(value, MIN, MAX, "log");
        const back = normToValue(norm, MIN, MAX, "log");
        expect(back).toBeCloseTo(value, 3);
      }
    });
  });
});
