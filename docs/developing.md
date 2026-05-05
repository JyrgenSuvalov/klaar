# Klaar — Developer Notes

Operational notes for building, packaging, and shipping Klaar.

## Driver build

The Klaar Virtual Audio Driver is a separate Rust crate in `driver/`
that compiles to a CoreAudio HAL plug-in bundle (`Klaar.driver`).

### Dev build

For day-to-day development, `pnpm dev:app` chains the three steps:

```bash
pnpm dev:app
# expands to:
#   cargo build --release --manifest-path driver/Cargo.toml
#   bash scripts/stage-driver.sh   # ad-hoc signs + copies into resources/
#   pnpm tauri dev
```

Run the steps individually if you want to iterate on just one — e.g.
re-stage without rebuilding (`pnpm stage-driver`), or skip the driver
rebuild entirely when working on frontend code (`pnpm tauri dev`).

`stage-driver.sh` ad-hoc signs the bundle as a mandatory step. macOS refuses
to load unsigned HAL plug-ins on modern releases, so skipping signing means
`install_driver` will silently fail to enumerate the device after
`coreaudiod` relaunch.

### Release build — Universal Binary

Set `KLAAR_DRIVER_UNIVERSAL=1` when running the driver build script to
produce a fat binary that runs on both Intel and Apple Silicon:

```bash
# Prerequisites: rustup target add x86_64-apple-darwin aarch64-apple-darwin
KLAAR_DRIVER_UNIVERSAL=1 scripts/build-driver.sh
```

The script builds both arch slices, combines them with `lipo`, and runs
`codesign -s - --force --deep` on the final `.driver`. Verify:

```bash
lipo -info driver/target/release/Klaar.driver/Contents/MacOS/Klaar
# → Architectures in the fat file: ... are: x86_64 arm64

codesign -dv driver/target/release/Klaar.driver
# Signature=adhoc
```

CI should set `KLAAR_DRIVER_UNIVERSAL=1` before invoking the build; local
dev can skip it.

### Driver debug logging (dev-only)

`driver_log!` and `driver_log_property!` write synchronously to
`/tmp/klaar-driver.log` from inside every CoreAudio HAL property
callback. That's useful when debugging enumeration issues but it must not
ship — the per-callback disk I/O is slow enough to push coreaudiod's
initial device rescan past `install_driver`'s poll deadline on real user
machines.

The macros are gated on the `dev-logging` cargo feature. It is **off** by
default. Opt in during a local build:

```bash
KLAAR_DRIVER_DEV_LOGGING=1 scripts/build-driver.sh
# or, combined with a universal build:
KLAAR_DRIVER_UNIVERSAL=1 KLAAR_DRIVER_DEV_LOGGING=1 scripts/build-driver.sh
# or, driving cargo directly:
cargo build --release --features dev-logging
```

Shipped release builds (default, no env var) produce no log file.
Verify after a plain `scripts/build-driver.sh` + `dev-install.sh` cycle:
`/tmp/klaar-driver.log` should not grow during device enumeration or
steady-state playback.

**Important:** Do not modify the bundle after `codesign` runs. Any change to
`Info.plist` or the Mach-O invalidates the ad-hoc signature and `coreaudiod`
will refuse to load the driver. The Tauri resource-copy step (which stages
the driver into `src-tauri/resources/`) is a plain `cp -R` that preserves
the signature; re-signing after that copy is not required.

### Note on the driver build pipeline

The driver crate doesn't use a `build.rs` — it relies on
`driver/linker-bundle.sh` as a cargo linker wrapper to produce MH_BUNDLE
instead of MH_DYLIB. All post-build steps (lipo, codesign) live in
`scripts/build-driver.sh`; `stage-driver.sh` handles the final copy.

## Code signing

The build scripts use **ad-hoc signing** (`codesign -s -`) by default for
both the driver and the app. Ad-hoc is the minimum bar `coreaudiod`
enforces for HAL plug-ins on modern macOS, so the driver will load; the
app will trip Gatekeeper on first launch but can be opened via
right-click → Open (macOS ≤14) or System Settings → Privacy & Security →
Open Anyway (macOS 15+).

To use a Developer ID identity instead, replace `codesign -s -` in
`scripts/build-driver.sh`, `scripts/stage-driver.sh`, and the relevant
`tauri.conf.json` signing settings with your identity, and add an
`xcrun notarytool submit … --wait` + `xcrun stapler staple` step after
`pnpm tauri build`. Sign both the driver and the app — partial migrations
produce confusing failure modes.

## MIN_DRIVER_VERSION bump policy

`src-tauri/src/constants.rs::MIN_DRIVER_VERSION` is the minimum driver
`CFBundleVersion` the app requires. On launch, the enumeration gate compares
the installed version against this constant and prompts the user to update if
it's older.

**Only bump this when the driver changes its ABI in a way the app depends
on.** Examples of real ABI changes:

- Device UID changes.
- Ring buffer layout / sample format changes.
- New required properties on the HAL device.

Cosmetic changes (log messages, internal refactors, test-only fixes) do
**not** justify a bump — every bump forces existing users through an admin
prompt on next launch, for no functional gain.

When you do bump it, also bump `driver/Cargo.toml::version` and
`driver/Info.plist::CFBundleVersion` in lockstep so the rebuilt bundle
reports the new version. `scripts/build-driver.sh` is the single source of
truth for assembling the bundle and copies `driver/Info.plist` into it on
every build, so the plist edit propagates end-to-end (through
`stage-driver.sh`, `tauri build`, and `install_driver`) without extra steps.
Do not skip `scripts/build-driver.sh` — a plain `cargo build --release` only
produces `libKlaar.dylib` and will not assemble the `.driver` bundle.

## `.pkg` installer

A secondary `.pkg` installer is available for non-app distribution paths
(MDM, silent installs). Build:

```bash
scripts/build-driver.sh                     # assembles + codesigns bundle
driver/pkg/build_pkg.sh
# → driver/pkg/build/KlaarDriver.pkg
```

The pre/post-install scripts set ownership and restart `coreaudiod`. The
package is unsigned; productsign can be layered on when a Developer ID is
available.

## Shipping a release build

Use `pnpm build:release` as the canonical command for producing shippable
artifacts. It chains:

1. `scripts/build-driver.sh` — builds (and optionally cross-compiles +
   `lipo`s), ad-hoc signs, and verifies `Klaar.driver`. Set
   `KLAAR_DRIVER_UNIVERSAL=1` in the environment for a fat binary.
2. `tauri build` — frontend build, Rust release build, DMG packaging.
   `stage-driver.sh` runs as `beforeBundleCommand` and embeds the built
   driver into the bundle. When `KLAAR_DRIVER_UNIVERSAL=1` is set,
   this step passes `--target universal-apple-darwin` so the app binary
   is a fat x86_64+arm64 Mach-O (matching the driver).
3. `scripts/verify-bundle.sh` — runs on the produced `.app`.

The `verify-bundle.sh` step runs:

- `file` on the main binary — asserts every Mach-O slice is the expected
  type (MH_BUNDLE for drivers, executable for apps). Catches the
  cross-compilation failure mode where a slice is emitted as MH_DYLIB.
- `lipo -info` — asserts expected architectures are present (both x86_64
  and arm64 when `KLAAR_DRIVER_UNIVERSAL=1`, otherwise host arch).
- `codesign --verify --deep --strict` — asserts the bundle verifies cleanly.
  Catches the "unsigned `.app` / Gatekeeper damaged" failure mode.

A failing check exits the build non-zero with a human-readable message
identifying which check failed and how to fix it. `scripts/build-driver.sh`
also runs `verify-bundle.sh` on its produced `.driver`, so a broken driver
bundle fails the driver build itself — never reaches `stage-driver` or the
tauri bundler.

**Do not use `pnpm tauri build` directly for release artifacts** — it skips
the verification layer and has historically produced `.app` bundles that
silently failed codesign or carried mismatched Mach-O slices.
Use `pnpm tauri build` only when iterating on the build itself.

## DMG packaging

The DMG produced by `pnpm build:release` contains `Klaar.app`. We also
ship `driver/uninstall.command` at the DMG root so users who've deleted
the app can still remove the driver.

Tauri's DMG bundler doesn't currently copy arbitrary files to the DMG root.
Until that's wired up (via a `dmg.with` config or a custom bundle hook), the
uninstall script must be added manually:

1. `pnpm build:release` — produces `src-tauri/target/release/bundle/dmg/Klaar_<ver>.dmg` (and verifies the bundled `.app`).
2. `hdiutil attach <dmg>`.
3. Drop `driver/uninstall.command` onto the mounted image.
4. Eject + re-compress.

A scripted hook for this lives in `scripts/` — fill in as the DMG tooling
solidifies.

## Frontend update-check

`src/lib/updateCheck.ts` polls
`https://api.github.com/repos/JyrgenSuvalov/klaar/releases/latest` at
most once every 24 hours. The response is cached to
`~/Library/Application Support/Klaar/update-check.json`. Dismissed tags
are stored in the same file. Failures are silent — we never show a
network-error UI here.

Relevant Tauri config:

- `tauri.conf.json` CSP adds `https://api.github.com` to `connect-src`.
- `capabilities/default.json` allows `opener:allow-open-url` (for the
  release-page link) and `clipboard-manager:allow-write-text` (for the
  install-failure diagnostics copy).

## CoreAudio listener kinds

The app keeps three distinct CoreAudio property listeners running, each
covering a different failure mode. They live in `src-tauri/src/`:

- **`device_change_listener.rs` — topology** — listens on
  `kAudioHardwarePropertyDevices` (system object). Fires when any device is
  added or removed. Drives device-list re-enumeration and disconnects the
  engine if the selected input/output disappears.
- **`device_format_listener.rs` — nominal sample rate** — registers
  `kAudioDevicePropertyNominalSampleRate` per enumerated device, re-synced
  from inside the topology callback. Debounces bursts (~150 ms) and on fire
  emits `audio://devices-changed`, runs the same disconnect-on-removal
  reconcile, and tears down the engine if an in-use device's SR drifted
  from the engine's negotiated rate (so the auto-reconnect path can restart
  it). This is the recovery channel for a stuck `sample_rate_mismatch`.
- **`device_monitor.rs` — alive status** — wraps `coreaudio-rs`'s
  `AliveListener` for the currently selected input + output devices.
  Signals when a device dies underneath a running stream (separate from
  topology removal).

All three callbacks run on a CoreAudio internal thread. `device_change_listener`
keeps Tauri work in the callback (existing pattern); `device_format_listener`
is stricter — its callback only does `AtomicU64::store` + `Thread::unpark`,
deferring all logic to a worker thread.

## Auto-launch and the `--launched-at-login` marker

When the user enables auto-launch in Settings, `tauri-plugin-autostart`
writes a `~/Library/LaunchAgents/eu.jyrkki.Klaar.plist` whose
`ProgramArguments` array contains `Klaar.app/Contents/MacOS/Klaar` followed
by `--launched-at-login`. macOS re-fires that command at every login.

The flag is detected at runtime by `util::launch_mode::launched_at_login_from`
(scanning `std::env::args()`) and gates the cold-launch
`show_main_window()` call in `setup()`. When the flag is present we keep
the configuration window hidden — the early-login WKWebView often comes up
as a wedged white surface, so we let the user surface the window via the
tray once the login session is warm. Without the flag (Finder, `open -a`,
Raycast, `cargo tauri dev`) cold-launch behaviour is unchanged.

### Local testing

```bash
cargo build --manifest-path src-tauri/Cargo.toml --release
./src-tauri/target/release/Klaar --launched-at-login
# → tray icon appears, no window opens.
```

### Migrating the LaunchAgent on upgrade

`EXPECTED_LAUNCH_AGENT_ARGS_VERSION` (in `src-tauri/src/lib.rs`) is the
schema marker for the args we expect to be on disk. `setup()` re-registers
the LaunchAgent (idempotent `disable()`+`enable()`) when the persisted
`launch_agent_args_version` lags. Bump the constant whenever the args we
pass to `tauri_plugin_autostart::init` change.
