#!/usr/bin/env bash
# Build a shippable Klaar release: driver → app → verify.
#
# Stages:
#   1. scripts/build-driver.sh         — build + codesign + verify the .driver.
#   2. tauri build [--target ...]      — frontend + Rust release + DMG.
#      stage-driver.sh runs as beforeBundleCommand and embeds the driver.
#   3. scripts/finalize-dmg.sh         — inject driver/uninstall.command into
#                                        the produced DMG (Tauri's DmgConfig
#                                        has no extra-files knob).
#   4. scripts/verify-bundle.sh        — run against the produced .app.
#
# Set KLAAR_DRIVER_UNIVERSAL=1 for a universal (x86_64 + arm64) build of
# both driver AND app. When unset, both stay host-arch.
#
# The variable is already named "_DRIVER_" for historical reasons but it
# governs the whole release now — same flag, both artefacts stay in lockstep.
#
# Usage:
#   pnpm build:release
#   KLAAR_DRIVER_UNIVERSAL=1 pnpm build:release

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

UNIVERSAL="${KLAAR_DRIVER_UNIVERSAL:-0}"

echo "[build-release] universal=$UNIVERSAL"

# Scrub build-machine paths from shipped binaries (issue #49). Rust embeds
# absolute source paths into release artefacts via file!(), panic messages,
# and debug info — including ~/.cargo/registry, ~/.cargo/git, and the project
# root. Without remapping, an end-user crash backtrace exposes the dev's home
# directory and username (cosmetic, not a security issue, but unprofessional).
#
# We only set this in build-release.sh, never for `pnpm tauri dev` or plain
# `cargo build` — local paths in panics are useful while debugging. The export
# cascades through the build-driver.sh subprocess and the `pnpm tauri build`
# invocation below, scrubbing both the driver bundle and the app binary.
#
# Acceptance: `strings <Klaar app binary> | grep -c "$HOME"` returns 0.
REMAP_FLAGS=(
  "--remap-path-prefix=$HOME/.cargo/registry=/cargo/registry"
  "--remap-path-prefix=$HOME/.cargo/git=/cargo/git"
  "--remap-path-prefix=$HOME/.rustup=/rustup"
  "--remap-path-prefix=$ROOT=/build"
)
export RUSTFLAGS="${RUSTFLAGS:-} ${REMAP_FLAGS[*]}"
echo "[build-release] RUSTFLAGS=$RUSTFLAGS"

# 1. Driver build. Reads KLAAR_DRIVER_UNIVERSAL itself, and runs
#    verify-bundle.sh on the produced .driver at the end.
bash "$ROOT/scripts/build-driver.sh"

# 2. Tauri build. Universal adds --target universal-apple-darwin, which makes
#    cargo lipo the two per-arch slices and changes the output path.
if [[ "$UNIVERSAL" == "1" ]]; then
  pnpm tauri build --target universal-apple-darwin
  BUNDLE_DIR="$ROOT/src-tauri/target/universal-apple-darwin/release/bundle"
else
  pnpm tauri build
  BUNDLE_DIR="$ROOT/src-tauri/target/release/bundle"
fi
APP_PATH="$BUNDLE_DIR/macos/Klaar.app"

# 3. Inject uninstall.command into every produced .dmg. tauri-bundler emits
#    one per arch (e.g. Klaar_<ver>_aarch64.dmg or _universal.dmg) at
#    bundle/dmg/. We loop in case multiple targets land here in the future.
shopt -s nullglob
DMG_FILES=("$BUNDLE_DIR"/dmg/*.dmg)
shopt -u nullglob
if [[ ${#DMG_FILES[@]} -eq 0 ]]; then
  echo "[build-release] no DMG produced under $BUNDLE_DIR/dmg/ — skipping finalize-dmg" >&2
else
  for dmg in "${DMG_FILES[@]}"; do
    bash "$ROOT/scripts/finalize-dmg.sh" "$dmg"
  done
fi

# 4. Verify the produced .app.
bash "$ROOT/scripts/verify-bundle.sh" "$APP_PATH"

echo "[build-release] Done. App at: $APP_PATH"
