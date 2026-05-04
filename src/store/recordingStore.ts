import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

// ── Types ──────────────────────────────────────────────────────────────────

interface RecordingState {
  isRecording: boolean;
  recordingId: string | null;
  /** Elapsed recording time in seconds */
  recordingDuration: number;
  isPlaying: boolean;
  /** Selected headphone device for playback */
  playbackDeviceId: string | null;

  // Internal
  _timerInterval: ReturnType<typeof setInterval> | null;
}

interface RecordingActions {
  startRecording: (maxSeconds?: number) => Promise<void>;
  stopRecording: () => Promise<void>;
  playRecording: () => Promise<void>;
  stopPlayback: () => Promise<void>;
  deleteRecording: () => Promise<void>;
  setPlaybackDevice: (uid: string) => void;
  cleanup: () => void;
}

// ── Store ──────────────────────────────────────────────────────────────────

export const useRecordingStore = create<RecordingState & RecordingActions>(
  (set, get) => ({
    // ── Initial state ────────────────────────────────────────────────────
    isRecording: false,
    recordingId: null,
    recordingDuration: 0,
    isPlaying: false,
    playbackDeviceId: null,
    _timerInterval: null,

    // ── Actions ──────────────────────────────────────────────────────────

    startRecording: async (maxSeconds = 30) => {
      try {
        await invoke("start_recording", { maxSeconds });
        // Start elapsed timer
        const interval = setInterval(() => {
          set((s) => ({ recordingDuration: s.recordingDuration + 1 }));
        }, 1000);
        set({
          isRecording: true,
          recordingDuration: 0,
          recordingId: null,
          _timerInterval: interval,
        });
      } catch (err) {
        console.error("[Klaar] start_recording failed:", err);
      }
    },

    stopRecording: async () => {
      const { _timerInterval } = get();
      if (_timerInterval !== null) {
        clearInterval(_timerInterval);
      }

      try {
        const id = await invoke<string>("stop_recording");
        set({
          isRecording: false,
          recordingId: id,
          _timerInterval: null,
        });
      } catch (err) {
        console.error("[Klaar] stop_recording failed:", err);
        set({ isRecording: false, _timerInterval: null });
      }
    },

    playRecording: async () => {
      const { recordingId, playbackDeviceId } = get();
      if (!recordingId) {
        console.warn("[Klaar] No recording to play");
        return;
      }
      if (!playbackDeviceId) {
        console.warn("[Klaar] No playback device selected");
        return;
      }

      try {
        await invoke("play_recording", {
          recordingId,
          outputDeviceUid: playbackDeviceId,
        });
        set({ isPlaying: true });
      } catch (err) {
        console.error("[Klaar] play_recording failed:", err);
      }
    },

    stopPlayback: async () => {
      try {
        await invoke("stop_playback");
        set({ isPlaying: false });
      } catch (err) {
        console.error("[Klaar] stop_playback failed:", err);
      }
    },

    deleteRecording: async () => {
      const { recordingId } = get();
      if (!recordingId) return;
      try {
        await invoke("delete_recording", { recordingId });
        set({ recordingId: null, recordingDuration: 0 });
      } catch (err) {
        console.error("[Klaar] delete_recording failed:", err);
      }
    },

    setPlaybackDevice: (uid: string) => {
      set({ playbackDeviceId: uid });
    },

    cleanup: () => {
      const { _timerInterval } = get();
      if (_timerInterval !== null) clearInterval(_timerInterval);
      set({ _timerInterval: null });
    },
  }),
);
