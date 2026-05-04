/**
 * Pure helpers for the `Knob` component.
 *
 * Extracted into a sibling module so `Knob.tsx` only exports its component,
 * which keeps Vite fast-refresh happy and gives tests a stable import surface
 * for the math without pulling in React / DOM machinery.
 */

/** Convert a value to normalised 0..1 range. For log scale, `min` must be > 0. */
export function valueToNorm(
  value: number,
  min: number,
  max: number,
  scale: "linear" | "log",
): number {
  if (scale === "log") {
    // Guard: log-scale requires positive min to avoid -Infinity / NaN
    const safeMin = Math.max(min, 1e-6);
    const safeVal = Math.max(value, safeMin);
    return Math.log(safeVal / safeMin) / Math.log(max / safeMin);
  }
  return (value - min) / (max - min);
}

/** Inverse of `valueToNorm`: convert a 0..1 normalised position back to value space. */
export function normToValue(
  norm: number,
  min: number,
  max: number,
  scale: "linear" | "log",
): number {
  if (scale === "log") {
    const safeMin = Math.max(min, 1e-6);
    return safeMin * Math.pow(max / safeMin, norm);
  }
  return min + norm * (max - min);
}

/**
 * Compute the per-step amount used by keyboard adjustments.
 *
 * Linear scale: returns an additive step. Resolution order is
 *   `keyboardStep` ?? (`step` > 0 ? `step` : 1% of range).
 * Log scale: returns a multiplicative factor `f` such that one step is
 *   `value * f^multiplier`. Resolution order is
 *   `keyboardStep` ?? (`step` > 0 ? `step` : `(max/min)^(1/100)`).
 */
export function computeKeyboardStep(
  min: number,
  max: number,
  scale: "linear" | "log",
  step: number,
  keyboardStep?: number,
): number {
  if (scale === "log") {
    if (keyboardStep !== undefined && keyboardStep > 0) return keyboardStep;
    if (step > 0) return step;
    const safeMin = Math.max(min, 1e-6);
    return Math.pow(max / safeMin, 1 / 100);
  }
  if (keyboardStep !== undefined && keyboardStep > 0) return keyboardStep;
  if (step > 0) return step;
  return (max - min) / 100;
}

/**
 * Apply a keyboard delta to the current value, returning the new clamped,
 * rounded value. `multiplier` is one of {-10, -1, -0.1, +0.1, +1, +10}.
 *
 * The result is rounded to `decimals` (or 2 if unset) so `aria-valuetext`
 * formats cleanly. Callers may pass `Number.POSITIVE_INFINITY` or
 * `Number.NEGATIVE_INFINITY` as the multiplier sentinel for Home/End — we
 * detect those explicitly and snap to bounds.
 */
export function applyKeyboardDelta(
  value: number,
  multiplier: number,
  min: number,
  max: number,
  scale: "linear" | "log",
  step: number,
  keyboardStep: number | undefined,
  decimals?: number,
): number {
  // Home / End sentinels
  if (multiplier === Number.POSITIVE_INFINITY) return roundTo(max, decimals);
  if (multiplier === Number.NEGATIVE_INFINITY) return roundTo(min, decimals);

  const stepSize = computeKeyboardStep(min, max, scale, step, keyboardStep);
  let next: number;
  if (scale === "log") {
    const safeMin = Math.max(min, 1e-6);
    const safeVal = Math.max(value, safeMin);
    next = safeVal * Math.pow(stepSize, multiplier);
  } else {
    next = value + multiplier * stepSize;
  }
  next = clamp(next, min, max);
  return roundTo(next, decimals);
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

function roundTo(value: number, decimals?: number): number {
  const d = decimals ?? 2;
  const f = Math.pow(10, d);
  return Math.round(value * f) / f;
}
