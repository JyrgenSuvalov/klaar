/**
 * Global mute button.
 *
 * Lives in the App header so it's reachable regardless of which DSP panel
 * is scrolled into view. Reads `muted` and `toggleMute` from `engineStore`
 * — there's no local mirror because the `klaar://mute-changed` event is
 * the single source of truth (every backend mute path emits it, and the
 * store listener writes the boolean back into state). Click invokes the
 * `toggle_mute` IPC command via `engineStore.toggleMute()`; the visual
 * flip happens when the event round-trips, giving the same render path
 * for clicks, ⌃⇧M hotkey, and tray-menu toggles.
 *
 * Visual:
 *   - unmuted → neutral (mic glyph in muted-text colour, transparent bg)
 *   - muted   → lit red (mic-off glyph + red bg + red border)
 *
 * The `⌃⇧M` accelerator hint is rendered next to the icon so the chord is
 * discoverable from the UI even before the user opens the tray menu.
 */
import { useEngineStore } from "@/store/engineStore";
import { MicIcon } from "@/components/icons/MicIcon";
import { MicOffIcon } from "@/components/icons/MicOffIcon";
import { a11y } from "@/i18n/a11yStrings";

export function MuteButton() {
  const muted = useEngineStore((s) => s.muted);
  const toggleMute = useEngineStore((s) => s.toggleMute);

  const label = muted ? a11y.muteButton.unmute() : a11y.muteButton.mute();

  return (
    <button
      type="button"
      onClick={() => {
        void toggleMute();
      }}
      aria-pressed={muted}
      aria-label={label}
      title={`${label} (⌃⇧M)`}
      className={[
        "flex items-center gap-1.5 shrink-0 cursor-pointer rounded px-2 py-1",
        "text-[10px] uppercase tracking-wider font-medium",
        "border transition-colors",
        // Focus ring matches the rest of the header controls
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-0",
        muted
          ? // Muted: lit red — bg, border, and icon all share the meter-red
            // accent so the state reads at a glance without needing the
            // strikethrough glyph alone to carry the meaning.
            "bg-[--color-meter-red]/15 border-[--color-meter-red]/60 text-[--color-meter-red] " +
              "hover:bg-[--color-meter-red]/25 focus-visible:ring-[--color-meter-red]/60"
          : // Unmuted: neutral — uses the shared `--color-control-*`
            // chrome so it stays visually in lockstep with select
            // triggers and other header pills. Override only the
            // resting text colour (we want secondary while idle, then
            // primary on hover for affordance).
            "bg-[--color-control-bg] border-[--color-control-border] text-[--color-text-secondary] " +
              "hover:bg-[--color-control-bg-hover] hover:text-[--color-text-primary] " +
              "focus-visible:ring-[--color-accent]/60",
      ].join(" ")}
      data-muted={muted ? "true" : "false"}
    >
      {muted ? (
        <MicOffIcon className="w-3.5 h-3.5" />
      ) : (
        <MicIcon className="w-3.5 h-3.5" />
      )}
      {/* Grid-stack the label so the button reserves the width of the
          longer glyph string ("Muted" is wider than "Mute" at 10px
          uppercase tracking-wider). Prevents the header from shifting
          when toggling. Same pattern as BypassToggle. */}
      <span aria-hidden className="grid">
        <span className="col-start-1 row-start-1 invisible">Muted</span>
        <span className="col-start-1 row-start-1">{muted ? "Muted" : "Mute"}</span>
      </span>
      <span
        aria-hidden
        className="text-[9px] tracking-wide opacity-70"
      >
        ⌃⇧M
      </span>
    </button>
  );
}
