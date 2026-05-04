import { a11y } from "@/i18n/a11yStrings";

interface BypassToggleProps {
  bypassed: boolean;
  onToggle: (bypassed: boolean) => void;
  label?: string;
}

export function BypassToggle({ bypassed, onToggle, label }: BypassToggleProps) {
  return (
    <button
      type="button"
      onClick={() => onToggle(!bypassed)}
      className="bypass-toggle flex items-center gap-1.5 px-2 py-1 rounded cursor-pointer select-none focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--color-accent)]"
      style={{
        backgroundColor: bypassed
          ? "var(--color-surface)"
          : "color-mix(in srgb, var(--color-accent) 20%, transparent)",
        border: `1px solid ${bypassed ? "var(--color-border)" : "var(--color-accent)"}`,
        color: bypassed ? "var(--color-text-secondary)" : "var(--color-accent)",
        opacity: bypassed ? 0.6 : 1,
      }}
      aria-pressed={!bypassed}
      aria-label={a11y.bypassToggle(label)}
      title={bypassed ? "Bypassed — click to enable" : "Active — click to bypass"}
    >
      {/* Indicator dot */}
      <span
        className="w-1.5 h-1.5 rounded-full"
        style={{
          backgroundColor: bypassed ? "var(--color-text-secondary)" : "var(--color-accent)",
          boxShadow: bypassed ? "none" : "0 0 4px var(--color-accent)",
        }}
      />
      {/* Grid-stack the label so the button reserves the width of the
          longer glyph string ("BYPASS" is wider than "ACTIVE" at 10px
          uppercase tracking-wider). Prevents the panel header from
          shifting when toggling. */}
      <span className="grid text-[10px] font-medium uppercase tracking-wider leading-none">
        <span className="col-start-1 row-start-1 invisible" aria-hidden>
          Bypass
        </span>
        <span className="col-start-1 row-start-1">{bypassed ? "Bypass" : "Active"}</span>
      </span>
    </button>
  );
}
