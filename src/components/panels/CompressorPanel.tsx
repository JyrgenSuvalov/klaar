import { useDspStore } from "@/store/dspStore";
import { useEngineStore } from "@/store/engineStore";
import { Knob } from "@/components/controls/Knob";
import { GainReductionMeter } from "@/components/controls/GainReductionMeter";
import { BypassToggle } from "@/components/controls/BypassToggle";
import { a11y } from "@/i18n/a11yStrings";

export function CompressorPanel() {
  const comp = useDspStore((s) => s.compressor);
  const setParam = useDspStore((s) => s.setParam);
  const setBypass = useDspStore((s) => s.setBypass);
  const compReduction = useEngineStore((s) => s.smoothedMeters.compressorReduction);
  const peakCompReduction = useEngineStore((s) => s.peakMeters.compressorReduction);

  return (
    <div
      className={`effect-panel${comp.bypassed ? " bypassed" : ""}`}
      role="region"
      aria-label={a11y.panel.compressor()}
    >
      <div className="panel-header">
        <span className="panel-title">Compressor</span>
        <BypassToggle
          bypassed={comp.bypassed}
          onToggle={(v) => setBypass("compressor", v)}
          label="Compressor"
        />
      </div>

      <div className="panel-controls">
        <div className="knobs-row">
          <Knob
            label="Threshold"
            value={comp.threshold}
            min={-60}
            max={0}
            defaultValue={-18}
            unit="dB"
            onChange={(v) => setParam("compressor", "threshold", v)}
            disabled={comp.bypassed}
          />
          <Knob
            label="Ratio"
            value={comp.ratio}
            min={1}
            max={20}
            defaultValue={4}
            unit=":1"
            onChange={(v) => setParam("compressor", "ratio", v)}
            disabled={comp.bypassed}
          />
          <Knob
            label="Attack"
            value={comp.attack}
            min={0.1}
            max={100}
            defaultValue={5}
            unit="ms"
            onChange={(v) => setParam("compressor", "attack", v)}
            disabled={comp.bypassed}
          />
          <Knob
            label="Release"
            value={comp.release}
            min={1}
            max={1000}
            defaultValue={100}
            unit="ms"
            onChange={(v) => setParam("compressor", "release", v)}
            disabled={comp.bypassed}
          />
          <Knob
            label="Makeup"
            value={comp.makeupGain}
            min={0}
            max={30}
            defaultValue={0}
            unit="dB"
            onChange={(v) => setParam("compressor", "makeupGain", v)}
            disabled={comp.bypassed}
          />
        </div>

        <div className="gr-meter-container">
          <GainReductionMeter
            reduction={compReduction}
            peakReduction={peakCompReduction}
            maxReduction={30}
            height={60}
            ariaLabel="Compressor gain reduction"
          />
        </div>
      </div>
    </div>
  );
}
