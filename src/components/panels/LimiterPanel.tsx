import { useDspStore } from "@/store/dspStore";
import { useEngineStore } from "@/store/engineStore";
import { Knob } from "@/components/controls/Knob";
import { GainReductionMeter } from "@/components/controls/GainReductionMeter";
import { BypassToggle } from "@/components/controls/BypassToggle";
import { a11y } from "@/i18n/a11yStrings";

export function LimiterPanel() {
  const lim = useDspStore((s) => s.limiter);
  const setParam = useDspStore((s) => s.setParam);
  const setBypass = useDspStore((s) => s.setBypass);
  const limReduction = useEngineStore((s) => s.smoothedMeters.limiterReduction);
  const peakLimReduction = useEngineStore((s) => s.peakMeters.limiterReduction);

  return (
    <div
      className={`effect-panel${lim.bypassed ? " bypassed" : ""}`}
      role="region"
      aria-label={a11y.panel.limiter()}
    >
      <div className="panel-header">
        <span className="panel-title">Limiter</span>
        <BypassToggle
          bypassed={lim.bypassed}
          onToggle={(v) => setBypass("limiter", v)}
          label="Limiter"
        />
      </div>

      <div className="panel-controls">
        <div className="knobs-row">
          <Knob
            label="Ceiling"
            value={lim.ceiling}
            min={-20}
            max={0}
            defaultValue={-1}
            unit="dBFS"
            onChange={(v) => setParam("limiter", "ceiling", v)}
            disabled={lim.bypassed}
          />
          <Knob
            label="Release"
            value={lim.release}
            min={1}
            max={500}
            defaultValue={50}
            unit="ms"
            onChange={(v) => setParam("limiter", "release", v)}
            disabled={lim.bypassed}
          />
        </div>

        <div className="gr-meter-container">
          <GainReductionMeter
            reduction={limReduction}
            peakReduction={peakLimReduction}
            maxReduction={20}
            height={60}
            ariaLabel="Limiter gain reduction"
          />
        </div>
      </div>
    </div>
  );
}
