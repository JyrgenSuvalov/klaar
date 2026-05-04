#!/usr/bin/env bash
# Build the Klaar audio driver bundle, optionally as a Universal Binary,
# and ad-hoc sign the resulting `.driver`.
#
# The driver crate doesn't have a `build.rs` (it relies on the
# `linker-bundle.sh` wrapper to emit MH_BUNDLE), so we wrap the whole
# post-build pipeline in a shell script invoked by CI and by the Tauri
# `beforeBundleCommand` (via `stage-driver.sh`).
#
# `cargo build --release` produces `target/release/libKlaar.dylib`
# (technically MH_BUNDLE thanks to `linker-bundle.sh`) — nothing more. This
# script is the single source of truth for assembling the
# `Klaar.driver/Contents/{MacOS/Klaar, Info.plist}` bundle skeleton
# around that binary, so `driver/Info.plist` edits always propagate.
#
# Background: relying on cargo's auto-generated bundle Info.plist shipped a
# wrong CFBundleExecutable, so coreaudiod silently rejected the plug-in.
# Building the bundle ourselves keeps Info.plist fields under our control.
#
# Usage:
#   scripts/build-driver.sh              # host-arch build + codesign
#   KLAAR_DRIVER_UNIVERSAL=1 scripts/build-driver.sh
#                                        # build both x86_64 + arm64, lipo
#                                        # them together, then codesign
#   KLAAR_DRIVER_DEV_LOGGING=1 scripts/build-driver.sh
#                                        # build with driver_log!/
#                                        # driver_log_property! macros
#                                        # enabled (cargo --features
#                                        # dev-logging). DO NOT ship — the
#                                        # synchronous disk I/O slows
#                                        # coreaudiod enumeration enough to
#                                        # trip install_driver's poll
#                                        # deadline.
#
# Required on the PATH: cargo (with host + cross targets installed), lipo,
# codesign. The script fails loudly if any step exits non-zero.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DRIVER_DIR="$ROOT/driver"
BUNDLE_NAME="Klaar.driver"
BINARY_NAME="Klaar"
DYLIB_NAME="libKlaar.dylib"
INFO_PLIST="$DRIVER_DIR/Info.plist"

cd "$DRIVER_DIR"

HOST_TRIPLE="$(rustc -vV | awk '/^host:/ { print $2 }')"
case "$HOST_TRIPLE" in
  aarch64-apple-darwin) OTHER_TRIPLE="x86_64-apple-darwin" ;;
  x86_64-apple-darwin)  OTHER_TRIPLE="aarch64-apple-darwin" ;;
  *)
    echo "ERROR: unsupported host triple $HOST_TRIPLE" >&2
    exit 1
    ;;
esac

BUNDLE_OUT="$DRIVER_DIR/target/release/$BUNDLE_NAME"
HOST_DYLIB="$DRIVER_DIR/target/release/$DYLIB_NAME"

CARGO_FEATURE_ARGS=()
if [[ "${KLAAR_DRIVER_DEV_LOGGING:-0}" == "1" ]]; then
  CARGO_FEATURE_ARGS=(--features dev-logging)
fi

echo "[build-driver] host=$HOST_TRIPLE universal=${KLAAR_DRIVER_UNIVERSAL:-0} dev-logging=${KLAAR_DRIVER_DEV_LOGGING:-0}"

# 1. Host-arch release build. Produces target/release/libKlaar.dylib
#    (MH_BUNDLE via linker-bundle.sh). The `.driver` bundle skeleton is
#    assembled by this script in step 3 — cargo does not produce it.
cargo build --release ${CARGO_FEATURE_ARGS[@]+"${CARGO_FEATURE_ARGS[@]}"}

if [[ ! -f "$HOST_DYLIB" ]]; then
  echo "ERROR: expected binary $HOST_DYLIB was not produced." >&2
  exit 1
fi

# 2. Optional: cross-compile the other arch and lipo the two dylibs together
#    before we wrap them in the bundle. Doing the lipo on the raw dylib (not
#    on a per-arch bundle) means we only assemble one `.driver` at the end,
#    and both arches pick up the same `Info.plist`.
FINAL_BINARY="$HOST_DYLIB"

if [[ "${KLAAR_DRIVER_UNIVERSAL:-0}" == "1" ]]; then
  if [[ "${KLAAR_DRIVER_SECOND_PASS:-0}" == "1" ]]; then
    echo "[build-driver] second-pass guard tripped — refusing to recurse." >&2
    exit 1
  fi

  echo "[build-driver] building $OTHER_TRIPLE slice"
  KLAAR_DRIVER_SECOND_PASS=1 cargo build --release --target "$OTHER_TRIPLE" ${CARGO_FEATURE_ARGS[@]+"${CARGO_FEATURE_ARGS[@]}"}

  OTHER_DYLIB="$DRIVER_DIR/target/$OTHER_TRIPLE/release/$DYLIB_NAME"

  if [[ ! -f "$OTHER_DYLIB" ]]; then
    echo "ERROR: cross-arch binary not found at $OTHER_DYLIB" >&2
    echo "       Is the $OTHER_TRIPLE toolchain installed? (rustup target add $OTHER_TRIPLE)" >&2
    exit 1
  fi

  FAT_DYLIB="$(mktemp -t klaar-lipo.XXXXXX)"
  echo "[build-driver] lipo -create $HOST_DYLIB + $OTHER_DYLIB"
  lipo -create -output "$FAT_DYLIB" "$HOST_DYLIB" "$OTHER_DYLIB"
  echo "[build-driver] lipo -info:"
  lipo -info "$FAT_DYLIB"
  FINAL_BINARY="$FAT_DYLIB"
fi

# 3. Assemble the `.driver` bundle skeleton from scratch. We rm -rf first so
#    stale residue (old Info.plist from a prior dev-install.sh, old binary
#    from a prior arch) can never survive across builds.
echo "[build-driver] assembling bundle at $BUNDLE_OUT"
rm -rf "$BUNDLE_OUT"
mkdir -p "$BUNDLE_OUT/Contents/MacOS"
cp "$FINAL_BINARY" "$BUNDLE_OUT/Contents/MacOS/$BINARY_NAME"
cp "$INFO_PLIST" "$BUNDLE_OUT/Contents/Info.plist"

# Clean up the fat-dylib tempfile once it's been copied into the bundle.
if [[ "${KLAAR_DRIVER_UNIVERSAL:-0}" == "1" ]]; then
  rm -f "$FAT_DYLIB"
fi

# 4. Required ad-hoc codesign. Amendment A10: non-optional. Any modification
# of the bundle after this point invalidates the signature and coreaudiod
# will refuse to load it.
echo "[build-driver] codesign -s - --force --deep $BUNDLE_OUT"
codesign -s - --force --deep "$BUNDLE_OUT"

# 5. Post-build verification. Fails loud on any of the known shippable-bundle
# regressions (MH_DYLIB slices, missing arch, invalid codesign). `set -e`
# propagates the non-zero exit so downstream steps (stage-driver, tauri
# bundle) never see a broken artifact.
"$ROOT/scripts/verify-bundle.sh" "$BUNDLE_OUT"

echo "[build-driver] Done. Bundle ready at: $BUNDLE_OUT"
