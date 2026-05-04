import { render } from "@testing-library/react";
import { EqCurveDisplay } from "./EqCurveDisplay";
import type { EqBand } from "@/store/dspStore";

const FILLER_BANDS: EqBand[] = Array.from({ length: 8 }, (_, i) => ({
  enabled: false,
  filterType: "bell",
  frequency: 100 * (i + 1),
  gain: 0,
  q: 1,
}));

describe("EqCurveDisplay accessibility", () => {
  it("canvas is exposed as role=img with descriptive aria-label and is not in tab order", () => {
    const { container } = render(<EqCurveDisplay bands={FILLER_BANDS} />);
    const canvas = container.querySelector("canvas")!;
    expect(canvas.getAttribute("role")).toBe("img");
    expect(canvas.getAttribute("aria-label")).toBe(
      "EQ frequency response and live spectrum overlay. Use the band controls below to adjust frequency, gain, and Q via keyboard.",
    );
    expect(canvas.getAttribute("tabindex")).toBe("-1");
  });
});
