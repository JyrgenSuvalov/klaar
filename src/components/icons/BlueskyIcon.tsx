/**
 * Pixel-art Bluesky glyph.
 *
 * Inlined from the user-supplied `pixel--bluesky.svg`. Renders monochrome
 * via `currentColor` so callers can recolour with a Tailwind text utility.
 */
type Props = { className?: string };

export function BlueskyIcon({ className }: Props) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 24 24"
      className={className}
      aria-hidden="true"
      focusable="false"
    >
      <path
        fill="currentColor"
        d="M23 3v8h-1v2h-2v1h-2v1h2v1h1v3h-1v1h-1v1h-2v1h-2v-1h-1v-1h-1v-2h-2v2h-1v1H9v1H7v-1H5v-1H4v-1H3v-3h1v-1h2v-1H4v-1H2v-2H1V3h1V2h2v1h2v1h1v1h1v1h1v1h1v2h1v1h2V9h1V7h1V6h1V5h1V4h1V3h2V2h2v1z"
      />
    </svg>
  );
}
