import type { FilterType } from "@/store/dspStore";

// Map FilterType → SVG filename in /public/filters/.
// We render the SVG via CSS mask-image so we can tint it with the current
// text colour (band accent) — Pro-Q / EQ Eight convention. The SVGs
// themselves are stroked white but the mask discards their colour.
const FILTER_ICON_FILE: Record<FilterType, string> = {
  bell: "Bell.svg",
  highPass: "HP.svg",
  highPass48: "HP4x.svg",
  lowPass: "LP.svg",
  lowPass48: "LP4x.svg",
  highShelf: "HS.svg",
  lowShelf: "LS.svg",
};

interface Props {
  type: FilterType;
  /** Pixel width of the icon. Height tracks 8/12 of width to match aspect. */
  width?: number;
  className?: string;
}

/**
 * Inline icon for an EQ filter type. Inherits colour from the parent's
 * `color` (via `currentColor` in the mask). Width defaults to 14 px which
 * fits comfortably inside the 36 px-wide band column.
 */
export function FilterTypeIcon({ type, width = 14, className }: Props) {
  const url = `/filters/${FILTER_ICON_FILE[type]}`;
  // The Bell / HS / LS files are 12×7; HP / LP / *4x files are 12×8.
  // Use a 12×8 box for all of them — the 7-tall ones will sit centred.
  const height = Math.round((width * 8) / 12);
  return (
    <span
      aria-hidden="true"
      className={className}
      style={{
        display: "inline-block",
        width,
        height,
        backgroundColor: "currentColor",
        WebkitMaskImage: `url(${url})`,
        maskImage: `url(${url})`,
        WebkitMaskRepeat: "no-repeat",
        maskRepeat: "no-repeat",
        WebkitMaskPosition: "center",
        maskPosition: "center",
        WebkitMaskSize: "contain",
        maskSize: "contain",
        flexShrink: 0,
      }}
    />
  );
}
