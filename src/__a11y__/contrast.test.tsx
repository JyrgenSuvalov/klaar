// WCAG 2.1 AA contrast regression gate.
//
// Two layers, each catching a different failure mode:
//
//   1. Token-pair contrast assertions read CSS custom-property values
//      directly off `:root` (mirrored from `src/index.css`'s `@theme` block —
//      JSDOM doesn't run Tailwind v4's `@theme` directive, so we have to
//      mirror the values here). Catches drift in the source tokens.
//
//   2. axe-core scan over a representative DOM fixture exercising the
//      tokens-via-inline-style pattern used throughout the app
//      (`style={{ color: "var(--color-text-primary)" }}`). axe reads the
//      computed style off the rendered DOM, so this catches situations
//      where a component pairs tokens that the static list missed.
//
// The full <App /> shell is *not* rendered for the axe scan — it requires
// ResizeObserver, `invoke()` mocks for the device list, and several other
// JSDOM-incompatible APIs. The fixture covers the same token combinations
// with far less surface area to maintain.

import { describe, it, expect, beforeEach } from "vitest";
import { render } from "@testing-library/react";
import axe from "axe-core";
import "../index.css";
import { contrastRatio, readToken, type ContrastPair } from "./contrast";

// ── Token mirror ────────────────────────────────────────────────────────────
// KEEP IN SYNC with src/index.css's @theme block. Tailwind v4's @theme
// directive compiles to :root declarations during the Vite CSS pipeline;
// JSDOM does not run that pipeline, so we mirror the values manually.
const THEME_TOKENS: Record<string, string> = {
  // Base
  "--color-background": "#0a0a0a",
  "--color-surface": "#141414",
  "--color-surface-elevated": "#1c1c1c",
  "--color-surface-panel": "#181818",
  "--color-border": "#636363",
  "--color-border-subtle": "#222222",
  "--color-text-primary": "#f0f0f0",
  "--color-text-option": "#d8d8d8",
  "--color-text-secondary": "#a8a8a8",
  "--color-accent": "#3b82f6",
  "--color-accent-hover": "#2563eb",
  // Meter
  "--color-meter-green": "#22c55e",
  "--color-meter-yellow": "#eab308",
  "--color-meter-red": "#ef4444",
  "--color-meter-bg": "#0c0c0c",
  // GR
  "--color-gr-fill": "#f59e0b",
  "--color-gr-peak": "#fbbf24",
  // Knob
  "--color-knob-track": "#2e2e2e",
  "--color-knob-fill": "#3b82f6",
  // EQ
  "--color-eq-bg": "#0c0c0c",
  "--color-eq-curve": "#60a5fa",
  "--color-band-1": "#60a5fa",
  "--color-band-2": "#34d399",
  "--color-band-3": "#fbbf24",
  "--color-band-4": "#f87171",
  "--color-band-5": "#a78bfa",
  "--color-band-6": "#fb923c",
  "--color-band-7": "#38bdf8",
  "--color-band-8": "#4ade80",
};

function applyThemeTokens() {
  for (const [k, v] of Object.entries(THEME_TOKENS)) {
    document.documentElement.style.setProperty(k, v);
  }
}

// ── Pair definitions ────────────────────────────────────────────────────────

const NORMAL_TEXT_PAIRS: ContrastPair[] = [
  { fg: "--color-text-primary", bg: "--color-surface", min: 4.5, label: "primary text on surface" },
  { fg: "--color-text-primary", bg: "--color-surface-elevated", min: 4.5, label: "primary text on elevated surface" },
  { fg: "--color-text-primary", bg: "--color-surface-panel", min: 4.5, label: "primary text on panel" },
  { fg: "--color-text-primary", bg: "--color-background", min: 4.5, label: "primary text on background" },
  { fg: "--color-text-secondary", bg: "--color-surface", min: 4.5, label: "secondary text on surface" },
  { fg: "--color-text-secondary", bg: "--color-background", min: 4.5, label: "secondary text on background" },
  { fg: "--color-text-secondary", bg: "--color-surface-panel", min: 4.5, label: "secondary text on panel" },
  { fg: "--color-text-option", bg: "--color-surface", min: 4.5, label: "option text on surface" },
];

const NON_TEXT_PAIRS: ContrastPair[] = [
  { fg: "--color-accent", bg: "--color-surface", min: 3.0, label: "accent (focus ring) on surface" },
  { fg: "--color-accent", bg: "--color-background", min: 3.0, label: "accent on background" },
  { fg: "--color-border", bg: "--color-surface", min: 3.0, label: "border on surface" },
  { fg: "--color-border", bg: "--color-background", min: 3.0, label: "border on background" },
  { fg: "--color-meter-green", bg: "--color-meter-bg", min: 3.0, label: "meter green on meter bg" },
  { fg: "--color-meter-yellow", bg: "--color-meter-bg", min: 3.0, label: "meter yellow on meter bg" },
  { fg: "--color-meter-red", bg: "--color-meter-bg", min: 3.0, label: "meter red on meter bg" },
  { fg: "--color-band-1", bg: "--color-eq-bg", min: 3.0, label: "EQ band 1 on EQ bg" },
  { fg: "--color-band-2", bg: "--color-eq-bg", min: 3.0, label: "EQ band 2 on EQ bg" },
  { fg: "--color-band-3", bg: "--color-eq-bg", min: 3.0, label: "EQ band 3 on EQ bg" },
  { fg: "--color-band-4", bg: "--color-eq-bg", min: 3.0, label: "EQ band 4 on EQ bg" },
  { fg: "--color-band-5", bg: "--color-eq-bg", min: 3.0, label: "EQ band 5 on EQ bg" },
  { fg: "--color-band-6", bg: "--color-eq-bg", min: 3.0, label: "EQ band 6 on EQ bg" },
  { fg: "--color-band-7", bg: "--color-eq-bg", min: 3.0, label: "EQ band 7 on EQ bg" },
  { fg: "--color-band-8", bg: "--color-eq-bg", min: 3.0, label: "EQ band 8 on EQ bg" },
];

function checkPairs(pairs: ContrastPair[]) {
  for (const pair of pairs) {
    const fg = readToken(pair.fg);
    const bg = readToken(pair.bg);
    const ratio = contrastRatio(fg, bg);
    expect(
      ratio,
      `${pair.label}: ${pair.fg} (${fg}) on ${pair.bg} (${bg}) = ${ratio.toFixed(2)}:1, want ≥${pair.min}:1`,
    ).toBeGreaterThanOrEqual(pair.min);
  }
}

// ── Tests ───────────────────────────────────────────────────────────────────

describe("WCAG 2.1 AA contrast — default theme tokens", () => {
  beforeEach(() => {
    applyThemeTokens();
  });

  it("normal-text token pairs reach 4.5:1", () => {
    checkPairs(NORMAL_TEXT_PAIRS);
  });

  it("non-text UI token pairs reach 3:1", () => {
    checkPairs(NON_TEXT_PAIRS);
  });
});

describe("axe-core colour-contrast — representative fixture", () => {
  beforeEach(() => {
    applyThemeTokens();
  });

  it("renders a fixture with no contrast violations", async () => {
    const t = THEME_TOKENS;
    const { container } = render(
      <div style={{ backgroundColor: t["--color-background"], color: t["--color-text-primary"] }}>
        <div style={{ backgroundColor: t["--color-surface"], color: t["--color-text-primary"] }}>
          Primary on surface
        </div>
        <div style={{ backgroundColor: t["--color-surface"], color: t["--color-text-secondary"] }}>
          Secondary on surface
        </div>
        <div style={{ backgroundColor: t["--color-surface"], color: t["--color-text-option"] }}>
          Option on surface
        </div>
        <div style={{ backgroundColor: t["--color-surface-elevated"], color: t["--color-text-primary"] }}>
          Primary on elevated
        </div>
        <div style={{ backgroundColor: t["--color-surface-panel"], color: t["--color-text-primary"] }}>
          Primary on panel
        </div>
        <div style={{ backgroundColor: t["--color-surface-panel"], color: t["--color-text-secondary"] }}>
          Secondary on panel
        </div>
        <div style={{ backgroundColor: t["--color-background"], color: t["--color-text-secondary"] }}>
          Secondary on background
        </div>
        <button
          style={{
            backgroundColor: t["--color-surface"],
            color: t["--color-text-primary"],
            border: `1px solid ${t["--color-border"]}`,
          }}
        >
          Button
        </button>
        <input
          type="text"
          defaultValue="profile name"
          aria-label="Profile name"
          style={{
            backgroundColor: t["--color-surface-elevated"],
            color: t["--color-text-primary"],
            border: `1px solid ${t["--color-border"]}`,
          }}
        />
      </div>,
    );

    const results = await axe.run(container, {
      runOnly: { type: "rule", values: ["color-contrast"] },
    });

    if (results.violations.length > 0) {
      const summary = results.violations
        .map(
          (v) =>
            `- ${v.id}: ${v.description}\n  nodes:\n${v.nodes
              .map((n) => `    ${n.target.join(" ")} — ${n.failureSummary ?? ""}`)
              .join("\n")}`,
        )
        .join("\n");
      throw new Error(`axe-core contrast violations:\n${summary}`);
    }
    expect(results.violations).toEqual([]);
  });
});

// ── High-contrast variant (prefers-contrast: more) ──────────────────────────

describe("WCAG 2.1 AA contrast — high-contrast variant", () => {
  beforeEach(() => {
    // Mirror the @media (prefers-contrast: more) overrides on :root.
    // KEEP IN SYNC with src/index.css's media block.
    applyThemeTokens();
    document.documentElement.style.setProperty("--color-text-primary", "#ffffff");
    document.documentElement.style.setProperty("--color-text-secondary", "#d0d0d0");
    document.documentElement.style.setProperty("--color-text-option", "#ffffff");
    document.documentElement.style.setProperty("--color-border", "#ffffff");
    document.documentElement.style.setProperty("--color-border-subtle", "#909090");
    document.documentElement.style.setProperty("--color-accent", "#00f0ff");
  });

  it("normal-text pairs still pass under high contrast", () => {
    checkPairs(NORMAL_TEXT_PAIRS);
  });

  it("non-text UI pairs still pass under high contrast", () => {
    checkPairs(NON_TEXT_PAIRS);
  });
});
