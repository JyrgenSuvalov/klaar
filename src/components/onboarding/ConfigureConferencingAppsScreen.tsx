import { ModalShell, ModalHeader, ModalBody, ModalFooter, ModalButton } from "./ModalShell";

interface Props {
  onDone: () => void;
}

/**
 * Post-install screen (amendment A7). Four screenshots showing the mic
 * picker in Zoom / Meet / FaceTime / Slack with "Klaar"
 * selected, each with a one-line caption.
 *
 * The screenshot files live in `src/assets/onboarding/` and are committed
 * alongside the source. If they are missing at build time the grid renders
 * the captions only — we don't want a broken image to block onboarding.
 */
const APPS = [
  {
    name: "Zoom",
    caption: "Zoom → Settings → Audio → Microphone",
    // Screenshots are captured in task 6.1. Paths below are the committed
    // locations; if the file is absent the <img> will fail gracefully.
    src: "/onboarding/zoom.webp",
  },
  {
    name: "Google Meet",
    caption: "Meet → Settings ⚙ → Audio → Microphone",
    src: "/onboarding/meet.webp",
  },
  {
    name: "FaceTime",
    caption: "FaceTime → Video → Microphone",
    src: "/onboarding/facetime.webp",
  },
  {
    name: "Slack",
    caption: "Slack huddle → Microphone → Klaar (Virtual)",
    src: "/onboarding/slack.webp",
  },
];

export function ConfigureConferencingAppsScreen({ onDone }: Props) {
  return (
    <ModalShell>
      <ModalHeader>One last step — tell your apps to listen to Klaar</ModalHeader>
      <ModalBody>
        <p>
          Open each app you use and set the microphone to{" "}
          <strong style={{ color: "var(--color-text-primary)" }}>Klaar</strong>.
          You only need to do this once per app.
        </p>

        <div className="mt-4 grid grid-cols-2 gap-3">
          {APPS.map((app) => (
            <div
              key={app.name}
              className="rounded-md border p-2"
              style={{ borderColor: "var(--color-border)" }}
            >
              <div
                className="rounded aspect-[4/3] w-full mb-1.5 flex items-center justify-center overflow-hidden"
                style={{ backgroundColor: "var(--color-background)" }}
              >
                <img
                  src={app.src}
                  alt={`${app.name} microphone setting`}
                  className="max-w-full max-h-full object-contain"
                  onError={(e) => {
                    // Hide the broken-image icon if the screenshot hasn't
                    // been captured yet (task 6.1).
                    (e.target as HTMLImageElement).style.display = "none";
                  }}
                />
              </div>
              <div className="text-xs font-medium" style={{ color: "var(--color-text-primary)" }}>
                {app.name}
              </div>
              <div className="text-[10px]" style={{ color: "var(--color-text-secondary)" }}>
                {app.caption}
              </div>
            </div>
          ))}
        </div>
      </ModalBody>
      <ModalFooter>
        <ModalButton onClick={onDone}>Done</ModalButton>
      </ModalFooter>
    </ModalShell>
  );
}
