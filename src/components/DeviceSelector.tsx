import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Label } from "@/components/ui/label";
import { useEngineStore } from "@/store/engineStore";
import { a11y } from "@/i18n/a11yStrings";

interface DeviceSelectorProps {
  /** Compact horizontal mode for the header bar */
  compact?: boolean;
}

function formatSampleRate(sr: number): string {
  return sr >= 1000 ? `${(sr / 1000).toFixed(1)} kHz` : `${sr} Hz`;
}

/**
 * Mic input selector.
 *
 * The engine output is hard-routed to the Klaar virtual driver —
 * there is no user-facing output device selector. See
 * `src-tauri/src/commands/driver.rs::KLAAR_DRIVER_UID`.
 */
export function DeviceSelector({ compact = false }: DeviceSelectorProps) {
  const inputDevices = useEngineStore((s) => s.inputDevices);
  const selectedInputUid = useEngineStore((s) => s.selectedInputUid);
  const selectInputDevice = useEngineStore((s) => s.selectInputDevice);

  if (compact) {
    const selectedDevice = inputDevices.find((d) => d.id === selectedInputUid) ?? null;
    return (
      <div className="flex items-center gap-3">
        {/* Input device */}
        <div className="flex items-center gap-1.5 min-w-0 flex-1">
          <span
            className="text-[10px] uppercase tracking-wider shrink-0"
            style={{ color: "var(--color-text-secondary)" }}
          >
            IN
          </span>
          <Select value={selectedInputUid ?? ""} onValueChange={selectInputDevice}>
            <SelectTrigger
              id="input-device"
              aria-label={a11y.inputDeviceSelect()}
              // Chrome (bg / border / text colour) comes from the
              // shared `--color-control-*` vars in the default trigger
              // style. Only sizing and the slightly tighter font size
              // are overridden here.
              className="h-6 text-xs px-2 py-0 flex-1 min-w-0"
              style={{ fontSize: 11 }}
            >
              {/* Custom trigger content — render name and sample rate as
                  separate spans so the name truncates while the sample
                  rate always shows in full. The shadcn SelectTrigger
                  applies [&>span]:line-clamp-1 to direct child spans;
                  wrapping in a div sidesteps that and lets us control
                  truncation per-element. SelectValue is still used as a
                  fallback for the placeholder when nothing is selected. */}
              {selectedDevice ? (
                <div className="flex items-center gap-2 min-w-0 w-full">
                  <span className="truncate text-left flex-1">{selectedDevice.name}</span>
                  <span
                    className="shrink-0 text-[10px] tabular-nums"
                    style={{ color: "var(--color-text-secondary)" }}
                  >
                    {formatSampleRate(selectedDevice.sampleRate)}
                  </span>
                </div>
              ) : (
                <SelectValue placeholder="Select…" />
              )}
            </SelectTrigger>
            <SelectContent>
              {inputDevices.length === 0 ? (
                <SelectItem value="__none__" disabled>
                  No input devices
                </SelectItem>
              ) : (
                inputDevices.map((d) => (
                  <SelectItem key={d.id} value={d.id} className="text-xs">
                    {d.name}
                    <span className="ml-2 text-[10px]" style={{ color: "var(--color-text-secondary)" }}>
                      {formatSampleRate(d.sampleRate)}
                    </span>
                  </SelectItem>
                ))
              )}
            </SelectContent>
          </Select>
        </div>
      </div>
    );
  }

  // Full vertical layout (legacy / standalone use)
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-1.5">
        <Label htmlFor="input-device">Input (Microphone)</Label>
        <Select value={selectedInputUid ?? ""} onValueChange={selectInputDevice}>
          <SelectTrigger id="input-device" className="w-full">
            <SelectValue placeholder="Select input device…" />
          </SelectTrigger>
          <SelectContent>
            {inputDevices.length === 0 ? (
              <SelectItem value="__none__" disabled>
                No input devices found
              </SelectItem>
            ) : (
              inputDevices.map((d) => (
                <SelectItem key={d.id} value={d.id}>
                  {d.name}
                  <span className="ml-2 text-xs text-[--color-text-secondary]">
                    {formatSampleRate(d.sampleRate)}
                  </span>
                </SelectItem>
              ))
            )}
          </SelectContent>
        </Select>
      </div>
    </div>
  );
}
