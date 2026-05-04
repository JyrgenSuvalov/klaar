# Installation

Download the latest DMG from the
[Releases page](https://github.com/JyrgenSuvalov/klaar/releases),
mount it, and drag `Klaar.app` to `/Applications`. On first launch, Klaar
walks you through installing its audio driver behind a single macOS admin
prompt — no Terminal required.

> **A Mac restart is required after install.** Once the driver bundle is on
> disk, macOS only loads it into a fresh `coreaudiod` instance, which means
> a reboot. Klaar surfaces a **Reboot Required** dialog with a one-click
> **Reboot Now** button. You can choose **Later** and reboot on your own
> schedule — until you do, the tray icon shows an orange overlay and the
> audio engine stays paused. Total time from first launch to your first
> Zoom call: ~5 minutes including the reboot.

After installation, select **Klaar** as your microphone in Zoom, Google
Meet, FaceTime, Slack, Teams, Discord, or any other app.

## First launch — getting past Gatekeeper

Klaar is distributed with an ad-hoc signature rather than an Apple Developer
ID, so macOS Gatekeeper will block the first launch. The exact override
flow depends on your macOS version:

- **macOS 14 (Sonoma) and earlier** — right-click `Klaar.app` in
  `/Applications` and choose **Open**, then confirm **Open** in the warning
  dialog.
- **macOS 15 (Sequoia) and later** — double-click the app once (it will be
  blocked), then open **System Settings → Privacy & Security**, scroll to
  the bottom, and click **Open Anyway** next to the Klaar entry. Confirm
  with your Mac password.

You only do this once per install. Subsequent launches open normally.

Prefer the command line?

```bash
xattr -dr com.apple.quarantine /Applications/Klaar.app
```

strips the quarantine bit and skips the Gatekeeper dance entirely.

## Uninstalling the driver

The DMG includes `uninstall.command` at its root. Mount the DMG,
double-click the script, and enter your Mac password when prompted. The
driver is removed and `coreaudiod` is restarted.

## Updates

Klaar checks GitHub once a day for new releases. When one is available, a
dismissible banner appears in the main UI linking to the release page —
there's no auto-installer, you just re-download the DMG.
