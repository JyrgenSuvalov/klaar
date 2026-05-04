import { useEffect } from "react";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useRecordingStore } from "@/store/recordingStore";
import { useEngineStore } from "@/store/engineStore";
import { a11y } from "@/i18n/a11yStrings";

function formatTime(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return `${m}:${String(s).padStart(2, "0")}`;
}

export function RecordPlaybackPanel() {
  const isStreaming = useEngineStore((s) => s.isStreaming);
  const outputDevices = useEngineStore((s) => s.outputDevices);

  const isRecording = useRecordingStore((s) => s.isRecording);
  const recordingId = useRecordingStore((s) => s.recordingId);
  const recordingDuration = useRecordingStore((s) => s.recordingDuration);
  const isPlaying = useRecordingStore((s) => s.isPlaying);
  const playbackDeviceId = useRecordingStore((s) => s.playbackDeviceId);
  const startRecording = useRecordingStore((s) => s.startRecording);
  const stopRecording = useRecordingStore((s) => s.stopRecording);
  const playRecording = useRecordingStore((s) => s.playRecording);
  const stopPlayback = useRecordingStore((s) => s.stopPlayback);
  const deleteRecording = useRecordingStore((s) => s.deleteRecording);
  const setPlaybackDevice = useRecordingStore((s) => s.setPlaybackDevice);
  const cleanup = useRecordingStore((s) => s.cleanup);

  useEffect(() => {
    return () => cleanup();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Filter playback devices: exclude the Klaar virtual driver
  // (playing into the sink would create a feedback loop) — show
  // headphone / speaker devices only.
  const playbackDevices = outputDevices.filter(
    (d) => d.name !== "Klaar",
  );

  // Auto-select first playback device if none selected
  useEffect(() => {
    if (!playbackDeviceId && playbackDevices.length > 0) {
      setPlaybackDevice(playbackDevices[0].id);
    }
  }, [playbackDeviceId, playbackDevices, setPlaybackDevice]);

  const canRecord = isStreaming && !isPlaying;
  const canPlay = !!recordingId && !isRecording && !!playbackDeviceId;

  return (
    <div className="effect-panel" role="region" aria-label={a11y.panel.recordPlayback()}>
      <div className="panel-header">
        <span className="panel-title">Record & Playback</span>
      </div>

      <div className="panel-controls">
        <div className="flex items-center gap-3">
          {/* Record button */}
          <button
            className="flex items-center justify-center w-8 h-8 rounded-full transition-all cursor-pointer focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--color-accent)]"
            style={{
              backgroundColor: isRecording
                ? "var(--color-record-active)"
                : canRecord
                  ? "var(--color-record-idle)"
                  : "var(--color-record-disabled)",
              opacity: canRecord || isRecording ? 1 : 0.4,
              boxShadow: isRecording ? "0 0 8px var(--color-record-active)" : "none",
            }}
            onClick={() => (isRecording ? stopRecording() : startRecording(30))}
            disabled={!canRecord && !isRecording}
            aria-label={
              isRecording
                ? a11y.recordButton.stop()
                : !isStreaming
                  ? a11y.recordButton.unavailableEngine()
                  : isPlaying
                    ? a11y.recordButton.unavailablePlaying()
                    : a11y.recordButton.start()
            }
            title={
              !isStreaming
                ? "Engine not running — cannot record"
                : isPlaying
                  ? "Stop playback before recording"
                  : isRecording
                    ? "Stop recording"
                    : "Start recording (30s max)"
            }
          >
            {isRecording ? (
              // Stop icon (square)
              <div className="w-3 h-3 rounded-sm" style={{ backgroundColor: "white" }} />
            ) : (
              // Record icon (circle)
              <div className="w-3 h-3 rounded-full" style={{ backgroundColor: "white" }} />
            )}
          </button>

          {/* Timer / status */}
          <span
            className="text-xs font-mono min-w-[3rem]"
            style={{ color: isRecording ? "var(--color-record-active)" : "var(--color-text-secondary)" }}
          >
            {isRecording ? formatTime(recordingDuration) : recordingId ? formatTime(recordingDuration) : "--:--"}
          </span>

          {/* Divider */}
          <div className="w-px h-5" style={{ backgroundColor: "var(--color-border)" }} />

          {/*
            Play / Stop — single button whose icon, label, handler, and
            disabled state derive from `isPlaying`. Stable element identity
            preserves keyboard focus across the state transition.
            Mirrors the Record-button pattern above.
          */}
          <button
            className="flex items-center gap-1.5 px-2 py-1 rounded text-xs font-medium transition-colors cursor-pointer focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--color-accent)]"
            style={
              isPlaying
                ? {
                    backgroundColor: "var(--color-surface-hover)",
                    color: "var(--color-accent)",
                  }
                : {
                    backgroundColor: canPlay ? "var(--color-surface-hover)" : "transparent",
                    color: canPlay ? "var(--color-text-primary)" : "var(--color-text-secondary)",
                    opacity: canPlay ? 1 : 0.4,
                  }
            }
            onClick={() => (isPlaying ? stopPlayback() : playRecording())}
            disabled={!isPlaying && !canPlay}
            aria-label={isPlaying ? a11y.stopPlayback() : a11y.playRecording()}
            title={
              isPlaying
                ? "Stop playback"
                : !recordingId
                  ? "No recording available"
                  : !playbackDeviceId
                    ? "Select a playback device"
                    : "Play recording"
            }
          >
            {isPlaying ? <StopIcon /> : <PlayIcon />} {isPlaying ? "Stop" : "Play"}
          </button>

          {/* Delete recording */}
          {recordingId && !isRecording && !isPlaying && (
            <button
              className="text-xs px-1.5 py-0.5 rounded transition-colors cursor-pointer focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--color-accent)]"
              style={{ color: "var(--color-text-secondary)" }}
              onClick={() => deleteRecording()}
              title="Delete recording"
            >
              Delete
            </button>
          )}

          {/* Spacer */}
          <div className="flex-1" />

          {/* Playback device selector */}
          <div className="flex items-center gap-1.5">
            <span
              className="text-[10px]"
              style={{ color: "var(--color-text-secondary)" }}
            >
              Listen on:
            </span>
            {/* Shadcn Select so trigger chrome AND dropdown popup match
                the rest of the app (DeviceSelector, ProfileSelector,
                EQ filter type). Empty `value=""` is mapped to a
                sentinel `__none__` because Radix Select rejects empty
                strings as item values. */}
            <Select
              value={playbackDeviceId ?? ""}
              onValueChange={(v) => {
                if (v !== "__none__") setPlaybackDevice(v);
              }}
              disabled={playbackDevices.length === 0}
            >
              <SelectTrigger
                aria-label={a11y.playbackDeviceSelect()}
                className="h-6 text-xs px-2 py-0 w-[180px]"
                style={{ fontSize: 11 }}
              >
                <SelectValue placeholder="No devices" />
              </SelectTrigger>
              <SelectContent>
                {playbackDevices.length === 0 ? (
                  <SelectItem value="__none__" disabled>
                    No devices
                  </SelectItem>
                ) : (
                  playbackDevices.map((d) => (
                    <SelectItem key={d.id} value={d.id} className="text-xs">
                      {d.name}
                    </SelectItem>
                  ))
                )}
              </SelectContent>
            </Select>
          </div>
        </div>
      </div>
    </div>
  );
}

function PlayIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" fill="currentColor" aria-hidden="true">
      <path d="M3 1.5v9l7.5-4.5L3 1.5z" />
    </svg>
  );
}

function StopIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" fill="currentColor" aria-hidden="true">
      <rect x="2" y="2" width="8" height="8" rx="1" />
    </svg>
  );
}
