import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ── Types ──────────────────────────────────────────────────────────────────

export interface AudioDevice {
  id: string;
  name: string;
  sampleRate: number;
  channels: number;
}

export interface MeterData {
  inputLevel: number;
  outputLevel: number;
  gateReduction: number;
  deEsserReduction: number;
  /**
   * De-esser sidechain (post-bandpass) peak level in dBFS, range [-96, 0]
   * as published by the backend. The DeEsserPanel renders this against a
   * narrower display floor (-60 dB) because real-speech sidechain
   * readings live in the -10 to -50 range — see `DeEsserPanel.tsx`.
   * Smoothed with the same level-meter envelope as `inputLevel` /
   * `outputLevel`. Published every buffer by the audio thread, even when
   * the de-esser is bypassed. See change `improve-deesser-tweaking`.
   */
  deEsserSidechainDb: number;
  compressorReduction: number;
  limiterReduction: number;
}

export type EngineError =
  | { type: "sample_rate_mismatch"; message: string }
  /**
   * Reactive variant — CoreAudio returned `-66748`/`10863` when opening or
   * starting the input AudioUnit. Kept as a belt-and-braces fallback; the
   * proactive gate (below) is the primary detection path.
   */
  | { type: "microphone_permission_denied" }
  /**
   * Proactive variant — `AVCaptureDevice.authorizationStatus` returned
   * `Denied` / `Restricted`, or the TCC prompt resolved to "Don't Allow".
   * Fires before any CoreAudio call, and carries a recovery affordance
   * (Open Privacy Settings) in the banner.
   */
  | { type: "microphone_permission_denied_persistent" }
  | { type: "device_disconnected" }
  | { type: "driver_missing" }
  | { type: "generic"; message: string }
  | null;

// ── Store shape ────────────────────────────────────────────────────────────

/**
 * Clip-event signal exposed for accessibility.
 * Increments by 1 each time the input or output peak meter crosses
 * −1 dBFS upward after a quiet period. The frontend `<ClipAlert>`
 * watches the counters and announces "Input clipping" / "Output
 * clipping" once per transition via a `role="alert"` live region.
 *
 * Counters (not booleans) so successive clip events on the same
 * channel still trigger React state updates without needing a
 * separate "consumed" flag.
 */
export interface ClipEvents {
  inputClipCount: number;
  outputClipCount: number;
}

interface EngineState {
  // Device lists
  inputDevices: AudioDevice[];
  /**
   * All CoreAudio output devices — used by RecordPlaybackPanel to populate
   * the "Listen on" headphone selector. The engine's routing target
   * (the Klaar virtual driver) is hard-wired on the Rust side;
   * there is no user-facing output selector.
   */
  outputDevices: AudioDevice[];

  // Current mic selection
  selectedInputUid: string | null;

  // Engine state
  processingEnabled: boolean;
  isStreaming: boolean;
  engineError: EngineError;

  // Meters — raw values from IPC
  meters: MeterData;

  // Meters — smoothed (exponential decay) for display
  smoothedMeters: MeterData;

  // Peak hold values (highest smoothed value, held for ~1.5s then decaying)
  peakMeters: MeterData;

  // Clip-event counters (driven by the meter polling loop, only mutated on
  // transition so React re-renders are rare).
  clipEvents: ClipEvents;

  // Global mute.
  // Source of truth lives in the Rust DSP layer; this is the cached UI
  // mirror, kept in sync via the `klaar://mute-changed` event. NOT persisted
  // and NOT part of profiles — every fresh launch is unmuted.
  muted: boolean;

  // Internal — polling handle
  _pollingRafId: number | null;
  _disconnectUnlisten: UnlistenFn | null;
  _devicesChangedUnlisten: UnlistenFn | null;
  _muteChangedUnlisten: UnlistenFn | null;
}

interface EngineActions {
  // Device loading
  loadDevices: () => Promise<void>;

  // Mic selection (output is hard-wired to the Klaar driver)
  selectInputDevice: (uid: string) => Promise<void>;

  // Reconnect after device disconnection (re-uses stored input UID)
  reconnect: () => Promise<void>;

  // Processing toggle
  setProcessingEnabled: (enabled: boolean) => Promise<void>;

  // Meter polling
  startMeterPolling: () => void;
  stopMeterPolling: () => void;

  // Event listener setup
  initEventListeners: () => Promise<void>;
  destroyEventListeners: () => void;

  // Global mute. `toggleMute` flips the backend state via the IPC chokepoint;
  // we deliberately do NOT update local state optimistically — the
  // `klaar://mute-changed` event fires immediately from the same chokepoint
  // and is the single source of truth for UI updates (so hotkey, tray menu,
  // and IPC paths all converge on one render path).
  toggleMute: () => Promise<void>;

  // Error
  clearError: () => void;
}

// ── Smoothing constants ────────────────────────────────────────────────────

// Target: decay from 0 dBFS to -40 dBFS in ~1.5 s at 60 fps (90 frames).
// Per-frame decay coefficient: e^(-1 / 90) ≈ 0.9889
const DECAY_COEFF = Math.exp(-1 / 90);

// Peak hold: ~1.5 s at 60 fps
const PEAK_HOLD_FRAMES = 90;
// Peak decay rate in dB per frame (after hold expires)
const PEAK_FALL_DB = 0.2;

// ── Default meter values ───────────────────────────────────────────────────

const SILENCE_METERS: MeterData = {
  inputLevel: -96,
  outputLevel: -96,
  gateReduction: 0,
  deEsserReduction: 0,
  deEsserSidechainDb: -96,
  compressorReduction: 0,
  limiterReduction: 0,
};

// ── Peak hold state (module-level mutable, only touched by polling loop) ──

type MeterKey = keyof MeterData;
const METER_KEYS: MeterKey[] = [
  "inputLevel",
  "outputLevel",
  "gateReduction",
  "deEsserReduction",
  "deEsserSidechainDb",
  "compressorReduction",
  "limiterReduction",
];

/** Current smoothed values (interpolated between raw IPC readings) */
const _smoothed: MeterData = { ...SILENCE_METERS };

/** Peak hold values per meter */
const _peakValue: Record<MeterKey, number> = {
  inputLevel: -96,
  outputLevel: -96,
  gateReduction: 0,
  deEsserReduction: 0,
  deEsserSidechainDb: -96,
  compressorReduction: 0,
  limiterReduction: 0,
};

/** Remaining hold frames per meter */
const _peakHold: Record<MeterKey, number> = {
  inputLevel: 0,
  outputLevel: 0,
  gateReduction: 0,
  deEsserReduction: 0,
  deEsserSidechainDb: 0,
  compressorReduction: 0,
  limiterReduction: 0,
};

export type { MeterKey };
export { METER_KEYS };

/**
 * Whether a meter key is a level reading (dBFS, range [-96, 0], higher = louder)
 * versus a gain-reduction reading (negative dB, more-negative = more reduction).
 * Determines smoothing direction and peak-hold floor in the polling loop.
 */
const LEVEL_METERS: ReadonlySet<MeterKey> = new Set([
  "inputLevel",
  "outputLevel",
  "deEsserSidechainDb",
]);

// ── Clip-event detection (a11y) ──────────────────────────────────────────
//
// Module-local refs so the gate logic does not churn React state on every
// frame. Only the transition (counter increment) reaches the store.
//
// Thresholds:
//   CLIP_ENTER_DBFS — peak must cross above this to register a new clip
//   CLIP_EXIT_DBFS  — peak must drop below this to "clear" the clip event
//   CLIP_QUIET_MS   — minimum quiet period after a clear before a new event
const CLIP_ENTER_DBFS = -1;
const CLIP_EXIT_DBFS = -3;
const CLIP_QUIET_MS = 3000;

/**
 * Per-channel clip state.
 *  - active: currently above the enter threshold (gating re-fire while still loud)
 *  - lastClearedAt: timestamp (ms) of the most recent transition out of clipping;
 *    a new clip event may only start CLIP_QUIET_MS after this.
 */
interface ClipState {
  active: boolean;
  lastClearedAt: number;
}

const _clipInput: ClipState = { active: false, lastClearedAt: -Infinity };
const _clipOutput: ClipState = { active: false, lastClearedAt: -Infinity };

/**
 * Update one channel's clip state given the current peak value and a "now"
 * timestamp. Returns `true` iff a new clip event just started (caller should
 * bump the counter in store state).
 *
 * Exported for tests.
 */
export function _stepClipState(
  state: ClipState,
  level: number,
  now: number,
  enterDbfs: number = CLIP_ENTER_DBFS,
  exitDbfs: number = CLIP_EXIT_DBFS,
  quietMs: number = CLIP_QUIET_MS,
): boolean {
  if (!state.active) {
    if (level >= enterDbfs && now - state.lastClearedAt >= quietMs) {
      state.active = true;
      return true;
    }
    return false;
  }
  // currently active — wait for hysteresis exit
  if (level <= exitDbfs) {
    state.active = false;
    state.lastClearedAt = now;
  }
  return false;
}

/** Reset module-level smoothing + clip state to silence. FOR TESTS ONLY. */
export function _resetSmoothingForTest(): void {
  for (const key of METER_KEYS) {
    _smoothed[key] = SILENCE_METERS[key];
    _peakValue[key] = SILENCE_METERS[key];
    _peakHold[key] = 0;
  }
  _clipInput.active = false;
  _clipInput.lastClearedAt = -Infinity;
  _clipOutput.active = false;
  _clipOutput.lastClearedAt = -Infinity;
}

// ── Store ──────────────────────────────────────────────────────────────────

export const useEngineStore = create<EngineState & EngineActions>((set, get) => ({
  // ── Initial state ──────────────────────────────────────────────────────
  inputDevices: [],
  outputDevices: [],
  selectedInputUid: null,
  processingEnabled: true,
  isStreaming: false,
  engineError: null,
  meters: { ...SILENCE_METERS },
  smoothedMeters: { ...SILENCE_METERS },
  peakMeters: { ...SILENCE_METERS },
  clipEvents: { inputClipCount: 0, outputClipCount: 0 },
  muted: false,
  _pollingRafId: null,
  _disconnectUnlisten: null,
  _devicesChangedUnlisten: null,
  _muteChangedUnlisten: null,

  // ── Actions ────────────────────────────────────────────────────────────

  loadDevices: async () => {
    const [inputDevices, outputDevices] = await Promise.all([
      invoke<AudioDevice[]>("list_input_devices"),
      invoke<AudioDevice[]>("list_output_devices"),
    ]);

    // Check whether the Klaar virtual driver is installed and visible.
    const DRIVER_DEVICE_NAME = "Klaar";
    const driverPresent = outputDevices.some(
      (d) => d.name === DRIVER_DEVICE_NAME,
    );
    const currentError = get().engineError;
    const newError: EngineError = driverPresent
      ? // Clear driver_missing if it was previously set
        currentError?.type === "driver_missing"
        ? null
        : currentError
      : { type: "driver_missing" };

    set({ inputDevices, outputDevices, engineError: newError });
  },

  selectInputDevice: async (uid: string) => {
    set({ engineError: null });
    try {
      // Backend returns true when the engine started, false otherwise.
      const started = await invoke<boolean>("set_input_device", { uid });
      set({ selectedInputUid: uid, isStreaming: started });
    } catch (err) {
      const error = parseEngineError(err);
      set({ engineError: error, isStreaming: false });
    }
  },

  reconnect: async () => {
    const { selectedInputUid, selectInputDevice } = get();
    if (!selectedInputUid) return;
    // Re-invoking selection with the stored UID re-triggers try_start_engine on the backend.
    await selectInputDevice(selectedInputUid);
  },

  setProcessingEnabled: async (enabled: boolean) => {
    const prev = get().processingEnabled;
    // Optimistic update — mirrors the pattern used in dspStore.setParam/setBypass
    set({ processingEnabled: enabled });
    try {
      await invoke("set_processing_enabled", { enabled });
    } catch (err) {
      // Revert and surface the error
      set({
        processingEnabled: prev,
        engineError: { type: "generic", message: String(err) },
      });
    }
  },

  startMeterPolling: () => {
    const { _pollingRafId } = get();
    if (_pollingRafId !== null) return; // already polling

    // Throttle IPC calls: fetch raw meters every ~3 frames (~20 Hz) to reduce
    // overhead; apply smoothing every frame for visual continuity.
    let ipcFrameCounter = 0;

    // Async inner function handles IPC; synchronous outer poll satisfies RAF's void signature.
    let _firstNonSilenceLogged = false;
    const fetchMeters = async () => {
      if (ipcFrameCounter % 3 === 0) {
        try {
          const raw = await invoke<MeterData>("get_meters");
          set({ meters: raw });
          // One-time log when we first receive a non-silence input level so it is
          // easy to verify in DevTools that the IPC pipeline is working end-to-end.
          if (import.meta.env.DEV && !_firstNonSilenceLogged && raw.inputLevel > -90) {
            _firstNonSilenceLogged = true;
            console.debug("[Klaar] First non-silence meter reading:", raw);
          }
        } catch (err) {
          // Unexpected — get_meters is a plain command that never returns Err.
          // Log once so the issue is visible in DevTools.
          console.error("[Klaar] get_meters IPC failed:", err);
        }
      }
    };

    const poll = () => {
      // Fire-and-forget IPC fetch (throttled to ~20 Hz)
      void fetchMeters();
      ipcFrameCounter++;

      const raw = get().meters;

      // Apply exponential decay smoothing: instant attack (rise), decay on fall
      for (const key of METER_KEYS) {
        const rawVal = raw[key];
        const prev = _smoothed[key];

        // Level meters: higher raw = louder (closer to 0), so "attack" is rawVal > prev.
        // GR meters: raw values are 0 or negative; more negative = more reduction,
        // so "attack" is a larger absolute magnitude.
        const isLevelMeter = LEVEL_METERS.has(key);
        const isAttack = isLevelMeter
          ? rawVal > prev
          : Math.abs(rawVal) >= Math.abs(prev);

        if (isAttack) {
          // Instant attack — snap to raw value
          _smoothed[key] = rawVal;
        } else {
          // Exponential decay toward the raw value
          _smoothed[key] = prev * DECAY_COEFF + rawVal * (1 - DECAY_COEFF);
        }

        // Peak hold tracking
        const smoothedVal = _smoothed[key];
        const prevPeak = _peakValue[key];

        // Same direction logic: louder level = larger value; more GR = larger magnitude
        if (isLevelMeter ? smoothedVal > prevPeak : Math.abs(smoothedVal) >= Math.abs(prevPeak)) {
          _peakValue[key] = smoothedVal;
          _peakHold[key] = PEAK_HOLD_FRAMES;
        } else if (_peakHold[key] > 0) {
          _peakHold[key]--;
        } else {
          // Decay peak back toward floor/zero
          if (isLevelMeter) {
            _peakValue[key] = Math.max(-96, _peakValue[key] - PEAK_FALL_DB);
          } else {
            // GR meters: decay toward 0 (no reduction)
            _peakValue[key] = Math.min(0, _peakValue[key] + PEAK_FALL_DB);
          }
        }
      }

      // ── Clip detection (a11y) ────────────────────────────────────────
      // Run AFTER smoothing/peak so the latest peak is used. Increment
      // counters only on rising-edge transitions; the alert component
      // watches the counters and announces on change.
      const now =
        typeof performance !== "undefined" ? performance.now() : Date.now();
      const inputFired = _stepClipState(_clipInput, _peakValue.inputLevel, now);
      const outputFired = _stepClipState(_clipOutput, _peakValue.outputLevel, now);

      // Commit smoothed + peak values AND schedule the next frame in a single
      // batched Zustand update (one subscriber notification per frame).
      const update: Partial<EngineState> = {
        smoothedMeters: { ..._smoothed },
        peakMeters: { ..._peakValue },
        _pollingRafId: requestAnimationFrame(poll),
      };
      if (inputFired || outputFired) {
        const prev = get().clipEvents;
        update.clipEvents = {
          inputClipCount: prev.inputClipCount + (inputFired ? 1 : 0),
          outputClipCount: prev.outputClipCount + (outputFired ? 1 : 0),
        };
      }
      set(update);
    };

    // Kick off first frame and immediately store the ID so the guard works for
    // any synchronous re-entrant calls before the first callback fires.
    set({ _pollingRafId: requestAnimationFrame(poll) });
  },

  stopMeterPolling: () => {
    const { _pollingRafId } = get();
    if (_pollingRafId !== null) {
      cancelAnimationFrame(_pollingRafId);
      set({ _pollingRafId: null });
    }
  },

  initEventListeners: async () => {
    // Listen for device disconnection events from the Rust backend
    const unlisten = await listen<void>("audio://device-disconnected", () => {
      // Wrap async work in void so the synchronous callback signature is satisfied.
      void (async () => {
        // Explicitly stop the engine — the backend cannot safely call stop() from
        // the monitor callback without risking deadlock, so the frontend does it.
        await invoke("stop_engine").catch(() => {}); // best-effort; ignore if already stopped
        // Reset smoothing + clip state
        for (const key of METER_KEYS) {
          _smoothed[key] = SILENCE_METERS[key];
          _peakValue[key] = SILENCE_METERS[key];
          _peakHold[key] = 0;
        }
        _clipInput.active = false;
        _clipInput.lastClearedAt = -Infinity;
        _clipOutput.active = false;
        _clipOutput.lastClearedAt = -Infinity;
        set({
          isStreaming: false,
          meters: { ...SILENCE_METERS },
          smoothedMeters: { ...SILENCE_METERS },
          peakMeters: { ...SILENCE_METERS },
          engineError: { type: "device_disconnected" },
        });
      })();
    });
    // Listen for system-wide device list changes (device plugged/unplugged).
    //
    // Auto-reconnect policy: when the driver reappears (e.g. after
    // install_driver completes or the user re-enables it in Audio MIDI Setup)
    // and we have a previously-selected mic but no live stream, start the
    // engine automatically so the user doesn't have to click Reconnect.
    const unlistenDevicesChanged = await listen<void>("audio://devices-changed", () => {
      // Note the prior error so QA can distinguish:
      //   - sample_rate_mismatch → user changed SR in AMS
      //   - driver_missing → driver was just (re)installed
      //   - device_disconnected → an in-use device's SR drifted mid-stream
      //     (DeviceFormatListener stopped the engine, now triggering reconnect)
      const priorErrorType = get().engineError?.type ?? null;
      console.debug(
        "[Klaar] Device list changed — re-enumerating (priorError=%s)",
        priorErrorType
      );
      void (async () => {
        await get().loadDevices();
        const { selectedInputUid, isStreaming, engineError } = get();
        const driverPresent = engineError?.type !== "driver_missing";
        if (selectedInputUid && !isStreaming && driverPresent) {
          console.debug(
            "[Klaar] Auto-reconnecting after devices-changed (priorError=%s)",
            priorErrorType
          );
          await get().reconnect();
        }
      })();
    });

    // Subscribe to global mute changes. The Rust chokepoint emits this event
    // on every toggle / set / hotkey fire, so the UI mirror only needs the
    // listener — no polling. We also fetch the initial state once here so a
    // window reopen after a hotkey-driven mute starts in the right state.
    const unlistenMute = await listen<{ muted: boolean }>(
      "klaar://mute-changed",
      (event) => {
        set({ muted: !!event.payload?.muted });
      },
    );
    try {
      const initialMuted = await invoke<boolean>("get_mute_state");
      set({ muted: !!initialMuted });
    } catch (err) {
      // Non-fatal: stale `false` is the safe default. The next event will
      // resync. Log so test runs and DevTools see the failure.
      console.warn("[Klaar] get_mute_state IPC failed:", err);
    }

    set({
      _disconnectUnlisten: unlisten,
      _devicesChangedUnlisten: unlistenDevicesChanged,
      _muteChangedUnlisten: unlistenMute,
    });
  },

  destroyEventListeners: () => {
    const { _disconnectUnlisten, _devicesChangedUnlisten, _muteChangedUnlisten } = get();
    _disconnectUnlisten?.();
    _devicesChangedUnlisten?.();
    _muteChangedUnlisten?.();
    set({
      _disconnectUnlisten: null,
      _devicesChangedUnlisten: null,
      _muteChangedUnlisten: null,
    });
  },

  toggleMute: async () => {
    try {
      // Backend returns the new state, but we ignore the return value: the
      // `klaar://mute-changed` listener above is the single render path.
      // Skipping the optimistic update means hotkey, tray-menu and button
      // all flow through the same code path with no risk of UI desync.
      await invoke<boolean>("toggle_mute");
    } catch (err) {
      // Don't surface to engineError — mute failures shouldn't trigger the
      // big red disconnect banner. Log for debug visibility instead.
      console.error("[Klaar] toggle_mute IPC failed:", err);
    }
  },

  clearError: () => set({ engineError: null }),
}));

// ── Helpers ────────────────────────────────────────────────────────────────

function parseEngineError(err: unknown): EngineError {
  const msg = String(err);
  // IMPORTANT: the persistent variant must be checked BEFORE the reactive
  // variant, because the reactive substring is a prefix of the persistent
  // one (`microphone_permission_denied` vs
  // `microphone_permission_denied_persistent`).
  if (msg.includes("microphone_permission_denied_persistent")) {
    return { type: "microphone_permission_denied_persistent" };
  }
  if (msg.includes("microphone_permission_denied")) {
    return { type: "microphone_permission_denied" };
  }
  if (msg.includes("sample_rate_mismatch")) {
    return { type: "sample_rate_mismatch", message: msg };
  }
  if (msg.includes("driver_missing")) {
    return { type: "driver_missing" };
  }
  return { type: "generic", message: msg };
}
