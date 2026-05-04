import { Power } from "lucide-react";
import { Knob } from "@/components/controls/Knob";
import { BAND_COLORS } from "@/components/eq/EqCurveDisplay";
import { FilterTypeIcon } from "@/components/eq/FilterTypeIcon";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select";
import type { EqBand, FilterType } from "@/store/dspStore";
import { a11y } from "@/i18n/a11yStrings";

// Filter type catalogue. The `label` is shown in the dropdown rows next to
// the glyph and used as the trigger's accessible name; the trigger itself
// renders only the icon to keep the band column compact (Pro-Q / EQ Eight
// convention).
const FILTER_TYPES: { value: FilterType; label: string }[] = [
  { value: "bell", label: "Bell" },
  { value: "highPass", label: "High Pass" },
  { value: "highPass48", label: "High Pass ×4" },
  { value: "lowPass", label: "Low Pass" },
  { value: "lowPass48", label: "Low Pass ×4" },
  { value: "highShelf", label: "High Shelf" },
  { value: "lowShelf", label: "Low Shelf" },
];

interface Props {
  index: number;
  band: EqBand;
  onChange: (partial: Partial<EqBand>) => void;
  eqBypassed: boolean;
}

export function EqBandControls({ index, band, onChange, eqBypassed }: Props) {
  const color = BAND_COLORS[index];
  const isPass = band.filterType === "highPass" || band.filterType === "lowPass" || band.filterType === "highPass48" || band.filterType === "lowPass48";
  const disabled = eqBypassed || !band.enabled;

  return (
    <div
      className="eq-band-controls relative"
      style={{ opacity: !eqBypassed && !band.enabled ? 0.35 : 1 }}
    >
      {/* Enable toggle — pinned to the top-right corner of the panel.
          Power-button affordance (FabFilter / Pro-Q convention): glyph is
          coloured with the band accent when on, dim when off. */}
      <button
        type="button"
        className="band-enable-toggle absolute top-0.5 right-0.5 flex items-center justify-center rounded transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--color-accent)]"
        style={{
          width: 16,
          height: 16,
          color: band.enabled ? color : "var(--color-text-secondary)",
          opacity: band.enabled ? 1 : 0.55,
          cursor: "pointer",
          padding: 0,
          background: "transparent",
          border: "none",
        }}
        onClick={() => onChange({ enabled: !band.enabled })}
        role="switch"
        aria-checked={band.enabled}
        aria-label={a11y.eqBandSwitch(index + 1)}
        title={band.enabled ? "Click to disable" : "Click to enable"}
      >
        <Power size={11} strokeWidth={2.5} aria-hidden="true" />
      </button>

      {/* Band number — centered above the knob column. */}
      <div className="flex items-center justify-center mb-1">
        <div
          className="w-4 h-4 rounded-full text-[8px] font-bold flex items-center justify-center"
          style={{ backgroundColor: `${color}33`, border: `1px solid ${color}`, color }}
        >
          {index + 1}
        </div>
      </div>

      {/* Filter type selector — shadcn Select so we can render the SVG
          glyphs from /public/filters/. The trigger shows only the icon
          (tinted with the band accent); the dropdown rows show icon + name. */}
      <Select
        value={band.filterType}
        onValueChange={(v) => onChange({ filterType: v as FilterType })}
        disabled={eqBypassed}
      >
        <SelectTrigger
          aria-label={a11y.filterTypeSelect()}
          className="relative h-5 w-full px-1 py-0 my-1.5 [&>span]:flex [&>span]:items-center [&>span]:justify-center [&>span]:w-full [&>svg]:absolute [&>svg]:right-1 [&>svg]:h-3 [&>svg]:w-3 [&>svg]:opacity-60"
          style={{
            // Chrome (bg/border) comes from the shared
            // `--color-control-*` vars in the default trigger style;
            // only the chevron colour, tighter radius, and bypass
            // cursor are local concerns. The band accent is applied
            // exclusively to the inner icon span below.
            color: "var(--color-text-secondary)",
            borderRadius: 3,
            cursor: eqBypassed ? "default" : "pointer",
          }}
        >
          <span style={{ color: band.enabled ? color : "var(--color-text-secondary)" }}>
            <FilterTypeIcon type={band.filterType} width={14} />
          </span>
        </SelectTrigger>
        <SelectContent>
          {FILTER_TYPES.map((ft) => (
            <SelectItem key={ft.value} value={ft.value} className="text-xs">
              <span className="flex items-center gap-2">
                <FilterTypeIcon type={ft.value} width={14} />
                <span>{ft.label}</span>
              </span>
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      {/* Knobs: Frequency, Gain (disabled for pass), Q */}
      <div className="flex flex-col items-center gap-2">
        <Knob
          label="Freq"
          value={band.frequency}
          min={20}
          max={20000}
          defaultValue={1000}
          unit="Hz"
          size={36}
          scale="log"
          onChange={(v) => onChange({ frequency: Math.round(v) })}
          disabled={disabled}
        />
        <Knob
          label="Gain"
          value={band.gain}
          min={-24}
          max={24}
          defaultValue={0}
          unit="dB"
          size={36}
          decimals={2}
          onChange={(v) => onChange({ gain: parseFloat(v.toFixed(2)) })}
          disabled={disabled || isPass}
        />
        <Knob
          label="Q"
          value={band.q}
          min={0.1}
          max={18}
          defaultValue={1.0}
          size={36}
          decimals={2}
          onChange={(v) => onChange({ q: parseFloat(v.toFixed(2)) })}
          disabled={disabled}
        />
      </div>
    </div>
  );
}
