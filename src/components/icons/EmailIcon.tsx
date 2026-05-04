/**
 * Pixel-art email glyph.
 *
 * Inlined from the user-supplied `memory--email.svg`. The native viewBox is
 * 22×22 (matches the source); callers size with Tailwind classes — `h-6 w-6`
 * for the 24px icon-button affordance is the typical use.
 */
type Props = { className?: string };

export function EmailIcon({ className }: Props) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 22 22"
      className={className}
      aria-hidden="true"
      focusable="false"
    >
      <path
        fill="currentColor"
        d="M1 5h1V4h18v1h1v13h-1v1H2v-1H1zm2 12h16V9h-1v1h-2v1h-2v1h-2v1h-2v-1H8v-1H6v-1H4V9H3zM19 6H3v1h2v1h2v1h2v1h4V9h2V8h2V7h2z"
      />
    </svg>
  );
}
