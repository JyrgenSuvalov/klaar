// Snapshot the keys (not the values) of the `a11y` module so future
// renames or removals are flagged in the diff. The English values are
// allowed to drift; the API surface is what consumers depend on.

import { describe, it, expect } from "vitest";
import { a11y } from "./a11yStrings";

function collectKeys(obj: unknown, prefix = ""): string[] {
  if (typeof obj === "function") return [prefix];
  if (obj === null || typeof obj !== "object") return [];
  const out: string[] = [];
  for (const [k, v] of Object.entries(obj as Record<string, unknown>)) {
    out.push(...collectKeys(v, prefix ? `${prefix}.${k}` : k));
  }
  return out.sort();
}

describe("a11y string module API", () => {
  it("exposes a stable set of keys", () => {
    expect(collectKeys(a11y)).toMatchInlineSnapshot(`
      [
        "autoLaunchToggle",
        "bypassToggle",
        "dialogClose",
        "dismissEngineError",
        "dismissProfileError",
        "dismissUpdateNotification",
        "eqBandSwitch",
        "eqCanvas",
        "filterTypeSelect",
        "hotkeyWarningDismiss",
        "inputDeviceSelect",
        "muteButton.mute",
        "muteButton.unmute",
        "panel.compressor",
        "panel.deEsser",
        "panel.dspChain",
        "panel.dspChainBypassed",
        "panel.eq",
        "panel.limiter",
        "panel.noiseGate",
        "panel.recordPlayback",
        "playRecording",
        "playbackDeviceSelect",
        "processingToggle",
        "profileNameInput",
        "recordButton.start",
        "recordButton.stop",
        "recordButton.unavailableEngine",
        "recordButton.unavailablePlaying",
        "stopPlayback",
      ]
    `);
  });
});
