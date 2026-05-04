import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

// ── Types ──────────────────────────────────────────────────────────────────

export type FilterType = "bell" | "highPass" | "lowPass" | "highShelf" | "lowShelf" | "highPass48" | "lowPass48";

export interface EqBand {
  enabled: boolean;
  filterType: FilterType;
  frequency: number;
  gain: number;
  q: number;
}

export interface NoiseGateParams {
  threshold: number;
  attack: number;
  release: number;
  range: number;
  bypassed: boolean;
}

export interface EqParams {
  bands: EqBand[];
  bypassed: boolean;
}

export interface DeEsserParams {
  frequency: number;
  threshold: number;
  ratio: number;
  attack: number;
  release: number;
  bypassed: boolean;
}

export interface CompressorParams {
  threshold: number;
  ratio: number;
  attack: number;
  release: number;
  makeupGain: number;
  bypassed: boolean;
}

export interface LimiterParams {
  ceiling: number;
  release: number;
  bypassed: boolean;
}

export interface DspState {
  noiseGate: NoiseGateParams;
  eq: EqParams;
  deEsser: DeEsserParams;
  compressor: CompressorParams;
  limiter: LimiterParams;
}

export type EffectName = "noiseGate" | "eq" | "deEsser" | "compressor" | "limiter";

// ── Defaults (must match Rust engine initial state) ────────────────────────

const DEFAULT_EQ_BAND: EqBand = {
  enabled: true,
  filterType: "bell",
  frequency: 1000,
  gain: 0,
  q: 1.0,
};

// Must exactly mirror DspParams::new() in src-tauri/src/dsp/params.rs
const DEFAULT_EQ_BANDS: EqBand[] = [
  { enabled: true, filterType: "bell", frequency: 80,    gain: 0, q: 1.0 },
  { enabled: true, filterType: "bell", frequency: 200,   gain: 0, q: 1.0 },
  { enabled: true, filterType: "bell", frequency: 500,   gain: 0, q: 1.0 },
  { enabled: true, filterType: "bell", frequency: 1000,  gain: 0, q: 1.0 },
  { enabled: true, filterType: "bell", frequency: 2500,  gain: 0, q: 1.0 },
  { enabled: true, filterType: "bell", frequency: 5000,  gain: 0, q: 1.0 },
  { enabled: true, filterType: "bell", frequency: 10000, gain: 0, q: 1.0 },
  { enabled: true, filterType: "bell", frequency: 16000, gain: 0, q: 1.0 },
];

export const DEFAULT_DSP_STATE: DspState = {
  // Must exactly mirror DspParams::new() — Rust defaults are "transparent"
  // (no processing) so the UI matches the engine on cold start before any
  // profile is loaded.
  noiseGate: {
    threshold: -80,  // effectively always open
    attack: 10,
    release: 100,
    range: -60,
    bypassed: false,
  },
  eq: {
    bands: DEFAULT_EQ_BANDS.map((b) => ({ ...b })),
    bypassed: false,
  },
  deEsser: {
    frequency: 6000,
    threshold: 0,    // no detection fires
    ratio: 4,
    attack: 1.0,     // ms
    release: 50,     // ms
    bypassed: false,
  },
  compressor: {
    threshold: 0,    // 0 dBFS threshold
    ratio: 1,        // 1:1 ratio → no compression
    attack: 10,
    release: 100,
    makeupGain: 0,
    bypassed: false,
  },
  limiter: {
    ceiling: 0,      // 0 dBFS → full headroom
    release: 100,
    bypassed: false,
  },
};

// ── Store shape ────────────────────────────────────────────────────────────

interface DspActions {
  /** Optimistically update a scalar effect param and sync to backend */
  setParam: (effect: EffectName, param: string, value: number) => Promise<void>;
  /** Optimistically update bypass state and sync to backend */
  setBypass: (effect: EffectName, bypassed: boolean) => Promise<void>;
  /** Merge a partial EqBand update into band[index] and sync to backend */
  setEqBand: (index: number, partial: Partial<EqBand>) => Promise<void>;
  /** Reset all DSP params to defaults (local only, no IPC) */
  resetToDefaults: () => void;
  /** Bulk replace all DSP state (used by profile load — no IPC, backend already updated) */
  bulkSetState: (state: DspState) => void;
}

type DspStore = DspState & DspActions;

// ── Store ──────────────────────────────────────────────────────────────────

export const useDspStore = create<DspStore>((set, get) => ({
  ...structuredClone(DEFAULT_DSP_STATE),

  // ── Task 1.2: setParam ─────────────────────────────────────────────────
  setParam: async (effect, param, value) => {
    // Capture previous value for potential rollback
    const prev = (get()[effect] as unknown as Record<string, unknown>)[param];

    // Optimistic update
    set((state) => ({
      [effect]: { ...state[effect as keyof DspState], [param]: value },
    }));

    try {
      await invoke("set_param", { effect, param, value });
    } catch (err) {
      // Revert on error
      console.error(`set_param(${effect}, ${param}) failed:`, err);
      set((state) => ({
        [effect]: { ...state[effect as keyof DspState], [param]: prev },
      }));
    }
  },

  // ── Task 1.3: setBypass ────────────────────────────────────────────────
  setBypass: async (effect, bypassed) => {
    const prev = (get()[effect] as unknown as Record<string, unknown>)["bypassed"];

    set((state) => ({
      [effect]: { ...state[effect as keyof DspState], bypassed },
    }));

    try {
      await invoke("set_bypass", { effect, bypassed });
    } catch (err) {
      console.error(`set_bypass(${effect}) failed:`, err);
      set((state) => ({
        [effect]: { ...state[effect as keyof DspState], bypassed: prev },
      }));
    }
  },

  // ── Task 1.4: setEqBand ────────────────────────────────────────────────
  setEqBand: async (index, partial) => {
    if (index < 0 || index > 7) return;

    const prevBand = get().eq.bands[index];
    const updatedBand: EqBand = { ...prevBand, ...partial };

    // Optimistic update
    set((state) => {
      const bands = [...state.eq.bands];
      bands[index] = updatedBand;
      return { eq: { ...state.eq, bands } };
    });

    try {
      await invoke("set_eq_band", {
        bandIndex: index,
        band: {
          enabled: updatedBand.enabled,
          filterType: updatedBand.filterType,
          frequency: updatedBand.frequency,
          gain: updatedBand.gain,
          q: updatedBand.q,
        },
      });
    } catch (err) {
      console.error(`set_eq_band(${index}) failed:`, err);
      // Revert
      set((state) => {
        const bands = [...state.eq.bands];
        bands[index] = prevBand;
        return { eq: { ...state.eq, bands } };
      });
    }
  },

  resetToDefaults: () => set(structuredClone(DEFAULT_DSP_STATE)),

  bulkSetState: (state: DspState) => set(structuredClone(state)),
}));

// ── Typed selectors (memoised by Zustand) ──────────────────────────────────

export const selectNoiseGate = (s: DspStore) => s.noiseGate;
export const selectEq = (s: DspStore) => s.eq;
export const selectDeEsser = (s: DspStore) => s.deEsser;
export const selectCompressor = (s: DspStore) => s.compressor;
export const selectLimiter = (s: DspStore) => s.limiter;

// Re-export DEFAULT_EQ_BAND for use in components
export { DEFAULT_EQ_BAND };
