import { useCallback, useEffect, useRef, useState } from "react";
import { applyKeyboardDelta, valueToNorm } from "./Knob.helpers";

interface KnobProps {
  label: string;
  value: number;
  min: number;
  max: number;
  defaultValue: number;
  unit?: string;
  step?: number;
  onChange: (value: number) => void;
  /** Size in pixels (default 48) */
  size?: number;
  /** Disable the knob */
  disabled?: boolean;
  /** Value mapping: "linear" (default) or "log" for frequency-style controls */
  scale?: "linear" | "log";
  /** Fixed number of decimal places for display and editing. When unset, auto-selects 1 decimal for |value| < 10, else 0. */
  decimals?: number;
  /**
   * Optional ARIA label for screen readers. When omitted, falls back to `label`.
   * Use this to disambiguate when the visible `label` lacks panel context
   * (e.g. pass "Compressor Threshold" while `label` stays "Threshold").
   */
  ariaLabel?: string;
  /**
   * Optional explicit step size for keyboard adjustment, overriding the default
   * "1% of range" rule and the `step` prop. For `scale="linear"` this is an
   * additive step (in value units). For `scale="log"` it is interpreted as a
   * multiplicative factor: value <- value * keyboardStep^multiplier.
   * Use sparingly — only when the default produces awkward UX at a call site.
   */
  keyboardStep?: number;
}

// The arc spans 270° starting from 135° (bottom-left) to 405° (bottom-right)
const ARC_START_DEG = 135;
const ARC_END_DEG = 405;
const ARC_RANGE_DEG = ARC_END_DEG - ARC_START_DEG; // 270°

// Pixels of vertical drag to traverse the full value range (normal mode)
const DRAG_SENSITIVITY = 200;
// Multiplier when Shift is held
const FINE_MULTIPLIER = 0.1;

function degToRad(deg: number) {
  return (deg * Math.PI) / 180;
}

function valueToAngle(value: number, min: number, max: number, scale: "linear" | "log"): number {
  const norm = clamp(valueToNorm(value, min, max, scale), 0, 1);
  return ARC_START_DEG + norm * ARC_RANGE_DEG;
}

function clamp(v: number, lo: number, hi: number) {
  return Math.max(lo, Math.min(hi, v));
}

function formatValue(value: number, unit?: string, decimals?: number): string {
  const str =
    decimals !== undefined
      ? value.toFixed(decimals)
      : Math.abs(value) < 10
        ? value.toFixed(1)
        : value.toFixed(0);
  return unit ? `${str} ${unit}` : str;
}

/** Map a keyboard event to a step multiplier, or null for non-handled keys. */
function keyToMultiplier(e: React.KeyboardEvent): number | null {
  const shift = e.shiftKey;
  switch (e.key) {
    case "ArrowUp":
    case "ArrowRight":
      return shift ? +0.1 : +1;
    case "ArrowDown":
    case "ArrowLeft":
      return shift ? -0.1 : -1;
    case "PageUp":
      return +10;
    case "PageDown":
      return -10;
    case "Home":
      return Number.NEGATIVE_INFINITY;
    case "End":
      return Number.POSITIVE_INFINITY;
    default:
      return null;
  }
}

export function Knob({
  label,
  value,
  min,
  max,
  defaultValue,
  unit,
  step = 0,
  onChange,
  size = 48,
  disabled = false,
  scale = "linear",
  decimals,
  ariaLabel,
  keyboardStep,
}: KnobProps) {
  const svgRef = useRef<SVGSVGElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const [editing, setEditing] = useState(false);
  const [editText, setEditText] = useState("");
  const dragState = useRef<{
    startY: number;
    startValue: number;
    active: boolean;
  } | null>(null);

  const cx = size / 2;
  const cy = size / 2;
  const radius = (size - 8) / 2; // 4px inset on each side
  const strokeWidth = 3;

  // Track arc path for a given end angle
  const arcPath = (endAngleDeg: number) => {
    const startRad = degToRad(ARC_START_DEG);
    const endRad = degToRad(endAngleDeg);
    const x1 = cx + radius * Math.cos(startRad);
    const y1 = cy + radius * Math.sin(startRad);
    const x2 = cx + radius * Math.cos(endRad);
    const y2 = cy + radius * Math.sin(endRad);
    const largeArc = endAngleDeg - ARC_START_DEG > 180 ? 1 : 0;
    return `M ${x1} ${y1} A ${radius} ${radius} 0 ${largeArc} 1 ${x2} ${y2}`;
  };

  // Full track arc
  const trackPath = arcPath(ARC_END_DEG);
  // Filled arc up to current value
  const currentAngle = valueToAngle(value, min, max, scale);
  const fillPath = arcPath(currentAngle);

  // Pointer dot at current angle
  const dotRad = degToRad(currentAngle);
  const dotX = cx + radius * Math.cos(dotRad);
  const dotY = cy + radius * Math.sin(dotRad);

  const applyDelta = useCallback(
    (dy: number, shift: boolean) => {
      const sensitivity = shift ? DRAG_SENSITIVITY / FINE_MULTIPLIER : DRAG_SENSITIVITY;
      const startValue = dragState.current?.startValue ?? value;
      let next: number;

      if (scale === "log") {
        // Work in normalised log-space: full drag range = full [0, 1] span
        const safeMin = Math.max(min, 1e-6);
        const logNorm = Math.log(startValue / safeMin) / Math.log(max / safeMin);
        const newLogNorm = clamp(logNorm + (-dy / sensitivity), 0, 1);
        next = safeMin * Math.pow(max / safeMin, newLogNorm);
      } else {
        const rangeSpan = max - min;
        let delta = (-dy / sensitivity) * rangeSpan;
        if (step > 0) delta = Math.round(delta / step) * step;
        next = startValue + delta;
      }

      const snapped = step > 0 ? Math.round((next - min) / step) * step + min : next;
      onChange(clamp(snapped, min, max));
    },
    [max, min, onChange, scale, step, value],
  );

  const onMouseDown = useCallback(
    (e: React.MouseEvent<SVGSVGElement>) => {
      if (disabled) return;
      e.preventDefault();
      dragState.current = { startY: e.clientY, startValue: value, active: true };
    },
    [disabled, value],
  );

  const onDoubleClick = useCallback(
    (e: React.MouseEvent) => {
      if (disabled) return;
      e.preventDefault();
      onChange(defaultValue);
    },
    [disabled, defaultValue, onChange],
  );

  const onKeyDown = useCallback(
    (e: React.KeyboardEvent<SVGSVGElement>) => {
      if (disabled) return;
      const multiplier = keyToMultiplier(e);
      if (multiplier === null) return;
      // Suppress page scroll for Arrow / Page keys; benign for Home / End.
      e.preventDefault();
      const next = applyKeyboardDelta(
        value,
        multiplier,
        min,
        max,
        scale,
        step,
        keyboardStep,
        decimals,
      );
      if (next !== value) onChange(next);
    },
    [disabled, value, min, max, scale, step, keyboardStep, decimals, onChange],
  );

  const startEditing = useCallback(() => {
    if (disabled) return;
    // Show the raw numeric value (no unit) for editing, respecting decimals prop
    const str =
      decimals !== undefined
        ? value.toFixed(decimals)
        : Math.abs(value) < 10
          ? value.toFixed(1)
          : value.toFixed(0);
    setEditText(str);
    setEditing(true);
  }, [disabled, value, decimals]);

  const commitEdit = useCallback(() => {
    setEditing(false);
    const parsed = parseFloat(editText);
    if (Number.isNaN(parsed)) return; // invalid input — discard
    const snapped = step > 0 ? Math.round((parsed - min) / step) * step + min : parsed;
    onChange(clamp(snapped, min, max));
  }, [editText, min, max, step, onChange]);

  const cancelEdit = useCallback(() => {
    setEditing(false);
  }, []);

  // Auto-focus and select the input when entering edit mode
  useEffect(() => {
    if (editing && inputRef.current) {
      inputRef.current.focus();
      inputRef.current.select();
    }
  }, [editing]);

  // Global mouse move / up handlers attached once drag starts
  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (!dragState.current?.active) return;
      const dy = e.clientY - dragState.current.startY;
      applyDelta(dy, e.shiftKey);
    };

    const onUp = () => {
      if (dragState.current) dragState.current.active = false;
    };

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, [applyDelta]);

  return (
    <div
      className="flex flex-col items-center gap-1 select-none"
    >
      <svg
        ref={svgRef}
        width={size}
        height={size}
        onMouseDown={onMouseDown}
        onDoubleClick={onDoubleClick}
        onKeyDown={onKeyDown}
        tabIndex={disabled ? -1 : 0}
        role="slider"
        aria-label={ariaLabel ?? label}
        aria-valuemin={min}
        aria-valuemax={max}
        aria-valuenow={value}
        aria-valuetext={formatValue(value, unit, decimals)}
        aria-disabled={disabled || undefined}
        style={{ cursor: disabled ? "default" : "ns-resize" }}
        className="knob-svg rounded-sm focus:outline-none focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--color-accent)]"
      >
        {/* Track (full arc, background) */}
        <path
          d={trackPath}
          fill="none"
          stroke="var(--color-knob-track)"
          strokeWidth={strokeWidth}
          strokeLinecap="round"
        />
        {/* Fill (value arc) */}
        <path
          d={fillPath}
          fill="none"
          stroke="var(--color-knob-fill)"
          strokeWidth={strokeWidth}
          strokeLinecap="round"
        />
        {/* Pointer dot */}
        <circle
          cx={dotX}
          cy={dotY}
          r={strokeWidth - 0.5}
          fill="var(--color-knob-fill)"
        />
      </svg>

      {/* Value display — click to edit. Fixed-height wrapper prevents layout shift. */}
      <div className="flex items-center justify-center h-[14px]">
        {editing ? (
          <input
            ref={inputRef}
            type="text"
            inputMode="decimal"
            value={editText}
            onChange={(e) => setEditText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") commitEdit();
              if (e.key === "Escape") cancelEdit();
            }}
            onBlur={commitEdit}
            className="text-[10px] leading-none tabular-nums text-center bg-white/10 rounded-sm outline-none border-0 p-0 m-0"
            style={{
              color: "var(--color-text-primary)",
              width: `${Math.max(3, editText.length + 1)}ch`,
              boxShadow: "0 0 0 1px var(--color-knob-fill)",
            }}
          />
        ) : (
          <span
            className="text-[10px] leading-none tabular-nums cursor-text rounded-sm px-0.5 hover:bg-white/10 transition-colors"
            style={{ color: "var(--color-text-secondary)" }}
            onClick={startEditing}
          >
            {formatValue(value, unit, decimals)}
          </span>
        )}
      </div>

      {/* Label */}
      <span
        className="text-[10px] leading-none font-medium uppercase tracking-wider"
        style={{ color: "var(--color-text-secondary)" }}
      >
        {label}
      </span>
    </div>
  );
}
