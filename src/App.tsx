import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { DeviceSelector } from "@/components/DeviceSelector";
import { ProcessingToggle } from "@/components/ProcessingToggle";
import { MuteButton } from "@/components/MuteButton";
import { KlaarLogo } from "@/components/icons/KlaarLogo";
import { HotkeyWarningBanner } from "@/components/HotkeyWarningBanner";
import { ProfileSelector } from "@/components/ProfileSelector";
import { ProfileActions } from "@/components/ProfileActions";
import { SettingsPanel } from "@/components/SettingsPanel";
import { Meter } from "@/components/controls/Meter";
import { ClipAlert } from "@/components/ClipAlert";
import { NoiseGatePanel } from "@/components/panels/NoiseGatePanel";
import { DeEsserPanel } from "@/components/panels/DeEsserPanel";
import { CompressorPanel } from "@/components/panels/CompressorPanel";
import { LimiterPanel } from "@/components/panels/LimiterPanel";
import { EqPanel } from "@/components/eq/EqPanel";
import { RecordPlaybackPanel } from "@/components/panels/RecordPlaybackPanel";
import { useEngineStore, type EngineError } from "@/store/engineStore";
import { useProfileStore } from "@/store/profileStore";
import { useMicPermissionRecheck } from "@/hooks/useMicPermissionRecheck";
import { a11y } from "@/i18n/a11yStrings";


export default function App() {
  const loadDevices = useEngineStore((s) => s.loadDevices);
  const startMeterPolling = useEngineStore((s) => s.startMeterPolling);
  const stopMeterPolling = useEngineStore((s) => s.stopMeterPolling);
  const initEventListeners = useEngineStore((s) => s.initEventListeners);
  const destroyEventListeners = useEngineStore((s) => s.destroyEventListeners);
  const engineError = useEngineStore((s) => s.engineError);
  const clearError = useEngineStore((s) => s.clearError);
  const reconnect = useEngineStore((s) => s.reconnect);
  const selectedInputUid = useEngineStore((s) => s.selectedInputUid);
  const smoothedMeters = useEngineStore((s) => s.smoothedMeters);
  const peakMeters = useEngineStore((s) => s.peakMeters);
  const processingEnabled = useEngineStore((s) => s.processingEnabled);

  // Auto-recover from persistent mic-permission denial when the user grants
  // access in System Settings and returns to the app.
  useMicPermissionRecheck();

  const restoreSession = useProfileStore((s) => s.restoreSession);
  const fetchProfiles = useProfileStore((s) => s.fetchProfiles);
  const profileError = useProfileStore((s) => s.profileError);
  const clearProfileError = useProfileStore((s) => s.clearProfileError);

  useEffect(() => {
    // Load devices first, then restore the session — the session restore may
    // try to start the audio engine with saved device IDs, and the DeviceSelector
    // should already have the device list populated when that happens.
    void loadDevices().then(() => restoreSession());
    void fetchProfiles();
    startMeterPolling();
    void initEventListeners();
    return () => {
      stopMeterPolling();
      destroyEventListeners();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div
      className="flex flex-col h-screen overflow-hidden"
      style={{ backgroundColor: "var(--color-background)", color: "var(--color-text-primary)" }}
    >
      {/* Screen-reader-only clip-detection live region (a11y) */}
      <ClipAlert />

      {/* ── Header ──────────────────────────────────────────────────────── */}
      <header
        className="flex flex-col shrink-0 border-b"
        style={{ borderColor: "var(--color-border)", backgroundColor: "var(--color-surface)" }}
      >
        {/* ── Top row: Logo, device selectors, meters ── */}
        <div className="px-4 py-1.5">
          <div className="app-wrapper flex items-center gap-3">
            {/* Logo */}
            <div
              className="flex items-center gap-2 shrink-0"
              style={{ color: "var(--color-logo)" }}
            >
              <KlaarLogo className="h-10 w-auto" />
              <span
                className="text-sm font-bold tracking-widest uppercase"
                style={{ color: "var(--color-text-primary)" }}
              >
                Klaar
              </span>
            </div>

            {/* Divider */}
            <div className="w-px h-5 shrink-0" style={{ backgroundColor: "var(--color-border)" }} />

            {/* Device selectors — allowed to shrink */}
            <div className="flex-1 min-w-0">
              <DeviceSelector compact />
            </div>

            {/* I/O meters — right-aligned */}
            <div className="flex items-end gap-2 pb-0.5 shrink-0">
              <Meter
                level={smoothedMeters.inputLevel}
                peakLevel={peakMeters.inputLevel}
                label="IN"
                ariaLabel="Input level"
                floor={-60}
                width={14}
                height={32}
                ticks={false}
              />
              <Meter
                level={smoothedMeters.outputLevel}
                peakLevel={peakMeters.outputLevel}
                label="OUT"
                ariaLabel="Output level"
                floor={-60}
                width={14}
                height={32}
                ticks={false}
              />
            </div>

            {/* Mute button — header-pinned so it's always reachable
                regardless of which DSP panel is scrolled into view. */}
            <MuteButton />
          </div>
        </div>

        {/* ── Bottom row: Profile, processing toggle, settings ── */}
        <div
          className="px-4 py-1.5 border-t"
          style={{ borderColor: "var(--color-border)" }}
        >
          <div className="app-wrapper flex items-center gap-3">
            {/* Profile selector + actions */}
            <div className="flex items-center gap-1.5 min-w-0">
              <ProfileSelector />
              <ProfileActions />
            </div>

            {/* Spacer */}
            <div className="flex-1" />

            {/* Divider */}
            <div className="w-px h-4 shrink-0" style={{ backgroundColor: "var(--color-border)" }} />

            {/* Processing toggle */}
            <ProcessingToggle compact />

            {/* Divider */}
            <div className="w-px h-4 shrink-0" style={{ backgroundColor: "var(--color-border)" }} />

            {/* Auto-start toggle */}
            <SettingsPanel />
          </div>
        </div>
      </header>

      {/* ── Hotkey-registration warning ──────────────────────────────────── */}
      {/* Anchored directly under the header (above the engine-error banner)
          so it sits next to the mute button it explains. Self-fetching and
          self-dismissing — see `HotkeyWarningBanner`. */}
      <HotkeyWarningBanner />

      {/* ── Engine error banner ──────────────────────────────────────────── */}
      {engineError && (
        <div
          role="alert"
          className="app-wrapper px-4 py-2 text-xs shrink-0"
          style={{
            backgroundColor: engineError.type === "driver_missing" ? "#1a1a0e" : "#2a1414",
            borderBottom: engineError.type === "driver_missing" ? "1px solid #854d0e" : "1px solid #7f1d1d",
            color: engineError.type === "driver_missing" ? "#fbbf24" : "#fca5a5",
          }}
        >
          <div className="flex items-start justify-between gap-3">
          <span>{formatError(engineError)}</span>
          <div className="flex items-center gap-2 shrink-0">
            {engineError.type === "device_disconnected" && selectedInputUid && (
              <button
                className="text-xs font-medium px-2 py-0.5 rounded border cursor-pointer transition-opacity hover:opacity-80"
                style={{ borderColor: "#fca5a5", color: "#fca5a5" }}
                onClick={reconnect}
              >
                Reconnect
              </button>
            )}
            {engineError.type === "microphone_permission_denied_persistent" && (
              <button
                className="text-xs font-medium px-2 py-0.5 rounded border cursor-pointer transition-opacity hover:opacity-80"
                style={{ borderColor: "#fca5a5", color: "#fca5a5" }}
                onClick={() => {
                  void invoke("open_privacy_microphone_settings").catch((e) => {
                    console.error("Failed to open Privacy Settings:", e);
                  });
                }}
              >
                Open Privacy Settings
              </button>
            )}
            <button
              className="opacity-60 hover:opacity-100 transition-opacity cursor-pointer"
              onClick={clearError}
              aria-label={a11y.dismissEngineError()}
            >
              ✕
            </button>
          </div>
          </div>
        </div>
      )}

      {/* ── Profile error banner ─────────────────────────────────────────── */}
      {profileError && (
        <div
          role="alert"
          className="app-wrapper px-4 py-2 text-xs shrink-0"
          style={{ backgroundColor: "#1e1a0e", borderBottom: "1px solid #78350f", color: "#fcd34d" }}
        >
          <div className="flex items-start justify-between gap-3">
            <span>{profileError}</span>
            <button
              className="opacity-60 hover:opacity-100 transition-opacity cursor-pointer shrink-0"
              onClick={clearProfileError}
              aria-label={a11y.dismissProfileError()}
            >
              ✕
            </button>
          </div>
        </div>
      )}

      {/* ── Main content: signal-flow stack ─────────────────────────────── */}
      <main
        className="app-wrapper flex-1 overflow-y-auto px-3 py-3"
        style={{ backgroundColor: "var(--color-background)" }}
      >
        <div className="flex flex-col gap-2">
          {/* Signal flow order: Gate → EQ → De-esser → Compressor → Limiter */}
          <div
            className={`dsp-chain flex flex-col gap-2${processingEnabled ? "" : " dsp-chain-bypassed"}`}
            aria-label={processingEnabled ? a11y.panel.dspChain() : a11y.panel.dspChainBypassed()}
          >
            <NoiseGatePanel />
            <EqPanel />
            <DeEsserPanel />
            <CompressorPanel />
            <LimiterPanel />
          </div>

          {/* Record & Playback */}
          <RecordPlaybackPanel />
        </div>
      </main>

      {/* ── Footer ──────────────────────────────────────────────────────── */}
      <footer
        className="px-4 py-1.5 text-[10px] border-t shrink-0"
        style={{
          borderColor: "var(--color-border)",
          color: "var(--color-text-secondary)",
          backgroundColor: "var(--color-surface)",
        }}
      >
        <div className="app-wrapper">
          Route mic through Klaar. Select “Klaar” as the input in Zoom / Teams / Discord.
        </div>
      </footer>
    </div>
  );
}

function formatError(error: EngineError): string {
  if (!error) return "";
  switch (error.type) {
    case "microphone_permission_denied":
      return "Microphone access denied. Grant access in System Settings → Privacy & Security → Microphone.";
    case "microphone_permission_denied_persistent":
      return "Klaar is blocked from using the microphone. Enable it in System Settings → Privacy & Security → Microphone, then return here.";
    case "sample_rate_mismatch":
      return `Sample rate mismatch: ${error.message}`;
    case "device_disconnected":
      return "Audio device disconnected.";
    case "driver_missing":
      return "Klaar virtual driver not detected. Install the driver to route audio to communication apps.";
    case "generic":
      return error.message;
  }
}
