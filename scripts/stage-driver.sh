#!/usr/bin/env bash
# Stage the built driver bundle so it's reachable from both the Tauri bundler
# AND the dev / cargo-built binaries.
#
# Tauri 2 copies the `resources` declarations from tauri.conf.json into the
# bundled app at `tauri build` time, but NOT during `tauri dev` / plain
# `cargo build`. `BaseDirectory::Resource` in dev resolves to the binary's
# parent directory (e.g. `src-tauri/target/debug/`), so we have to put the
# bundle there ourselves for `install_driver` to find it.
#
# Called from Tauri's `beforeBundleCommand` (release bundling) and should
# also be run manually before `pnpm tauri dev` whenever the driver is rebuilt.
#
# Fails loudly if the driver source bundle doesn't exist — no silent fallback.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="$ROOT/driver/target/release/Klaar.driver"

if [[ ! -d "$SRC" ]]; then
  echo "ERROR: driver bundle not found at $SRC" >&2
  echo "       Build it first: cd driver && cargo build --release" >&2
  echo "       Or for a Universal + signed release: scripts/build-driver.sh" >&2
  exit 1
fi

# Ad-hoc codesigning is required — coreaudiod on modern macOS refuses to
# load unsigned HAL plug-ins, and ad-hoc is the only option until we acquire
# a Developer ID. Signing here is idempotent (`--force` overwrites any prior
# signature). Must run BEFORE stage_to so the signed bundle is what ends up
# in resources/ and the dev target dir.
echo "  codesigning $SRC"
codesign -s - --force --deep "$SRC" >/dev/null

stage_to() {
  local dest_dir="$1"
  local dest="$dest_dir/Klaar.driver"
  mkdir -p "$dest_dir"
  rm -rf "$dest"
  cp -R "$SRC" "$dest"
  echo "  staged → $dest"
}

# 1. Bundle-resources source of truth (used by `tauri build` bundler).
#    The backend resolves `resources/Klaar.driver` under
#    `BaseDirectory::Resource`, which matches where tauri-bundler actually
#    copies glob-matched resources in the packaged .app
#    (Contents/Resources/resources/Klaar.driver).
stage_to "$ROOT/src-tauri/resources"

# 2. Dev binary location — `BaseDirectory::Resource` in `pnpm tauri dev`.
#    Mirror the `resources/` subdirectory so the same resolve call works in
#    both dev and release modes.
stage_to "$ROOT/src-tauri/target/debug/resources"

# 3. Release binary location — `BaseDirectory::Resource` for ad-hoc
#    `cargo run --release` without going through the bundler.
if [[ -d "$ROOT/src-tauri/target/release" ]]; then
  stage_to "$ROOT/src-tauri/target/release/resources"
fi

echo "Done."
