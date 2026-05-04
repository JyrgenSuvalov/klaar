import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { useEngineStore } from "@/store/engineStore";
import { a11y } from "@/i18n/a11yStrings";

interface ProcessingToggleProps {
  /** Compact horizontal mode for the header bar */
  compact?: boolean;
}

export function ProcessingToggle({ compact = false }: ProcessingToggleProps) {
  const processingEnabled = useEngineStore((s) => s.processingEnabled);
  const setProcessingEnabled = useEngineStore((s) => s.setProcessingEnabled);
  const isStreaming = useEngineStore((s) => s.isStreaming);

  const statusDot = (
    <span
      className={[
        "inline-block w-2 h-2 rounded-full transition-colors shrink-0",
        !isStreaming
          ? "bg-[--color-meter-red]"
          : processingEnabled
            ? "bg-[--color-meter-green]"
            : "bg-[--color-meter-yellow]",
      ].join(" ")}
      aria-hidden
    />
  );

  if (compact) {
    return (
      <div className="flex items-center gap-2 shrink-0">
        {statusDot}
        {/* Grid-stack the label so the container always reserves the
            width of the longer glyph string ("BYPASS" is wider than
            "ACTIVE" at 10px uppercase tracking-wider). Prevents the
            divider/switch from shifting when toggling. */}
        <span
          className="grid text-[10px] uppercase tracking-wider"
          style={{ color: "var(--color-text-secondary)" }}
        >
          <span className="col-start-1 row-start-1 invisible" aria-hidden>
            Bypass
          </span>
          <span className="col-start-1 row-start-1">
            {processingEnabled ? "Active" : "Bypass"}
          </span>
        </span>
        <Switch
          id="processing-toggle-compact"
          checked={processingEnabled}
          onCheckedChange={setProcessingEnabled}
          disabled={!isStreaming}
          aria-label={a11y.processingToggle()}
        />
      </div>
    );
  }

  return (
    <div className="flex items-center justify-between">
      <div className="flex flex-col gap-0.5">
        <Label htmlFor="processing-toggle" className="text-sm font-medium text-[--color-text-primary]">
          Processing
        </Label>
        <span className="text-xs text-[--color-text-secondary]">
          {processingEnabled ? "DSP chain active" : "Dry passthrough"}
        </span>
      </div>

      <div className="flex items-center gap-2">
        {statusDot}
        <Switch
          id="processing-toggle"
          checked={processingEnabled}
          onCheckedChange={setProcessingEnabled}
          disabled={!isStreaming}
        />
      </div>
    </div>
  );
}
