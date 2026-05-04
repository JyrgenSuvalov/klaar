import { useEffect, useRef, useState } from "react";

interface MeterProps {
  /** Current smoothed level in dBFS (from engineStore.smoothedMeters) */
  level: number;
  /** Peak hold level in dBFS (from engineStore.peakMeters) */
  peakLevel?: number;
  /** Label shown below the meter */
  label?: string;
  /** dBFS floor (default -60) */
  floor?: number;
  /** Width in pixels (default 16) */
  width?: number;
  /** Height in pixels (default 120) */
  height?: number;
  /** Whether to show tick marks */
  ticks?: boolean;
  /**
   * Optional dBFS value at which to render a horizontal threshold-tick
   * marker (e.g. the de-esser sidechain's current threshold). Drawn as a
   * 2 px high accent-coloured line spanning the meter width. Default
   * `undefined` → no tick rendered (preserves existing call-site visuals).
   */
  tickDb?: number;
  /**
   * ARIA label for screen readers. Pass something contextual at the call
   * site, e.g. "Input level" or "Output level". Defaults to "Level".
   */
  ariaLabel?: string;
}

// Color zone thresholds (dBFS)
const ZONE_RED = -3;
const ZONE_YELLOW = -12;

export function Meter({
  level,
  peakLevel,
  label,
  floor = -60,
  width = 16,
  height = 120,
  ticks = true,
  tickDb,
  ariaLabel = "Level",
}: MeterProps) {
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

  // Redraw whenever level or peakLevel props change — no internal RAF needed;
  // engineStore drives updates at ~60 fps via its single polling loop.
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const w = width;
    const h = height;

    const dbToY = (db: number) => {
      const norm = (db - floor) / (0 - floor); // 0..1 quiet→loud
      return h - norm * h;
    };

    const clamped = Math.max(floor, Math.min(0, level));

    ctx.clearRect(0, 0, w, h);

    // Background
    // Hardcoded hex — Canvas 2D ignores CSS var() on WebKit/WKWebView
    ctx.fillStyle = "#0f0f0f";
    ctx.fillRect(0, 0, w, h);

    const levelY = dbToY(clamped);

    // Green zone (floor → -12 dBFS)
    const greenTop = dbToY(ZONE_YELLOW);
    if (levelY < h) {
      const fillTop = Math.max(levelY, greenTop);
      ctx.fillStyle = "#22c55e";
      ctx.fillRect(1, fillTop, w - 2, h - fillTop);
    }

    // Yellow zone (-12 → -3 dBFS)
    const yellowBottom = dbToY(ZONE_YELLOW);
    const yellowTop = dbToY(ZONE_RED);
    if (levelY < yellowBottom) {
      const fillTop = Math.max(levelY, yellowTop);
      ctx.fillStyle = "#eab308";
      ctx.fillRect(1, fillTop, w - 2, yellowBottom - fillTop);
    }

    // Red zone (-3 → 0 dBFS)
    const redBottom = dbToY(ZONE_RED);
    if (levelY < redBottom) {
      const fillTop = Math.max(levelY, 0);
      ctx.fillStyle = "#ef4444";
      ctx.fillRect(1, fillTop, w - 2, redBottom - fillTop);
    }

    // Peak hold line — drawn from the peakLevel prop (owned by engineStore)
    const effectivePeak = peakLevel ?? floor;
    const peakClamped = Math.max(floor, Math.min(0, effectivePeak));
    if (peakClamped > floor + 1) {
      const peakY = dbToY(peakClamped);
      ctx.fillStyle =
        peakClamped >= ZONE_RED
          ? "#ef4444"
          : peakClamped >= ZONE_YELLOW
            ? "#eab308"
            : "#22c55e";
      ctx.fillRect(0, peakY - 1, w, 2);
    }

    // Tick marks
    if (ticks) {
      ctx.fillStyle = "rgba(255,255,255,0.1)";
      for (const db of [-6, -12, -18, -24, -36, -48]) {
        if (db > floor) {
          const y = dbToY(db);
          ctx.fillRect(0, y, w, 1);
        }
      }
    }

    // Threshold tick marker — drawn last so it overlays the level fill.
    // Used by the de-esser sidechain meter to show the current threshold
    // setting; absent for all other call sites (default `tickDb`
    // undefined). Hex matches the bypass / accent palette.
    if (tickDb !== undefined && tickDb >= floor && tickDb <= 0) {
      const tickY = dbToY(tickDb);
      ctx.fillStyle = "#ffffff";
      ctx.fillRect(0, tickY - 1, w, 2);
    }
  }, [level, peakLevel, floor, width, height, ticks, tickDb]);

  // ── Snapshot-on-focus ARIA values ───────────────────────────────────────
  // Continuously updating aria-valuenow / aria-valuetext makes VoiceOver
  // re-announce on every change — multiple times per second during normal
  // speech. Instead, we capture the current level when the user focuses the
  // meter and freeze the announced text at that snapshot. To get a fresh
  // reading the user simply re-focuses (Shift+Tab, Tab back). This matches
  // the design intent: "poll on demand. Tab, hear value, adjust, repeat."
  // The visible canvas continues to update at full frame rate.
  const [snapshot, setSnapshot] = useState<number | null>(null);

  const captureSnapshot = () => {
    setSnapshot(Math.max(floor, Math.min(0, level)));
  };

  const announcedLevel = snapshot ?? floor;
  const ariaValueText =
    snapshot === null
      ? undefined
      : announcedLevel <= floor + 0.5
        ? `Below ${floor} dB`
        : `${Math.round(announcedLevel)} dB`;

  return (
    <div
      className="flex flex-col items-center gap-1 rounded-sm focus:outline-none focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--color-accent)]"
      role="meter"
      tabIndex={0}
      aria-label={ariaLabel}
      aria-valuemin={floor}
      aria-valuemax={0}
      aria-valuenow={snapshot === null ? undefined : announcedLevel}
      aria-valuetext={ariaValueText}
      onFocus={captureSnapshot}
      onBlur={() => setSnapshot(null)}
    >
      <canvas
        ref={canvasRef}
        style={{ width, height }}
        className="rounded-sm"
      />
      {label && (
        <span
          className="text-[10px] leading-none uppercase tracking-wider"
          style={{ color: "var(--color-text-secondary)" }}
        >
          {label}
        </span>
      )}
    </div>
  );
}
