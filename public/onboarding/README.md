# Onboarding screenshots

Post-install conferencing-apps screen (Phase 4 task 6.1). Expected files:

- `zoom.png`      — Zoom → Settings → Audio → Microphone, with "Klaar" selected
- `meet.png`      — Google Meet → Settings ⚙ → Audio → Microphone
- `facetime.png`  — FaceTime → Video menu → Microphone
- `slack.png`     — Slack huddle → microphone picker

Capture at 2× (Retina), crop to ~4:3, export as PNG or JPEG at reasonable
compression (target under 200 KB each). Files live in `public/onboarding/`
because the `ConfigureConferencingAppsScreen` component references them via
Vite's `/onboarding/<name>.png` static-asset path. Missing files render as
a blank tile with the caption visible — intentional graceful fallback.
