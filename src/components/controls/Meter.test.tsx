import { render, fireEvent } from "@testing-library/react";
import { Meter } from "./Meter";
import { GainReductionMeter } from "./GainReductionMeter";

describe("Meter accessibility", () => {
  it("exposes role=meter, tabIndex=0, label, and static range; valuenow/text appear only on focus", () => {
    const { getByRole } = render(
      <Meter level={-12} peakLevel={-6} ariaLabel="Input level" floor={-60} />,
    );
    const meter = getByRole("meter");
    expect(meter.getAttribute("aria-label")).toBe("Input level");
    expect(meter.getAttribute("tabindex")).toBe("0");
    expect(meter.getAttribute("aria-valuemin")).toBe("-60");
    expect(meter.getAttribute("aria-valuemax")).toBe("0");
    // Unfocused: no value attrs (prevents VO chatter on continuous updates)
    expect(meter.getAttribute("aria-valuenow")).toBeNull();
    expect(meter.getAttribute("aria-valuetext")).toBeNull();
  });

  it("focus snapshots the current level into aria-valuenow / aria-valuetext", () => {
    const { getByRole } = render(
      <Meter level={-12} ariaLabel="Input level" floor={-60} />,
    );
    const meter = getByRole("meter");
    fireEvent.focus(meter);
    expect(meter.getAttribute("aria-valuenow")).toBe("-12");
    expect(meter.getAttribute("aria-valuetext")).toBe("-12 dB");
  });

  it("blur clears the snapshot so subsequent updates do not chatter", () => {
    const { getByRole } = render(
      <Meter level={-12} ariaLabel="Input level" floor={-60} />,
    );
    const meter = getByRole("meter");
    fireEvent.focus(meter);
    fireEvent.blur(meter);
    expect(meter.getAttribute("aria-valuenow")).toBeNull();
    expect(meter.getAttribute("aria-valuetext")).toBeNull();
  });

  it("announces silence as 'Below {floor} dB' when focused at floor", () => {
    const { getByRole } = render(
      <Meter level={-96} ariaLabel="Input level" floor={-60} />,
    );
    const meter = getByRole("meter");
    fireEvent.focus(meter);
    expect(meter.getAttribute("aria-valuetext")).toBe("Below -60 dB");
  });

  it("default ariaLabel is 'Level'", () => {
    const { getByRole } = render(<Meter level={-20} />);
    expect(getByRole("meter").getAttribute("aria-label")).toBe("Level");
  });

  it("has focus-visible ring class on the meter wrapper", () => {
    const { getByRole } = render(<Meter level={-12} ariaLabel="Input level" />);
    expect(getByRole("meter").className).toMatch(/focus-visible:ring/);
  });
});

describe("GainReductionMeter accessibility", () => {
  it("exposes role=meter, tabIndex=0, label, and static range; valuenow/text appear only on focus", () => {
    const { getByRole } = render(
      <GainReductionMeter
        reduction={-3.4}
        ariaLabel="Compressor gain reduction"
        maxReduction={30}
      />,
    );
    const meter = getByRole("meter");
    expect(meter.getAttribute("aria-label")).toBe("Compressor gain reduction");
    expect(meter.getAttribute("tabindex")).toBe("0");
    expect(meter.getAttribute("aria-valuemin")).toBe("-30");
    expect(meter.getAttribute("aria-valuemax")).toBe("0");
    expect(meter.getAttribute("aria-valuenow")).toBeNull();
    expect(meter.getAttribute("aria-valuetext")).toBeNull();
  });

  it("focus snapshots the current reduction into aria-value*", () => {
    const { getByRole } = render(
      <GainReductionMeter reduction={-3.4} ariaLabel="GR" maxReduction={30} />,
    );
    const meter = getByRole("meter");
    fireEvent.focus(meter);
    expect(meter.getAttribute("aria-valuenow")).toBe("-3.4");
    expect(meter.getAttribute("aria-valuetext")).toBe("-3.4 dB");
  });

  it("announces 0 dB on focus when there is no reduction", () => {
    const { getByRole } = render(
      <GainReductionMeter reduction={0} ariaLabel="GR" />,
    );
    const meter = getByRole("meter");
    fireEvent.focus(meter);
    expect(meter.getAttribute("aria-valuetext")).toBe("0 dB");
  });

  it("clamps focused aria-valuenow to maxReduction floor", () => {
    const { getByRole } = render(
      <GainReductionMeter reduction={-99} ariaLabel="GR" maxReduction={20} />,
    );
    const meter = getByRole("meter");
    fireEvent.focus(meter);
    expect(meter.getAttribute("aria-valuenow")).toBe("-20");
  });

  it("blur clears the snapshot", () => {
    const { getByRole } = render(
      <GainReductionMeter reduction={-3.4} ariaLabel="GR" />,
    );
    const meter = getByRole("meter");
    fireEvent.focus(meter);
    fireEvent.blur(meter);
    expect(meter.getAttribute("aria-valuenow")).toBeNull();
  });

  it("has focus-visible ring class", () => {
    const { getByRole } = render(<GainReductionMeter reduction={0} ariaLabel="GR" />);
    expect(getByRole("meter").className).toMatch(/focus-visible:ring/);
  });
});
