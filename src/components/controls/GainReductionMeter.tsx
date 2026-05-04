import { useEffect, useRef, useState } from "react";

interface GainReductionMeterProps {
  /** Gain reduction in dB (0 = no reduction, negative = reduction applied) */
  reduction: number;
  /** Peak gain reduction in dB, sourced from engineStore.peakMeters (default 0 = no peak marker) */
  peakReduction?: number;
  /** Maximum displayable reduction in dB magnitude (default 30) */
  maxReduction?: number;
  /** Width in pixels (default 16) */
  width?: number;
  /** Height in pixels (default 80) */
  height?: number;
  /**
   * Render a small numeric dB readout (e.g. "−3.2") below the bar, above
   * the "GR" label. Default `false` to preserve the Compressor / Limiter
   * panel visuals; the de-esser panel opts in to this readout.
   */
  showNumericReadout?: boolean;
  /** ARIA label for screen readers (e.g. "Compressor gain reduction"). */
  ariaLabel?: string;
}

export function GainReductionMeter({
  reduction,
  peakReduction = 0,
  maxReduction = 30,
  width = 16,
  height = 80,
  showNumericReadout = false,
  ariaLabel = "Gain reduction",
}: GainReductionMeterProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  // Initialise canvas dimensions once (or when size props change)
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = width * dpr;
    canvas.height = height * dpr;
    const ctx = canvas.getContext("2d");
    if (ctx) ctx.scale(dpr, dpr);
  }, [width, height]);

  // Redraw whenever reduction or peakReduction props change — no internal RAF needed;
  // engineStore drives updates at ~60 fps via its single polling loop.
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const w = width;
    const h = height;

    // magnitude: 0 = no reduction, positive = how many dB of reduction
    const magnitude = Math.max(0, -reduction);
    const clamped = Math.min(magnitude, maxReduction);

    ctx.clearRect(0, 0, w, h);
    // Hardcoded hex — Canvas 2D ignores CSS var() on WebKit/WKWebView
    ctx.fillStyle = "#0f0f0f";
    ctx.fillRect(0, 0, w, h);

    if (clamped > 0) {
      const barH = (clamped / maxReduction) * h;
      // Inverted bar — grows downward from top
      ctx.fillStyle = "#f59e0b";
      ctx.fillRect(1, 0, w - 2, barH);
    }

    // Peak hold marker — sourced from engineStore.peakMeters, held and decayed externally
    const peakMag = Math.max(0, -(peakReduction ?? 0));
    if (peakMag > 0.1) {
      const peakY = (Math.min(peakMag, maxReduction) / maxReduction) * h;
      ctx.fillStyle = "#fbbf24";
      ctx.fillRect(0, peakY - 1, w, 2);
    }
  }, [reduction, peakReduction, maxReduction, width, height]);

  // Format the displayed reduction value
  const magnitude = Math.max(0, -reduction);
  const displayText =
    magnitude < 0.1 ? "0" : `-${magnitude < 10 ? magnitude.toFixed(1) : magnitude.toFixed(0)}`;

  // ── Snapshot-on-focus ARIA values ───────────────────────────────────────
  // Same rationale as `Meter`: continuous aria-valuenow updates make VoiceOver
  // chatter on every meter change. Snapshot at focus, freeze for the
  // announcement, refocus for a fresh reading.
  const [snapshot, setSnapshot] = useState<number | null>(null);

  const captureSnapshot = () => {
    setSnapshot(Math.max(-maxReduction, Math.min(0, reduction)));
  };

  const ariaValueText =
    snapshot === null
      ? undefined
      : Math.abs(snapshot) < 0.1
        ? "0 dB"
        : `${snapshot.toFixed(1)} dB`;

  return (
    <div
      className="flex flex-col items-center gap-1 rounded-sm focus:outline-none focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--color-accent)]"
      role="meter"
      tabIndex={0}
      aria-label={ariaLabel}
      aria-valuemin={-maxReduction}
      aria-valuemax={0}
      aria-valuenow={snapshot ?? undefined}
      aria-valuetext={ariaValueText}
      onFocus={captureSnapshot}
      onBlur={() => setSnapshot(null)}
    >
      <canvas
        ref={canvasRef}
        style={{ width, height }}
        className="rounded-sm"
      />
      {showNumericReadout && (
        <span
          className="text-[9px] leading-none tabular-nums"
          style={{ color: "var(--color-text-secondary)" }}
        >
          {displayText}
        </span>
      )}
      <span
        className="text-[9px] leading-none uppercase tracking-wider"
        style={{ color: "var(--color-text-secondary)" }}
      >
        GR
      </span>
    </div>
  );
}
