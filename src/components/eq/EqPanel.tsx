import { useLayoutEffect, useRef, useState } from "react";
import { useDspStore } from "@/store/dspStore";
import { BypassToggle } from "@/components/controls/BypassToggle";
import { EqCurveDisplay } from "@/components/eq/EqCurveDisplay";
import { EqBandControls } from "@/components/eq/EqBandControls";
import type { EqBand } from "@/store/dspStore";
import { a11y } from "@/i18n/a11yStrings";

// Initial fallback width used until the first ResizeObserver measurement
// lands. Matches the previous hardcoded width so layout doesn't visibly
// jump on first paint.
const DEFAULT_EQ_CURVE_WIDTH = 860;
const EQ_CURVE_HEIGHT = 160;

export function EqPanel() {
  const eq = useDspStore((s) => s.eq);
  const setBypass = useDspStore((s) => s.setBypass);
  const setEqBand = useDspStore((s) => s.setEqBand);

  const wrapperRef = useRef<HTMLDivElement>(null);
  const [curveWidth, setCurveWidth] = useState(DEFAULT_EQ_CURVE_WIDTH);

  // Measure the wrapper synchronously before paint (useLayoutEffect) for the
  // initial size, then keep it in sync via ResizeObserver as the window
  // resizes. EqCurveDisplay already reacts to width changes — it tears down
  // its cached frequency-axis x-coordinates and redraws on the next frame.
  useLayoutEffect(() => {
    const el = wrapperRef.current;
    if (!el) return;

    const apply = (w: number) => {
      // Round and clamp to a sane minimum to avoid passing 0 during transient
      // hides (e.g. parent collapses) which would cause canvas-coordinate
      // caches to recompute against a degenerate width.
      const next = Math.max(1, Math.round(w));
      setCurveWidth((prev) => (prev === next ? prev : next));
    };

    apply(el.clientWidth);

    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) {
        apply(entry.contentRect.width);
      }
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const handleBandChange = (index: number, partial: Partial<EqBand>) => {
    void setEqBand(index, partial);
  };

  return (
    <div
      className={`effect-panel eq-panel${eq.bypassed ? " bypassed" : ""}`}
      role="region"
      aria-label={a11y.panel.eq()}
    >
      {/* Panel header */}
      <div className="panel-header">
        <span className="panel-title">Parametric EQ</span>
        <BypassToggle
          bypassed={eq.bypassed}
          onToggle={(v) => setBypass("eq", v)}
          label="EQ"
        />
      </div>

      {/* EQ curve */}
      <div className="eq-curve-wrapper" ref={wrapperRef}>
        <EqCurveDisplay
          bands={eq.bands}
          width={curveWidth}
          height={EQ_CURVE_HEIGHT}
          onBandChange={eq.bypassed ? undefined : handleBandChange}
        />
      </div>

      {/* Per-band controls */}
      <div className="eq-bands-row">
        {eq.bands.map((band, i) => (
          <EqBandControls
            key={i}
            index={i}
            band={band}
            onChange={(partial) => handleBandChange(i, partial)}
            eqBypassed={eq.bypassed}
          />
        ))}
      </div>
    </div>
  );
}
