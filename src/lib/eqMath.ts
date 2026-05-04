/**
 * Client-side biquad frequency response calculation.
 *
 * Implements the RBJ Audio EQ Cookbook formulas (Robert Bristow-Johnson).
 * Matches the Rust backend's biquad implementation exactly so the visual
 * curve reflects what the audio engine is doing.
 *
 * All formulas: https://www.w3.org/TR/audio-eq-cookbook/
 */

import type { FilterType } from "@/store/dspStore";

// ── Butterworth constants ─────────────────────────────────────────────────

/**
 * Butterworth Q values for an 8th-order filter (4 cascaded biquad stages).
 * Derived from pole placement on the unit circle.
 */
const BUTTERWORTH_8TH_ORDER_QS = [0.5098, 0.6013, 0.8999, 2.5628] as const;

/** Reference Q for Butterworth normalisation (1/√2). */
const BUTTERWORTH_REF_Q = Math.SQRT1_2;

/** Returns the base filter type for coefficient computation. */
function baseFilterType(ft: FilterType): FilterType {
  if (ft === "highPass48") return "highPass";
  if (ft === "lowPass48") return "lowPass";
  return ft;
}

/** Returns true if this filter type uses a 4-stage Butterworth cascade. */
function is48dbFilter(ft: FilterType): boolean {
  return ft === "highPass48" || ft === "lowPass48";
}

// ── Biquad coefficients ────────────────────────────────────────────────────

export interface BiquadCoeffs {
  b0: number;
  b1: number;
  b2: number;
  a0: number; // normalisation denominator
  a1: number;
  a2: number;
}

/** Compute RBJ biquad coefficients for the given band parameters. */
export function computeCoeffs(
  filterType: FilterType,
  frequency: number,
  gain: number,
  q: number,
  sampleRate: number,
): BiquadCoeffs {
  const w0 = (2 * Math.PI * frequency) / sampleRate;
  const cos_w0 = Math.cos(w0);
  const sin_w0 = Math.sin(w0);
  const A = Math.pow(10, gain / 40); // linear amplitude (for peaking/shelf)
  const alpha = sin_w0 / (2 * q);

  let b0: number, b1: number, b2: number, a0: number, a1: number, a2: number;

  switch (filterType) {
    case "bell": {
      // Peaking EQ
      b0 = 1 + alpha * A;
      b1 = -2 * cos_w0;
      b2 = 1 - alpha * A;
      a0 = 1 + alpha / A;
      a1 = -2 * cos_w0;
      a2 = 1 - alpha / A;
      break;
    }
    case "highPass": {
      b0 = (1 + cos_w0) / 2;
      b1 = -(1 + cos_w0);
      b2 = (1 + cos_w0) / 2;
      a0 = 1 + alpha;
      a1 = -2 * cos_w0;
      a2 = 1 - alpha;
      break;
    }
    case "lowPass": {
      b0 = (1 - cos_w0) / 2;
      b1 = 1 - cos_w0;
      b2 = (1 - cos_w0) / 2;
      a0 = 1 + alpha;
      a1 = -2 * cos_w0;
      a2 = 1 - alpha;
      break;
    }
    case "highShelf": {
      const sqrtA = Math.sqrt(A);
      const alpha_s = (sin_w0 / 2) * Math.sqrt((A + 1 / A) * (1 / q - 1) + 2);
      b0 = A * (A + 1 + (A - 1) * cos_w0 + 2 * sqrtA * alpha_s);
      b1 = -2 * A * (A - 1 + (A + 1) * cos_w0);
      b2 = A * (A + 1 + (A - 1) * cos_w0 - 2 * sqrtA * alpha_s);
      a0 = A + 1 - (A - 1) * cos_w0 + 2 * sqrtA * alpha_s;
      a1 = 2 * (A - 1 - (A + 1) * cos_w0);
      a2 = A + 1 - (A - 1) * cos_w0 - 2 * sqrtA * alpha_s;
      break;
    }
    case "lowShelf": {
      const sqrtA = Math.sqrt(A);
      const alpha_s = (sin_w0 / 2) * Math.sqrt((A + 1 / A) * (1 / q - 1) + 2);
      b0 = A * (A + 1 - (A - 1) * cos_w0 + 2 * sqrtA * alpha_s);
      b1 = 2 * A * (A - 1 - (A + 1) * cos_w0);
      b2 = A * (A + 1 - (A - 1) * cos_w0 - 2 * sqrtA * alpha_s);
      a0 = A + 1 + (A - 1) * cos_w0 + 2 * sqrtA * alpha_s;
      a1 = -2 * (A - 1 + (A + 1) * cos_w0);
      a2 = A + 1 + (A - 1) * cos_w0 - 2 * sqrtA * alpha_s;
      break;
    }
    case "highPass48":
      // Delegate to base type — cascade logic is handled in combinedMagnitudeResponse
      return computeCoeffs("highPass", frequency, gain, q, sampleRate);
    case "lowPass48":
      return computeCoeffs("lowPass", frequency, gain, q, sampleRate);
    default: {
      // Identity (bypass)
      b0 = 1; b1 = 0; b2 = 0;
      a0 = 1; a1 = 0; a2 = 0;
    }
  }

  return { b0, b1, b2, a0, a1, a2 };
}

/**
 * Evaluate the magnitude response (in dB) at a single normalised frequency ω.
 *
 * |H(e^jω)| = |b0 + b1·z⁻¹ + b2·z⁻²| / |a0 + a1·z⁻¹ + a2·z⁻²|
 * where z = e^(jω), ω = 2π·f/fs
 *
 * Evaluates using the complex magnitude directly.
 */
export function magnitudeResponseAtFreq(
  coeffs: BiquadCoeffs,
  frequency: number,
  sampleRate: number,
): number {
  const { b0, b1, b2, a0, a1, a2 } = coeffs;
  const w = (2 * Math.PI * frequency) / sampleRate;
  const cos1 = Math.cos(w);
  const cos2 = Math.cos(2 * w);
  const sin1 = Math.sin(w);
  const sin2 = Math.sin(2 * w);

  // Numerator: b0 + b1·e^(-jω) + b2·e^(-2jω)
  const numRe = b0 + b1 * cos1 + b2 * cos2;
  const numIm = -(b1 * sin1 + b2 * sin2);

  // Denominator: a0 + a1·e^(-jω) + a2·e^(-2jω)
  const denRe = a0 + a1 * cos1 + a2 * cos2;
  const denIm = -(a1 * sin1 + a2 * sin2);

  const numMag = Math.sqrt(numRe * numRe + numIm * numIm);
  const denMag = Math.sqrt(denRe * denRe + denIm * denIm);

  if (denMag < 1e-12) return 0;

  return 20 * Math.log10(numMag / denMag);
}

/**
 * Generate an array of log-spaced frequencies from fMin to fMax.
 * N = number of points.
 */
export function logSpacedFreqs(fMin: number, fMax: number, n: number): number[] {
  const logMin = Math.log10(fMin);
  const logMax = Math.log10(fMax);
  return Array.from({ length: n }, (_, i) => {
    const t = i / (n - 1);
    return Math.pow(10, logMin + t * (logMax - logMin));
  });
}

/**
 * Compute the combined magnitude response (dB) of multiple EQ bands at each
 * frequency in `freqs`. Disabled bands contribute 0 dB. Responses are summed
 * in log domain (i.e., dB values are additive, equivalent to linear
 * multiplication).
 */
export interface BandInput {
  enabled: boolean;
  filterType: FilterType;
  frequency: number;
  gain: number;
  q: number;
}

export function combinedMagnitudeResponse(
  bands: BandInput[],
  freqs: number[],
  sampleRate: number,
): number[] {
  const result = new Array<number>(freqs.length).fill(0);

  for (const band of bands) {
    if (!band.enabled) continue;

    if (is48dbFilter(band.filterType)) {
      // 48 dB/oct: 3 fixed Butterworth stages + 1 proportionally-scaled stage.
      // At Q ≈ 0.707, stage 3 = 2.5628 → true 8th-order Butterworth.
      // Above 0.707, resonant peak at cutoff increases.
      const base = baseFilterType(band.filterType);
      const resonanceScale = band.q / BUTTERWORTH_REF_Q;
      const stageQs = [
        BUTTERWORTH_8TH_ORDER_QS[0],
        BUTTERWORTH_8TH_ORDER_QS[1],
        BUTTERWORTH_8TH_ORDER_QS[2],
        BUTTERWORTH_8TH_ORDER_QS[3] * resonanceScale,
      ];

      for (const stageQ of stageQs) {
        const coeffs = computeCoeffs(base, band.frequency, band.gain, stageQ, sampleRate);
        for (let i = 0; i < freqs.length; i++) {
          result[i] += magnitudeResponseAtFreq(coeffs, freqs[i], sampleRate);
        }
      }
    } else {
      // Single-stage filter
      const coeffs = computeCoeffs(
        band.filterType,
        band.frequency,
        band.gain,
        band.q,
        sampleRate,
      );

      for (let i = 0; i < freqs.length; i++) {
        result[i] += magnitudeResponseAtFreq(coeffs, freqs[i], sampleRate);
      }
    }
  }

  return result;
}
