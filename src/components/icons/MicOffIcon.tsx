/**
 * Microphone-with-strikethrough glyph.
 *
 * Rendered by `MuteButton` while the app is *muted* — clicking the button
 * will unmute. Visual reuses the `MicIcon` body and adds a diagonal slash
 * across the icon so the muted state is recognisable at glance even in
 * grayscale (we also tint the button red when muted, but the slash carries
 * the meaning if a user has overridden colour with high-contrast / forced
 * colours mode).
 */
type Props = { className?: string };

export function MicOffIcon({ className }: Props) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 24 24"
      className={className}
      aria-hidden="true"
      focusable="false"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {/* Capsule body */}
      <rect x="9" y="2" width="6" height="12" rx="3" />
      {/* Stand arc */}
      <path d="M5 11a7 7 0 0 0 14 0" />
      {/* Stem + base */}
      <line x1="12" y1="18" x2="12" y2="22" />
      <line x1="8" y1="22" x2="16" y2="22" />
      {/* Diagonal strikethrough — drawn last so it sits on top of the
          mic body for max contrast. Slight offset from corner-to-corner
          so the slash visually clears the icon's stroke joins. */}
      <line x1="3" y1="3" x2="21" y2="21" />
    </svg>
  );
}
