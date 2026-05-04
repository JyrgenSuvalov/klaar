// Centralised a11y-string module.
//
// Every aria-label, aria-description, and sr-only text used by the app is
// sourced from this module. Components import `a11y` and call the
// corresponding accessor rather than hard-coding the string inline.
//
// Why function-shaped accessors (not a flat string map):
//   • Interpolation entries (e.g. "Band 3", "Compressor enabled") must be
//     functions; making non-interpolated entries also functions keeps the
//     call-site shape uniform (`a11y.foo()` everywhere).
//   • A future swap to a real i18n framework (i18next / Fluent / ICU) is
//     mechanical — replace the function bodies with calls into the
//     framework, leave call sites untouched.
//
// Strings are English. There is no locale loader and no async — this is
// prep for i18n, not i18n itself. See change `a11y-contrast-themes-zoom-i18n`.

export const a11y = {
  // ── DSP chain panels ──────────────────────────────────────────────────
  panel: {
    noiseGate: () => "Noise gate",
    eq: () => "Parametric EQ",
    deEsser: () => "De-esser",
    compressor: () => "Compressor",
    limiter: () => "Limiter",
    recordPlayback: () => "Record and playback",
    dspChain: () => "DSP chain",
    dspChainBypassed: () => "DSP chain (bypassed)",
  },

  // ── Bypass toggle ─────────────────────────────────────────────────────
  bypassToggle: (label: string | undefined) =>
    label ? `${label} enabled` : "enabled",

  // ── Header / device / processing ──────────────────────────────────────
  inputDeviceSelect: () => "Input device",
  processingToggle: () => "Processing on/off",
  autoLaunchToggle: () => "Launch at login",

  // ── Global mute ────────────────────────────────────────────────────────
  // Two distinct labels because aria-pressed alone isn't enough — VoiceOver
  // announces the state, but the *action* must read naturally in both
  // states ("Mute microphone" / "Unmute microphone"), not the conjugated
  // toggle name ("Mute toggle, pressed").
  muteButton: {
    mute: () => "Mute microphone",
    unmute: () => "Unmute microphone",
  },
  hotkeyWarningDismiss: () => "Dismiss hotkey warning",

  // ── Profile actions ───────────────────────────────────────────────────
  profileNameInput: () => "Profile name",

  // ── EQ band controls ──────────────────────────────────────────────────
  eqBandSwitch: (index: number) => `Band ${index}`,
  filterTypeSelect: () => "Filter type",

  // ── Record / playback transport ───────────────────────────────────────
  recordButton: {
    start: () => "Start recording",
    stop: () => "Stop recording",
    unavailableEngine: () => "Recording unavailable — engine not running",
    unavailablePlaying: () => "Stop playback before recording",
  },
  playRecording: () => "Play recording",
  stopPlayback: () => "Stop playback",
  playbackDeviceSelect: () => "Listen on (playback device)",

  // ── EQ canvas + spectrum overlay ──────────────────────────────────────
  eqCanvas: () =>
    "EQ frequency response and live spectrum overlay. Use the band controls below to adjust frequency, gain, and Q via keyboard.",

  // ── Banners / dismiss buttons ─────────────────────────────────────────
  dismissEngineError: () => "Dismiss error",
  dismissProfileError: () => "Dismiss profile error",
  dismissUpdateNotification: () => "Dismiss update notification",
  dialogClose: () => "Close",
} as const;

export type A11yStrings = typeof a11y;
