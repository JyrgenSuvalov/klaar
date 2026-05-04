import { useDspStore } from "@/store/dspStore";
import { useEngineStore } from "@/store/engineStore";
import { Knob } from "@/components/controls/Knob";
import { GainReductionMeter } from "@/components/controls/GainReductionMeter";
import { BypassToggle } from "@/components/controls/BypassToggle";
import { a11y } from "@/i18n/a11yStrings";

export function NoiseGatePanel() {
  const gate = useDspStore((s) => s.noiseGate);
  const setParam = useDspStore((s) => s.setParam);
  const setBypass = useDspStore((s) => s.setBypass);
  const gateReduction = useEngineStore((s) => s.smoothedMeters.gateReduction);
  const peakGateReduction = useEngineStore((s) => s.peakMeters.gateReduction);

  return (
    <div
      className={`effect-panel${gate.bypassed ? " bypassed" : ""}`}
      role="region"
      aria-label={a11y.panel.noiseGate()}
    >
      {/* Panel header */}
      <div className="panel-header">
        <span className="panel-title">Noise Gate</span>
        <BypassToggle
          bypassed={gate.bypassed}
          onToggle={(v) => setBypass("noiseGate", v)}
          label="Noise Gate"
        />
      </div>

      {/* Controls row */}
      <div className="panel-controls">
        <div className="knobs-row">
          <Knob
            label="Threshold"
            value={gate.threshold}
            min={-80}
            max={0}
            defaultValue={-40}
            unit="dB"
            onChange={(v) => setParam("noiseGate", "threshold", v)}
            disabled={gate.bypassed}
          />
          <Knob
            label="Attack"
            value={gate.attack}
            min={0.1}
            max={100}
            defaultValue={10}
            unit="ms"
            onChange={(v) => setParam("noiseGate", "attack", v)}
            disabled={gate.bypassed}
          />
          <Knob
            label="Release"
            value={gate.release}
            min={1}
            max={1000}
            defaultValue={100}
            unit="ms"
            onChange={(v) => setParam("noiseGate", "release", v)}
            disabled={gate.bypassed}
          />
          <Knob
            label="Range"
            value={gate.range}
            min={-80}
            max={0}
            defaultValue={-80}
            unit="dB"
            onChange={(v) => setParam("noiseGate", "range", v)}
            disabled={gate.bypassed}
          />
        </div>

        {/* Gain reduction meter */}
        <div className="gr-meter-container">
          <GainReductionMeter
            reduction={gateReduction}
            peakReduction={peakGateReduction}
            maxReduction={40}
            height={60}
            ariaLabel="Noise gate gain reduction"
          />
        </div>
      </div>
    </div>
  );
}
