/**
 * Microphone glyph.
 *
 * Rendered by `MuteButton` while the app is *unmuted* — clicking the button
 * will mute. Monochrome via `currentColor` so the parent button can recolour
 * via a Tailwind text utility.
 */
type Props = { className?: string };

export function MicIcon({ className }: Props) {
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
    </svg>
  );
}
