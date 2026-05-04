/**
 * Klaar wordmark glyph — the stylised waveform/capsule that sits to the
 * left of the "Klaar" text in the app header. Replaces the earlier
 * placeholder accent dot.
 *
 * Monochrome via `currentColor` so the parent can theme it with a
 * Tailwind text utility (header uses `text-[--color-accent]` to match
 * the rest of the accent chrome). `aria-hidden` because the adjacent
 * "Klaar" wordmark already provides the accessible name.
 *
 * Source artwork: `assets/logo.svg` at the repo root. Viewbox preserved so the
 * proportions match the master; size with `h-*` and let the width track
 * automatically (aspect ≈ 0.94).
 */
type Props = { className?: string };

export function KlaarLogo({ className }: Props) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 340 363"
      className={className}
      aria-hidden="true"
      focusable="false"
      fill="none"
      stroke="currentColor"
      strokeLinecap="round"
    >
      {/* Inner short bars */}
      <path d="M70 231.25V131.25" strokeWidth="10" />
      <path d="M270 231.25V131.25" strokeWidth="10" />
      <path d="M45 200V162.5" strokeWidth="10" />
      <path d="M295 200V162.5" strokeWidth="10" />
      <path d="M95 260V102.708" strokeWidth="10" />
      <path d="M245 260V102.708" strokeWidth="10" />
      <path d="M120 293.75V68.75" strokeWidth="10" />
      <path d="M220 293.75V68.75" strokeWidth="10" />
      <path d="M170 356.25V6.25" strokeWidth="10" />
      <path d="M145 325V37.5" strokeWidth="10" />
      <path d="M195 325V37.5" strokeWidth="10" />
      {/* Outer rails — thicker, square caps in the source so leave default */}
      <line x1="330" y1="0" x2="330" y2="362.5" strokeWidth="20" strokeLinecap="butt" />
      <line x1="10" y1="362.5" x2="10" y2="0" strokeWidth="20" strokeLinecap="butt" />
    </svg>
  );
}
