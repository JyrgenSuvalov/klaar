import { useEffect, useState } from "react";
import { useEngineStore } from "@/store/engineStore";

/**
 * Hidden-but-visible-to-AT live region that announces clipping events.
 *
 * Watches `engineStore.clipEvents` (a pair of counters incremented by the
 * meter polling loop on rising-edge transitions). When a counter increments
 * we briefly populate the alert text — VoiceOver / NVDA / JAWS announce
 * the new content — then clear it on the next render so the same string
 * is treated as a new alert if the counter increments again.
 *
 * Visual feedback for clipping is handled by the meters themselves; this
 * component is intentionally `sr-only`.
 */
export function ClipAlert() {
  const inputClipCount = useEngineStore((s) => s.clipEvents.inputClipCount);
  const outputClipCount = useEngineStore((s) => s.clipEvents.outputClipCount);

  // Render-time rising-edge detection: compare current store counters to
  // the last pair we've already announced. When they differ, we synthesize
  // the alert text and update both pieces of state in the same render.
  // This is the "store previous props" pattern from the React docs — it
  // replaces an effect that synchronously called setState.
  const [seen, setSeen] = useState({ input: inputClipCount, output: outputClipCount });
  const [message, setMessage] = useState("");

  if (inputClipCount !== seen.input || outputClipCount !== seen.output) {
    let next = "";
    if (inputClipCount > seen.input && outputClipCount > seen.output) {
      next = "Input and output clipping";
    } else if (inputClipCount > seen.input) {
      next = "Input clipping";
    } else if (outputClipCount > seen.output) {
      next = "Output clipping";
    }
    setSeen({ input: inputClipCount, output: outputClipCount });
    if (next) setMessage(next);
  }

  // Clear the message after a short delay so the same wording can re-fire
  // on a subsequent transition (assistive tech only re-announces when the
  // text content actually changes).
  useEffect(() => {
    if (!message) return;
    const t = window.setTimeout(() => setMessage(""), 500);
    return () => window.clearTimeout(t);
  }, [message]);

  return (
    <div role="alert" className="sr-only" data-testid="clip-alert">
      {message}
    </div>
  );
}
