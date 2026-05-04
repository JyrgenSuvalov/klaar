/**
 * Pixel-art Mastodon glyph.
 *
 * Inlined from the user-supplied `pixel--mastodon.svg`. Renders monochrome
 * via `currentColor` so callers can recolour with a Tailwind text utility.
 */
type Props = { className?: string };

export function MastodonIcon({ className }: Props) {
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
        d="M22 7V4h-1V3h-1V2h-1V1H5v1H4v1H3v1H2v3H1v9h1v3h1v1h1v1h1v1h2v1h7v-1h2v-2H9v-1H8v-2h1v1h9v-1h2v-1h1v-1h1v-2h1V7zm-3 7h-3V7h-2v1h-1v4h-2V8h-1V7H8v7H5V6h1V5h1V4h3v1h1v1h2V5h1V4h3v1h1v1h1z"
      />
    </svg>
  );
}
