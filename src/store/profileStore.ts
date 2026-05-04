import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { useDspStore, type DspState, type EqBand, type FilterType } from "./dspStore";
import { useEngineStore, type EngineError } from "./engineStore";

/**
 * Map a `restore_session` warning string like
 * `"Failed to start audio engine: microphone_permission_denied_persistent"`
 * to a frontend `EngineError`. Returns `null` if the warning doesn't match
 * a known failure mode — caller can fall through to its existing logging.
 *
 * Kept in-sync with `parseEngineError` in `engineStore.ts`. Same precedence
 * rules (persistent variant checked before reactive variant).
 */
function parseEngineErrorFromWarning(warning: string): EngineError {
  if (warning.includes("microphone_permission_denied_persistent")) {
    return { type: "microphone_permission_denied_persistent" };
  }
  if (warning.includes("microphone_permission_denied")) {
    return { type: "microphone_permission_denied" };
  }
  if (warning.includes("driver_missing")) {
    return { type: "driver_missing" };
  }
  if (warning.includes("sample_rate_mismatch")) {
    return { type: "sample_rate_mismatch", message: warning };
  }
  return null;
}

// ── Types (match Rust serialisation) ─────────────────────────────────────

export interface ProfileSummary {
  id: string;
  name: string;
  updatedAt: string;
}

/** Serialisable filter type variants — matches the Rust `FilterTypeSerde` enum. */
export type FilterTypeSerde = "bell" | "highPass" | "lowPass" | "highShelf" | "lowShelf" | "highPass48" | "lowPass48" | "unknown";

/** Matches Rust AllDspParams serialised as camelCase JSON */
export interface AllDspParams {
  noiseGate: {
    threshold: number;
    attack: number;
    release: number;
    range: number;
    bypassed: boolean;
  };
  eq: {
    bands: Array<{
      enabled: boolean;
      filterType: FilterTypeSerde;
      frequency: number;
      gain: number;
      q: number;
    }>;
    bypassed: boolean;
  };
  deEsser: {
    frequency: number;
    threshold: number;
    ratio: number;
    attackMs: number;
    releaseMs: number;
    bypassed: boolean;
  };
  compressor: {
    threshold: number;
    ratio: number;
    attack: number;
    release: number;
    makeupGain: number;
    bypassed: boolean;
  };
  limiter: {
    ceiling: number;
    release: number;
    bypassed: boolean;
  };
}

export interface AppSettings {
  inputDeviceId: string | null;
  outputDeviceId: string | null;
  activeProfileId: string | null;
  autoLaunch: boolean;
  processingEnabled: boolean;
  sampleRate: number;
  bufferSize: number;
}

export interface RestoreSessionResult {
  settings: AppSettings;
  loadedProfileParams: AllDspParams | null;
  warnings: string[];
}

// ── Store shape ──────────────────────────────────────────────────────────

interface ProfileState {
  profiles: ProfileSummary[];
  activeProfileId: string | null;
  activeProfileName: string | null;
  autoLaunch: boolean;
  isLoading: boolean;
  /** Non-null when a profile operation (save/load/delete/update) fails. */
  profileError: string | null;
}

interface ProfileActions {
  fetchProfiles: () => Promise<void>;
  loadProfile: (id: string) => Promise<void>;
  saveProfile: (name: string) => Promise<void>;
  updateProfile: (id: string) => Promise<void>;
  deleteProfile: (id: string) => Promise<void>;
  setAutoLaunch: (enabled: boolean) => Promise<void>;
  restoreSession: () => Promise<RestoreSessionResult>;
  setActiveProfile: (id: string | null, name: string | null) => void;
  clearProfileError: () => void;
}

type ProfileStore = ProfileState & ProfileActions;

// ── Helper: convert AllDspParams from Rust to DspState for the dspStore ──

function allParamsToDspState(params: AllDspParams): DspState {
  return {
    noiseGate: {
      threshold: params.noiseGate.threshold,
      attack: params.noiseGate.attack,
      release: params.noiseGate.release,
      range: params.noiseGate.range,
      bypassed: params.noiseGate.bypassed,
    },
    eq: {
      bands: params.eq.bands.map((b) => ({
        enabled: b.enabled,
        // "unknown" is the serde catch-all for unrecognised filter types; map to bell.
        filterType: (b.filterType === "unknown" ? "bell" : b.filterType) as FilterType,
        frequency: b.frequency,
        gain: b.gain,
        q: b.q,
      })) as EqBand[],
      bypassed: params.eq.bypassed,
    },
    deEsser: {
      frequency: params.deEsser.frequency,
      threshold: params.deEsser.threshold,
      ratio: params.deEsser.ratio,
      attack: params.deEsser.attackMs,
      release: params.deEsser.releaseMs,
      bypassed: params.deEsser.bypassed,
    },
    compressor: {
      threshold: params.compressor.threshold,
      ratio: params.compressor.ratio,
      attack: params.compressor.attack,
      release: params.compressor.release,
      makeupGain: params.compressor.makeupGain,
      bypassed: params.compressor.bypassed,
    },
    limiter: {
      ceiling: params.limiter.ceiling,
      release: params.limiter.release,
      bypassed: params.limiter.bypassed,
    },
  };
}

// ── Store ────────────────────────────────────────────────────────────────

/** Extract a human-readable message from an IPC error (string or Error object). */
function extractError(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return String(err);
}

export const useProfileStore = create<ProfileStore>((set, get) => ({
  profiles: [],
  activeProfileId: null,
  activeProfileName: null,
  autoLaunch: false,
  isLoading: false,
  profileError: null,

  clearProfileError: () => set({ profileError: null }),

  fetchProfiles: async () => {
    try {
      const profiles = await invoke<ProfileSummary[]>("list_profiles");
      set({ profiles });
    } catch (err) {
      console.error("Failed to fetch profiles:", err);
      // Listing failures are silent — the UI just shows an empty dropdown
    }
  },

  loadProfile: async (id: string) => {
    set({ isLoading: true, profileError: null });
    try {
      const params = await invoke<AllDspParams>("load_profile", { id });

      // Update dspStore without triggering individual IPC calls
      const dspState = allParamsToDspState(params);
      useDspStore.getState().bulkSetState(dspState);

      // Find the profile name
      const profile = get().profiles.find((p) => p.id === id);
      set({
        activeProfileId: id,
        activeProfileName: profile?.name ?? null,
        isLoading: false,
      });
    } catch (err) {
      console.error("Failed to load profile:", err);
      set({ isLoading: false, profileError: `Failed to load profile: ${extractError(err)}` });
    }
  },

  saveProfile: async (name: string) => {
    set({ profileError: null });
    try {
      const summary = await invoke<ProfileSummary>("save_profile", { name });
      set({
        activeProfileId: summary.id,
        activeProfileName: summary.name,
      });
      // Refresh profile list
      await get().fetchProfiles();
    } catch (err) {
      console.error("Failed to save profile:", err);
      set({ profileError: `Failed to save profile: ${extractError(err)}` });
    }
  },

  updateProfile: async (id: string) => {
    set({ profileError: null });
    try {
      await invoke<ProfileSummary>("update_profile", { id });
      // Refresh profile list (updatedAt changed)
      await get().fetchProfiles();
    } catch (err) {
      console.error("Failed to update profile:", err);
      set({ profileError: `Failed to update profile: ${extractError(err)}` });
    }
  },

  deleteProfile: async (id: string) => {
    set({ profileError: null });
    try {
      await invoke("delete_profile", { id });

      // If we deleted the active profile, clear it
      if (get().activeProfileId === id) {
        set({ activeProfileId: null, activeProfileName: null });
      }

      // Refresh profile list
      await get().fetchProfiles();
    } catch (err) {
      console.error("Failed to delete profile:", err);
      set({ profileError: `Failed to delete profile: ${extractError(err)}` });
    }
  },

  setAutoLaunch: async (enabled: boolean) => {
    try {
      await invoke("set_auto_launch", { enabled });
      set({ autoLaunch: enabled });
    } catch (err) {
      console.error("Failed to set auto-launch:", err);
      set({ profileError: `Failed to update auto-launch setting: ${extractError(err)}` });
    }
  },

  restoreSession: async () => {
    set({ isLoading: true });
    try {
      const result = await invoke<RestoreSessionResult>("restore_session");

      // Update profile store state from settings
      set({
        activeProfileId: result.settings.activeProfileId,
        autoLaunch: result.settings.autoLaunch,
      });

      // Sync mic and processing state to engineStore so DeviceSelector,
      // ProcessingToggle, and isStreaming reflect the restored state.
      // The output device is hard-wired to the Klaar driver — the backend
      // resolves it at engine start; no persisted output UID is read here.
      const { inputDeviceId, processingEnabled } = result.settings;
      if (inputDeviceId) {
        useEngineStore.setState({ selectedInputUid: inputDeviceId });
      }
      useEngineStore.setState({ processingEnabled });
      // Mark as streaming only when the mic was restored and the engine started
      const engineWarning = result.warnings.find((w) =>
        w.includes("Failed to start audio engine"),
      );
      const engineFailed = engineWarning !== undefined;
      if (inputDeviceId && !engineFailed) {
        useEngineStore.setState({ isStreaming: true });
      }
      // Route the engine-failure warning through the same EngineError parser
      // that the set_input_device command uses, so the banner (including the
      // persistent mic-permission variant with its Privacy Settings button)
      // surfaces on a fresh launch that can't start the engine.
      if (engineWarning) {
        const parsed = parseEngineErrorFromWarning(engineWarning);
        if (parsed) {
          useEngineStore.setState({ engineError: parsed, isStreaming: false });
        }
      }

      // If a profile was loaded, sync its params to the DSP store
      if (result.loadedProfileParams) {
        const dspState = allParamsToDspState(result.loadedProfileParams);
        useDspStore.getState().bulkSetState(dspState);

        // Find the profile name
        const profiles = await invoke<ProfileSummary[]>("list_profiles");
        const active = profiles.find((p) => p.id === result.settings.activeProfileId);
        set({
          profiles,
          activeProfileName: active?.name ?? null,
          isLoading: false,
        });
      } else {
        await get().fetchProfiles();
        set({ isLoading: false });
      }

      // Log warnings
      for (const w of result.warnings) {
        console.warn("Session restore:", w);
      }

      return result;
    } catch (err) {
      console.error("Failed to restore session:", err);
      set({ isLoading: false });
      throw err;
    }
  },

  setActiveProfile: (id, name) => {
    set({ activeProfileId: id, activeProfileName: name });
  },
}));
