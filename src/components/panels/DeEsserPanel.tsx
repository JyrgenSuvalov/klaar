import { useDspStore } from "@/store/dspStore";
import { useEngineStore } from "@/store/engineStore";
import { Knob } from "@/components/controls/Knob";
import { Meter } from "@/components/controls/Meter";
import { GainReductionMeter } from "@/components/controls/GainReductionMeter";
import { BypassToggle } from "@/components/controls/BypassToggle";
import { a11y } from "@/i18n/a11yStrings";

// Display floor for the sidechain meter. The backend publishes
// `deEsserSidechainDb` clamped to [-96, 0], but useful detector readings on
// natural speech (post Q=6 bandpass + envelope-follower magnitude) land in
// the -10 to -50 dBFS range. Pinning the meter floor to the threshold
// knob's range (-40 dB) plus a small headroom gives the bar usable visual
// resolution and keeps the threshold tick in the upper 2/3 of the meter
// where the user can actually move it. Values below this floor are
// graphically clamped (still legitimate for ARIA / debugging).
const SIDECHAIN_FLOOR_DB = -60;

export function DeEsserPanel() {
  const deEsser = useDspStore((s) => s.deEsser);
  const setParam = useDspStore((s) => s.setParam);
  const setBypass = useDspStore((s) => s.setBypass);
  const sidechainLevel = useEngineStore((s) => s.smoothedMeters.deEsserSidechainDb);
  const peakSidechain = useEngineStore((s) => s.peakMeters.deEsserSidechainDb);
  const deEsserReduction = useEngineStore((s) => s.smoothedMeters.deEsserReduction);
  const peakDeEsserReduction = useEngineStore((s) => s.peakMeters.deEsserReduction);

  return (
    <div
      className={`effect-panel${deEsser.bypassed ? " bypassed" : ""}`}
      role="region"
      aria-label={a11y.panel.deEsser()}
    >
      <div className="panel-header">
        <span className="panel-title">De-esser</span>
        <BypassToggle
          bypassed={deEsser.bypassed}
          onToggle={(v) => setBypass("deEsser", v)}
          label="De-esser"
        />
      </div>

      <div className="panel-controls">
        <div className="knobs-row">
          <Knob
            label="Freq"
            value={deEsser.frequency}
            min={2000}
            max={12000}
            defaultValue={6000}
            unit="Hz"
            scale="log"
            onChange={(v) => setParam("deEsser", "frequency", v)}
            disabled={deEsser.bypassed}
          />
          <Knob
            label="Threshold"
            value={deEsser.threshold}
            min={-40}
            max={0}
            defaultValue={-20}
            unit="dB"
            onChange={(v) => setParam("deEsser", "threshold", v)}
            disabled={deEsser.bypassed}
          />
          {/* Sidechain meter beside Threshold so users can dial threshold
              against the live sibilance envelope. Tick at the current
              threshold value; clamped to the meter's display range. */}
          <Meter
            level={sidechainLevel}
            peakLevel={peakSidechain}
            label="SC"
            floor={SIDECHAIN_FLOOR_DB}
            height={60}
            tickDb={Math.max(SIDECHAIN_FLOOR_DB, Math.min(0, deEsser.threshold))}
            ariaLabel="De-esser sidechain level"
          />
          <Knob
            label="Ratio"
            value={deEsser.ratio}
            min={1.0}
            max={10.0}
            defaultValue={4.0}
            step={0.1}
            decimals={1}
            unit=":1"
            onChange={(v) => setParam("deEsser", "ratio", v)}
            disabled={deEsser.bypassed}
          />
          <Knob
            label="Attack"
            value={deEsser.attack}
            min={0.1}
            max={10}
            defaultValue={1.0}
            scale="log"
            decimals={1}
            unit="ms"
            onChange={(v) => setParam("deEsser", "attack", v)}
            disabled={deEsser.bypassed}
          />
          <Knob
            label="Release"
            value={deEsser.release}
            min={10}
            max={500}
            defaultValue={50}
            scale="log"
            unit="ms"
            onChange={(v) => setParam("deEsser", "release", v)}
            disabled={deEsser.bypassed}
          />
        </div>

        <div className="gr-meter-container">
          <GainReductionMeter
            reduction={deEsserReduction}
            peakReduction={peakDeEsserReduction}
            maxReduction={12}
            height={60}
            showNumericReadout
            ariaLabel="De-esser gain reduction"
          />
        </div>
      </div>
    </div>
  );
}
