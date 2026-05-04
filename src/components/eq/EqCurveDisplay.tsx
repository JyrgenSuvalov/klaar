import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { combinedMagnitudeResponse, logSpacedFreqs } from "@/lib/eqMath";
import type { EqBand } from "@/store/dspStore";
import { a11y } from "@/i18n/a11yStrings";

// ── Constants ──────────────────────────────────────────────────────────────

const FREQ_MIN = 20;
const FREQ_MAX = 20000;
const DB_MAX = 24;
const DB_MIN = -24;
const CURVE_POINTS = 512;
const SAMPLE_RATE = 48000;

// One colour per band (8 bands)
const BAND_COLORS = [
  "#60a5fa", // blue-400
  "#34d399", // emerald-400
  "#fbbf24", // amber-400
  "#f87171", // red-400
  "#a78bfa", // violet-400
  "#fb923c", // orange-400
  "#38bdf8", // sky-400
  "#4ade80", // green-400
];

// ── CSS-var → canvas colour bridge ────────────────────────────────────────
//
// Canvas 2D can't parse `var(--…)` strings — `ctx.fillStyle = "var(--x)"`
// silently becomes black. To honour theme variables we have to resolve
// them to concrete colours in JS via `getComputedStyle`, then hand the
// resulting hex/rgb to the canvas. Done once per draw-loop setup (cheap)
// rather than per frame.

// Lazily-allocated 1×1 scratch canvas used to convert any CSS colour
// string — including modern functions like `oklch(...)`, `lab(...)`,
// or CSS Color 4 syntaxes that string-parsing regexes don't cover —
// to a concrete sRGB byte tuple. Previous versions of this file
// regex-parsed the serialised `fillStyle` getter, which broke when
// WebKit started preserving `oklch()` through the round-trip (or
// emitting space-separated `rgb(58 206 162)` CSS Color 4 form). The
// scratch canvas defers all colour-space conversion to the engine
// itself and reads the post-rasterisation pixel.
let _colorScratch: { canvas: HTMLCanvasElement; ctx: CanvasRenderingContext2D } | null = null;

function getColorScratch(): { canvas: HTMLCanvasElement; ctx: CanvasRenderingContext2D } | null {
  if (_colorScratch) return _colorScratch;
  if (typeof document === "undefined") return null;
  const canvas = document.createElement("canvas");
  canvas.width = 1;
  canvas.height = 1;
  // `willReadFrequently` keeps WebKit's pixel buffer on the CPU side so
  // repeated `getImageData` calls don't trigger GPU↔CPU sync. We only
  // hit this scratch a handful of times per draw-loop setup, but the
  // hint is free.
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) return null;
  _colorScratch = { canvas, ctx };
  return _colorScratch;
}

/** Convert any valid CSS colour string to an `{r, g, b}` byte tuple by
 *  painting it on a 1×1 canvas and reading the resulting pixel. Returns
 *  `null` if the colour is invalid (the canvas leaves the pixel as the
 *  cleared transparent black, distinguishable via the alpha byte). */
function parseColorToRgb(input: string): { r: number; g: number; b: number } | null {
  const scratch = getColorScratch();
  if (!scratch) return null;
  const { ctx } = scratch;
  ctx.clearRect(0, 0, 1, 1);
  // `fillStyle = invalid` is a no-op per the Canvas 2D spec — reset to
  // a known sentinel first so an invalid input can't quietly inherit
  // whatever was last assigned.
  ctx.fillStyle = "rgba(0, 0, 0, 0)";
  ctx.fillStyle = input;
  ctx.fillRect(0, 0, 1, 1);
  const data = ctx.getImageData(0, 0, 1, 1).data;
  // Alpha = 0 means the fillStyle assignment failed (still transparent
  // from the clear). Treat as parse failure so callers can fall back.
  if (data[3] === 0) return null;
  return { r: data[0], g: data[1], b: data[2] };
}

/** Resolve a CSS custom property to its computed value (already follows
 *  `var()` chains) on a given element. Returns the trimmed string, or
 *  the supplied fallback if the property is empty/unset. */
function resolveCssVar(el: Element, name: string, fallback: string): string {
  const v = getComputedStyle(el).getPropertyValue(name).trim();
  return v.length > 0 ? v : fallback;
}

/** Normalise any CSS colour string (`#abc`, `rgb(...)`, `hsl(...)`,
 *  `oklch(...)`, named colours, …) to the canvas-serialised form —
 *  `#rrggbb` for opaque colours, `rgba(r, g, b, a)` for translucent.
 *  Uses the browser's own colour parser by round-tripping through
 *  `fillStyle`, which is required by the Canvas 2D spec to return a
 *  serialised colour. Mutates and restores `fillStyle` so it's safe to
 *  call mid-draw. */
function normaliseCanvasColor(ctx: CanvasRenderingContext2D, input: string): string {
  const prev = ctx.fillStyle;
  // Defensive double-set: if `input` is invalid the assignment is a no-op,
  // and we'd otherwise leak whatever `prev` was as the "parsed" colour.
  ctx.fillStyle = "#000";
  ctx.fillStyle = input;
  const out = String(ctx.fillStyle);
  ctx.fillStyle = prev;
  return out;
}

// Spectrum analyzer constants
const SPECTRUM_BANDS = 256;
const SPECTRUM_DB_FLOOR = -80;
const SPECTRUM_DB_CEIL = 0;
// Throttle spectrum polling to ~30 fps (every ~33 ms)
const SPECTRUM_POLL_INTERVAL_MS = 33;

// Note: an earlier revision of this file paired the canvas with an
// `aria-live="polite"` text node that summarised the spectrum content
// for screen-reader users ("Silence" / "Tonal signal" / "Broadband
// activity"). It was removed during a11y manual QA: `aria-live` is
// global, not focus-scoped, so VoiceOver announced the summary
// continuously regardless of which control had focus, polluting the
// experience even when the user was working in a different panel.
// Ambient spectrum state is the wrong fit for a live region — SR users
// get signal-presence awareness from the input/output meters
// (`role="meter"`), interruption-worthy events from the existing clip
// and engine-error live regions, and full EQ editing from the per-band
// controls. The canvas's `aria-label` still references the overlay so
// users know it exists.

const HANDLE_RADIUS = 7;

interface Props {
  bands: EqBand[];
  width?: number;
  height?: number;
  onBandChange?: (index: number, partial: Partial<EqBand>) => void;
}

// ── Coordinate helpers ─────────────────────────────────────────────────────

function freqToX(freq: number, w: number): number {
  return (
    ((Math.log10(freq) - Math.log10(FREQ_MIN)) /
      (Math.log10(FREQ_MAX) - Math.log10(FREQ_MIN))) *
    w
  );
}

function dbToY(db: number, h: number): number {
  return ((DB_MAX - db) / (DB_MAX - DB_MIN)) * h;
}

function clamp(v: number, lo: number, hi: number) {
  return Math.max(lo, Math.min(hi, v));
}

/** Map spectrum dB value to canvas Y. DB_FLOOR → bottom, 0 dB → top. */
function spectrumDbToY(db: number, h: number): number {
  const clamped = clamp(db, SPECTRUM_DB_FLOOR, SPECTRUM_DB_CEIL);
  // 0 dB at top, DB_FLOOR at bottom
  return ((SPECTRUM_DB_CEIL - clamped) / (SPECTRUM_DB_CEIL - SPECTRUM_DB_FLOOR)) * h;
}

/**
 * Log-spaced frequencies for 256 spectrum bands (20 Hz – 20 kHz).
 * Pre-computed once — same log mapping as the EQ axis.
 */
const SPECTRUM_FREQS: number[] = (() => {
  const freqs: number[] = new Array<number>(SPECTRUM_BANDS);
  const logMin = Math.log10(FREQ_MIN);
  const logMax = Math.log10(FREQ_MAX);
  for (let i = 0; i < SPECTRUM_BANDS; i++) {
    freqs[i] = Math.pow(10, logMin + (i / (SPECTRUM_BANDS - 1)) * (logMax - logMin));
  }
  return freqs;
})();

/**
 * Cached x-coordinates for spectrum bands. Only depends on canvas width,
 * so we cache and recompute only on resize. (QA S1)
 */
let _spectrumXsCache: { w: number; xs: Float64Array } | null = null;

function getSpectrumXs(w: number): Float64Array {
  if (_spectrumXsCache && _spectrumXsCache.w === w) return _spectrumXsCache.xs;
  const xs = new Float64Array(SPECTRUM_BANDS);
  for (let i = 0; i < SPECTRUM_BANDS; i++) {
    xs[i] = freqToX(SPECTRUM_FREQS[i], w);
  }
  _spectrumXsCache = { w, xs };
  return xs;
}

/** Emit the Bézier top-edge curves onto a path (fill or stroke).
 *  Caller must have already positioned the path at (xs[0], ys[0]). (QA S2) */
function traceSpectrumBezier(
  path: Path2D,
  xs: Float64Array,
  ys: Float64Array,
  count: number,
) {
  for (let i = 1; i < count; i++) {
    const cpX = (xs[i - 1] + xs[i]) / 2;
    path.quadraticCurveTo(cpX, ys[i - 1], xs[i], ys[i]);
  }
}

/** Draw spectrum as a semi-transparent filled area behind the EQ curve.
 *  Uses quadratic Bézier interpolation for smooth curves between bands.
 *  `curveRgb` is the resolved `--color-eq-curve` so the spectrum fill
 *  tints with the theme. */
function drawSpectrum(
  ctx: CanvasRenderingContext2D,
  spectrum: number[],
  w: number,
  h: number,
  curveRgb: { r: number; g: number; b: number },
) {
  // Defensive: bail if spectrum data doesn't match expected length (QA S5)
  const count = Math.min(spectrum.length, SPECTRUM_BANDS);
  if (count < 2) return;

  ctx.save();

  const xs = getSpectrumXs(w);
  const ys = new Float64Array(count);
  for (let i = 0; i < count; i++) {
    ys[i] = spectrumDbToY(spectrum[i], h);
  }

  // ── Fill path (closed area under the Bézier curve) ────────────────────
  const fillPath = new Path2D();
  fillPath.moveTo(xs[0], h); // bottom-left
  fillPath.lineTo(xs[0], ys[0]); // up to first point
  traceSpectrumBezier(fillPath, xs, ys, count);
  fillPath.lineTo(xs[count - 1], h); // down to bottom-right
  fillPath.closePath();

  const { r, g, b } = curveRgb;
  const grad = ctx.createLinearGradient(0, 0, 0, h);
  grad.addColorStop(0, `rgba(${r},${g},${b},0.12)`);
  grad.addColorStop(1, `rgba(${r},${g},${b},0.02)`);
  ctx.fillStyle = grad;
  ctx.fill(fillPath);

  // ── Stroke the top edge using the same Bézier shape ───────────────────
  const strokePath = new Path2D();
  strokePath.moveTo(xs[0], ys[0]);
  traceSpectrumBezier(strokePath, xs, ys, count);

  ctx.strokeStyle = `rgba(${r},${g},${b},0.20)`;
  ctx.lineWidth = 1;
  ctx.stroke(strokePath);

  ctx.restore();
}

// ── Component ──────────────────────────────────────────────────────────────

export function EqCurveDisplay({
  bands,
  width = 600,
  height = 180,
  onBandChange,
}: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const bandsRef = useRef(bands);
  const rafRef = useRef<number>(0);
  // Memoised frequency response — recomputed only when bands change, not every frame
  const magnitudesRef = useRef<number[]>([]);
  // Spectrum data — polled at ~30 fps, stays in a ref (no React re-renders)
  const spectrumRef = useRef<number[]>(new Array<number>(SPECTRUM_BANDS).fill(SPECTRUM_DB_FLOOR));
  const lastSpectrumPollRef = useRef<number>(0);

  // Drag state
  const [draggingBand, setDraggingBand] = useState<number | null>(null);
  const dragStartRef = useRef<{
    mouseX: number;
    mouseY: number;
    startFreq: number;
    startGain: number;
  } | null>(null);

  // Keep bandsRef fresh for animation loop
  useEffect(() => {
    bandsRef.current = bands;
  }, [bands]);

  // Recompute frequency response only when bands change (not every RAF frame)
  useEffect(() => {
    const freqs = logSpacedFreqs(FREQ_MIN, FREQ_MAX, CURVE_POINTS);
    magnitudesRef.current = combinedMagnitudeResponse(bands, freqs, SAMPLE_RATE);
  }, [bands]);

  // ── Drawing ──────────────────────────────────────────────────────────────

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    canvas.width = width * dpr;
    canvas.height = height * dpr;
    ctx.scale(dpr, dpr);

    const w = width;
    const h = height;

    // Frequency axis — computed once per canvas resize, not per frame
    const freqs = logSpacedFreqs(FREQ_MIN, FREQ_MAX, CURVE_POINTS);

    // Resolve theme-driven canvas colours once per draw-loop setup. Two
    // steps are needed:
    //   1. `getComputedStyle` to follow `var(...)` chains — Canvas 2D
    //      itself can't parse `var(...)` strings.
    //   2. `normaliseCanvasColor` to fold modern colour functions
    //      (`oklch`, `hsl`, `lab`, named colours) down to the canvas's
    //      serialised form (`#rrggbb` / `rgba(...)`) so we can extract
    //      an RGB tuple for the alpha-modulated gradient and glow.
    const eqBg = normaliseCanvasColor(
      ctx,
      resolveCssVar(canvas, "--color-eq-bg", "#0c0c0c"),
    );
    const eqCurveSolid = normaliseCanvasColor(
      ctx,
      resolveCssVar(canvas, "--color-eq-curve", "#60a5fa"),
    );
    const curveRgb = parseColorToRgb(eqCurveSolid) ?? { r: 96, g: 165, b: 250 };

    const draw = (timestamp?: number) => {
      ctx.clearRect(0, 0, w, h);

      // Background — driven by `--color-eq-bg`, resolved above.
      ctx.fillStyle = eqBg;
      ctx.fillRect(0, 0, w, h);

      // ── Spectrum poll (fire-and-forget, ~30 fps) ────────────────────────
      const now = timestamp ?? performance.now();
      if (now - lastSpectrumPollRef.current >= SPECTRUM_POLL_INTERVAL_MS) {
        lastSpectrumPollRef.current = now;
        invoke<{ bins: number[] }>("get_spectrum")
          .then((data) => {
            if (data?.bins?.length === SPECTRUM_BANDS) {
              spectrumRef.current = data.bins;
            }
          })
          .catch(() => {
            // Silently ignore — spectrum is non-critical
          });
      }

      // ── Spectrum fill (behind grid, behind EQ curve) ────────────────────
      drawSpectrum(ctx, spectrumRef.current, w, h, curveRgb);

      // ── Grid lines ──────────────────────────────────────────────────────

      ctx.strokeStyle = "rgba(255,255,255,0.06)";
      ctx.lineWidth = 1;

      // Vertical grid: decade marks + key frequencies
      for (const f of [50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000]) {
        const x = freqToX(f, w);
        ctx.beginPath();
        ctx.moveTo(x, 0);
        ctx.lineTo(x, h);
        ctx.stroke();
      }

      // Horizontal grid: dB values
      for (const db of [-24, -18, -12, -6, 0, 6, 12, 18, 24]) {
        const y = dbToY(db, h);
        ctx.strokeStyle = db === 0 ? "rgba(255,255,255,0.15)" : "rgba(255,255,255,0.06)";
        ctx.beginPath();
        ctx.moveTo(0, y);
        ctx.lineTo(w, y);
        ctx.stroke();
      }

      // ── Axis labels (frequency) ──────────────────────────────────────────
      ctx.fillStyle = "rgba(255,255,255,0.25)";
      ctx.font = "9px 'SF Mono', monospace";
      ctx.textAlign = "center";
      for (const [f, label] of [
        [100, "100"],
        [1000, "1k"],
        [10000, "10k"],
      ] as [number, string][]) {
        ctx.fillText(label, freqToX(f, w), h - 3);
      }

      // ── Axis labels (dB) ─────────────────────────────────────────────────
      // Reference EQ8: indicators at -12, -6, 0, +6, +12 dB.
      ctx.textAlign = "left";
      ctx.textBaseline = "middle";
      for (const [db, label] of [
        [12, "+12"],
        [6, "+6"],
        [0, "0"],
        [-6, "-6"],
        [-12, "-12"],
      ] as [number, string][]) {
        ctx.fillStyle = db === 0 ? "rgba(255,255,255,0.4)" : "rgba(255,255,255,0.25)";
        ctx.fillText(label, 3, dbToY(db, h));
      }
      ctx.textBaseline = "alphabetic";

      // ── Curve ───────────────────────────────────────────────────────────

      const currentBands = bandsRef.current;
      // Use pre-computed magnitudes (recomputed by useEffect when bands change, not every frame)
      const magnitudes = magnitudesRef.current.length > 0
        ? magnitudesRef.current
        : combinedMagnitudeResponse(currentBands, freqs, SAMPLE_RATE);

      ctx.beginPath();
      ctx.strokeStyle = eqCurveSolid;
      ctx.lineWidth = 1.5;
      ctx.shadowColor = eqCurveSolid;
      ctx.shadowBlur = 4;

      for (let i = 0; i < freqs.length; i++) {
        const x = freqToX(freqs[i], w);
        const y = dbToY(clamp(magnitudes[i], DB_MIN, DB_MAX), h);
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      }
      ctx.stroke();
      ctx.shadowBlur = 0;

      // Filled area under the curve
      ctx.save();
      ctx.beginPath();
      ctx.moveTo(freqToX(freqs[0], w), dbToY(0, h));
      for (let i = 0; i < freqs.length; i++) {
        const x = freqToX(freqs[i], w);
        const y = dbToY(clamp(magnitudes[i], DB_MIN, DB_MAX), h);
        ctx.lineTo(x, y);
      }
      ctx.lineTo(freqToX(freqs[freqs.length - 1], w), dbToY(0, h));
      ctx.closePath();
      // Alpha tuned for the high-chroma teal accent — the original
      // 0.15/0.02 values were sized for a blue-400 curve and read as
      // cyan/blue on black at low alpha (Bezold–Brücke shift). Bumping
      // the top stop lets the actual hue come through; bottom stop
      // stays nearly transparent so deep cuts don't fight the grid.
      const grad = ctx.createLinearGradient(0, 0, 0, h);
      grad.addColorStop(0, `rgba(${curveRgb.r},${curveRgb.g},${curveRgb.b},0.20)`);
      grad.addColorStop(1, `rgba(${curveRgb.r},${curveRgb.g},${curveRgb.b},0.05)`);
      ctx.fillStyle = grad;
      ctx.fill();
      ctx.restore();

      // ── Band handle markers ──────────────────────────────────────────────

      for (let bi = 0; bi < currentBands.length; bi++) {
        const band = currentBands[bi];
        const color = BAND_COLORS[bi];
        const isPass = band.filterType === "highPass" || band.filterType === "lowPass" || band.filterType === "highPass48" || band.filterType === "lowPass48";
        const displayGain = isPass ? 0 : band.gain;
        const x = freqToX(band.frequency, w);
        const y = dbToY(clamp(displayGain, DB_MIN, DB_MAX), h);
        const isDragging = draggingBand === bi;

        if (!band.enabled) {
          // Dimmed ring only
          ctx.beginPath();
          ctx.arc(x, y, HANDLE_RADIUS, 0, Math.PI * 2);
          ctx.strokeStyle = `${color}44`;
          ctx.lineWidth = 1;
          ctx.stroke();
          continue;
        }

        // Filled circle
        ctx.beginPath();
        ctx.arc(x, y, HANDLE_RADIUS + (isDragging ? 2 : 0), 0, Math.PI * 2);
        ctx.fillStyle = isDragging ? color : `${color}cc`;
        ctx.fill();
        ctx.strokeStyle = color;
        ctx.lineWidth = 1.5;
        ctx.stroke();

        // Band number label
        ctx.fillStyle = "#000";
        ctx.font = `bold 8px sans-serif`;
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.fillText(String(bi + 1), x, y);
        ctx.textBaseline = "alphabetic";
      }

      // Only schedule next frame if the window is still visible
      if (!document.hidden) {
        rafRef.current = requestAnimationFrame(draw);
      } else {
        rafRef.current = 0;
      }
    };

    // Start the draw loop only if the window is visible
    if (!document.hidden) {
      rafRef.current = requestAnimationFrame(draw);
    }

    // ── Visibility lifecycle: resume RAF when window becomes visible ──
    const onVisibilityChange = () => {
      if (!document.hidden && rafRef.current === 0) {
        rafRef.current = requestAnimationFrame(draw);
      }
    };
    document.addEventListener("visibilitychange", onVisibilityChange);

    return () => {
      cancelAnimationFrame(rafRef.current);
      rafRef.current = 0;
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [width, height, draggingBand]);

  // ── Mouse interaction ────────────────────────────────────────────────────

  const getCanvasPos = useCallback(
    (e: React.MouseEvent): { x: number; y: number } => {
      // canvasRef.current is always set when a mouse event fires on the canvas
      const rect = canvasRef.current?.getBoundingClientRect() ?? { left: 0, top: 0 };
      return {
        x: e.clientX - rect.left,
        y: e.clientY - rect.top,
      };
    },
    [],
  );

  const hitTestHandle = useCallback(
    (x: number, y: number): number | null => {
      for (let i = 0; i < bands.length; i++) {
        const band = bands[i];
        const isPass = band.filterType === "highPass" || band.filterType === "lowPass" || band.filterType === "highPass48" || band.filterType === "lowPass48";
        const displayGain = isPass ? 0 : band.gain;
        const hx = freqToX(band.frequency, width);
        const hy = dbToY(clamp(displayGain, DB_MIN, DB_MAX), height);
        const dist = Math.sqrt((x - hx) ** 2 + (y - hy) ** 2);
        if (dist <= HANDLE_RADIUS + 4) return i;
      }
      return null;
    },
    [bands, width, height],
  );

  const onMouseDown = useCallback(
    (e: React.MouseEvent<HTMLCanvasElement>) => {
      if (!onBandChange) return;
      const { x, y } = getCanvasPos(e);
      const idx = hitTestHandle(x, y);
      if (idx === null) return;
      e.preventDefault();
      setDraggingBand(idx);
      const band = bands[idx];
      dragStartRef.current = {
        mouseX: e.clientX,
        mouseY: e.clientY,
        startFreq: band.frequency,
        startGain: band.gain,
      };
    },
    [bands, getCanvasPos, hitTestHandle, onBandChange],
  );

  useEffect(() => {
    if (draggingBand === null) return;

    const onMove = (e: MouseEvent) => {
      if (!dragStartRef.current || !onBandChange) return;
      const { mouseX, mouseY, startFreq, startGain } = dragStartRef.current;

      const dx = e.clientX - mouseX;
      const dy = e.clientY - mouseY;

      // Horizontal: log-scale frequency
      const logMin = Math.log10(FREQ_MIN);
      const logMax = Math.log10(FREQ_MAX);
      const logFreq =
        Math.log10(startFreq) + (dx / width) * (logMax - logMin);
      const newFreq = clamp(Math.pow(10, logFreq), FREQ_MIN, FREQ_MAX);

      const band = bandsRef.current[draggingBand];
      const isPass = band.filterType === "highPass" || band.filterType === "lowPass" || band.filterType === "highPass48" || band.filterType === "lowPass48";

      if (isPass) {
        // Pass filters: only adjust frequency
        onBandChange(draggingBand, { frequency: Math.round(newFreq) });
      } else {
        // Vertical: gain, inverted (drag up = positive gain)
        const gainDelta = (-dy / height) * (DB_MAX - DB_MIN);
        const newGain = clamp(startGain + gainDelta, DB_MIN, DB_MAX);
        onBandChange(draggingBand, {
          frequency: Math.round(newFreq),
          gain: parseFloat(newGain.toFixed(1)),
        });
      }
    };

    const onUp = () => {
      setDraggingBand(null);
      dragStartRef.current = null;
    };

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, [draggingBand, width, height, onBandChange]);

  const cursor =
    draggingBand !== null
      ? "grabbing"
      : onBandChange
        ? "crosshair"
        : "default";

  return (
    <canvas
      ref={canvasRef}
      style={{ width, height, cursor }}
      className="block rounded"
      onMouseDown={onMouseDown}
      role="img"
      aria-label={a11y.eqCanvas()}
      tabIndex={-1}
    />
  );
}

// Export colours for use in EqBandControls
export { BAND_COLORS };
