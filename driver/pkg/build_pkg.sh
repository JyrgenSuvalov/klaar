#!/usr/bin/env bash
# Build a macOS `.pkg` installer for the Klaar Virtual Audio Driver.
#
# Produces: driver/pkg/build/KlaarDriver.pkg
#
# This is the secondary distribution path. The primary distribution is the
# DMG shipped from the main Tauri app, which installs the driver via
# `install_driver` on first launch. The `.pkg` is retained for users or
# admins who prefer a non-app-driven install.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DRIVER_BUNDLE="$ROOT/driver/target/release/Klaar.driver"
PKG_DIR="$ROOT/driver/pkg"
BUILD_DIR="$PKG_DIR/build"
ROOT_DIR="$BUILD_DIR/root"
SCRIPTS_DIR="$PKG_DIR/scripts"
HAL_DIR="Library/Audio/Plug-Ins/HAL"
OUT="$BUILD_DIR/KlaarDriver.pkg"

IDENTIFIER="com.klaar.driver.installer"
VERSION="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$DRIVER_BUNDLE/Contents/Info.plist" 2>/dev/null || echo 1.0.0)"

if [[ ! -d "$DRIVER_BUNDLE" ]]; then
  echo "ERROR: driver bundle not found at $DRIVER_BUNDLE" >&2
  echo "       Build it first: scripts/build-driver.sh" >&2
  exit 1
fi

rm -rf "$BUILD_DIR"
mkdir -p "$ROOT_DIR/$HAL_DIR"
cp -R "$DRIVER_BUNDLE" "$ROOT_DIR/$HAL_DIR/Klaar.driver"

# Ensure scripts are executable — pkgbuild respects the +x bit.
chmod +x "$SCRIPTS_DIR/preinstall" "$SCRIPTS_DIR/postinstall"

pkgbuild \
  --root "$ROOT_DIR" \
  --scripts "$SCRIPTS_DIR" \
  --identifier "$IDENTIFIER" \
  --version "$VERSION" \
  --install-location "/" \
  "$OUT"

echo
echo "Built: $OUT (version $VERSION)"
