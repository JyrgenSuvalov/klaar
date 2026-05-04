// WCAG 2.1 contrast helpers.
//
// Used by `contrast.test.ts` to assert token-pair contrast ratios.
// Token values are read from CSS custom properties on `:root` after
// `src/index.css` has been imported into JSDOM.
//
// Reference: https://www.w3.org/WAI/WCAG21/Techniques/general/G18

/** Parse a 3-, 6-, or 8-digit hex colour into [r,g,b] in 0..255 (alpha ignored). */
export function parseHex(hex: string): [number, number, number] {
  const trimmed = hex.trim().replace(/^#/, "");
  if (trimmed.length === 3) {
    const r = parseInt(trimmed[0] + trimmed[0], 16);
    const g = parseInt(trimmed[1] + trimmed[1], 16);
    const b = parseInt(trimmed[2] + trimmed[2], 16);
    return [r, g, b];
  }
  if (trimmed.length === 6 || trimmed.length === 8) {
    const r = parseInt(trimmed.slice(0, 2), 16);
    const g = parseInt(trimmed.slice(2, 4), 16);
    const b = parseInt(trimmed.slice(4, 6), 16);
    return [r, g, b];
  }
  throw new Error(`Cannot parse colour: ${hex}`);
}

/** Parse `rgb(...)` / `rgba(...)` (alpha ignored). */
function parseRgb(str: string): [number, number, number] | null {
  const m = str.match(/rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/);
  if (!m) return null;
  return [parseInt(m[1], 10), parseInt(m[2], 10), parseInt(m[3], 10)];
}

/** Parse hex or rgb() into RGB. */
export function parseColor(value: string): [number, number, number] {
  const v = value.trim();
  if (v.startsWith("#")) return parseHex(v);
  const rgb = parseRgb(v);
  if (rgb) return rgb;
  throw new Error(`Unsupported colour format: ${value}`);
}

/** WCAG relative luminance for a sRGB colour. */
export function relativeLuminance([r, g, b]: [number, number, number]): number {
  const norm = (c: number) => {
    const x = c / 255;
    return x <= 0.03928 ? x / 12.92 : Math.pow((x + 0.055) / 1.055, 2.4);
  };
  return 0.2126 * norm(r) + 0.7152 * norm(g) + 0.0722 * norm(b);
}

/** WCAG contrast ratio between two colours. Always returns ≥1. */
export function contrastRatio(a: string, b: string): number {
  const la = relativeLuminance(parseColor(a));
  const lb = relativeLuminance(parseColor(b));
  const [light, dark] = la > lb ? [la, lb] : [lb, la];
  return (light + 0.05) / (dark + 0.05);
}

/** Read a CSS custom property from `:root` (or any element). */
export function readToken(name: string, el: Element = document.documentElement): string {
  const value = getComputedStyle(el).getPropertyValue(name).trim();
  if (!value) throw new Error(`Token not found: ${name}`);
  return value;
}

export interface ContrastPair {
  /** Foreground token (CSS custom-property name). */
  fg: string;
  /** Background token. */
  bg: string;
  /** Minimum required ratio (4.5 for normal text, 3.0 for large text / non-text UI). */
  min: number;
  /** Human-readable label. */
  label: string;
}
