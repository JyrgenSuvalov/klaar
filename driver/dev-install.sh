#!/bin/bash
set -euo pipefail

# Development install script for the Klaar virtual audio driver.
# Builds the driver, assembles the .driver bundle, installs to the HAL
# directory, and restarts coreaudiod.
#
# Usage: sudo ./dev-install.sh
#   or:  ./dev-install.sh  (will prompt for sudo password)

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DRIVER_NAME="Klaar"
BUNDLE_NAME="${DRIVER_NAME}.driver"
HAL_DIR="/Library/Audio/Plug-Ins/HAL"
INSTALL_PATH="${HAL_DIR}/${BUNDLE_NAME}"
BUNDLE_DIR="$SCRIPT_DIR/target/release/${BUNDLE_NAME}"

echo "=== Klaar Driver: Build && Install ==="
echo ""

# 1 & 2. Build + assemble + codesign the bundle via the shared build script.
#        build-driver.sh is the single source of truth for bundle assembly —
#        it copies `driver/Info.plist` into the bundle every build, so edits
#        to the plist can't silently go missing (see
#        artifacts/DRIVER-BUNDLE-INFO-PLIST-BUG.md).
echo "[1/4] Building & assembling driver bundle..."
"$REPO_ROOT/scripts/build-driver.sh"

if ! codesign --verify --verbose "$BUNDLE_DIR" 2>&1; then
    echo "ERROR: Codesign verification failed"
    exit 1
fi
echo "  ✓ Bundle ready at $BUNDLE_DIR"

# 3. Install to HAL directory (requires root)
echo "[2/4] Installing to ${HAL_DIR}/..."
if [ "$(id -u)" -ne 0 ]; then
    echo "  (Requesting sudo for installation...)"
    sudo mkdir -p "$HAL_DIR"
    sudo rm -rf "$INSTALL_PATH"
    sudo cp -R "$BUNDLE_DIR" "$INSTALL_PATH"
    sudo chown -R root:wheel "$INSTALL_PATH"
    sudo chmod -R 755 "$INSTALL_PATH"
else
    mkdir -p "$HAL_DIR"
    rm -rf "$INSTALL_PATH"
    cp -R "$BUNDLE_DIR" "$INSTALL_PATH"
    chown -R root:wheel "$INSTALL_PATH"
    chmod -R 755 "$INSTALL_PATH"
fi
echo "  ✓ Installed to $INSTALL_PATH"

# 4. Restart coreaudiod
#
# Why dev restarts coreaudiod when production does NOT:
#
# The production install IPC (`src-tauri/src/commands/driver.rs::install_driver`)
# deliberately stops at the file-copy step and surfaces a Reboot Required
# dialog instead of restarting coreaudiod. The rationale: even a graceful
# SIGTERM-respawn is unsafe in production because the dev's next bundle
# could be broken in ways the dev hasn't seen, and a broken bundle in HAL
# during a respawn can wedge the user's machine.
#
# This dev script keeps the restart for two reasons:
#   - Dev iteration time matters more than production safety here. Waiting
#     for a reboot per build cycle would gut the inner loop.
#   - The dev knows the bundle is sane (this script just built and
#     codesign-verified it), so the broken-bundle pathology that
#     production has to defend against doesn't apply.
#
# So: this script is fine to keep restarting coreaudiod. Just don't copy
# the strategy back into the prod IPC — that's a regression that costs
# users a reboot of their entire Mac.
#
# IMPORTANT: Use `killall` (SIGTERM), NOT `killall -9` (SIGKILL).
#
# SIGKILL gives coreaudiod no chance to tear down its XPC connections to
# out-of-process HAL plugin hosts and third-party audio control daemons
# (e.g. FocusriteControlServer). Those peers are left holding stale XPC
# handles and enter tight reconnection loops against the respawned
# coreaudiod, dragging every audio daemon on the system into a CPU
# feedback storm. On machines with persistent third-party audio helpers
# this cascades into a system-wide wedge (load average 60+, every audio
# process pegged). See DIAGNOSIS.md at the repo root for the full
# post-mortem.
#
# SIGTERM is caught by coreaudiod's cleanup handlers, XPC connections are
# torn down gracefully, and launchd respawns coreaudiod via its KeepAlive
# declaration.
#
# (`launchctl kickstart -k system/com.apple.audio.coreaudiod` would be the
# most "correct" approach but is blocked by SIP on modern macOS: "Operation
# not permitted while System Integrity Protection is engaged".)
echo "[3/4] Restarting coreaudiod gracefully (SIGTERM + launchd respawn)..."
if [ "$(id -u)" -ne 0 ]; then
    sudo killall coreaudiod 2>/dev/null || true
else
    killall coreaudiod 2>/dev/null || true
fi
echo "  ✓ coreaudiod restarted"

# 5. Verify
echo "[4/4] Waiting for device to appear..."
sleep 2

if system_profiler SPAudioDataType 2>/dev/null | grep -q "Klaar"; then
    echo ""
    echo "=== SUCCESS ==="
    echo "\"Klaar\" is now available in System Settings → Sound"
else
    echo ""
    echo "=== WARNING ==="
    echo "Device not yet detected in system_profiler."
    echo "It may take a few more seconds. Check:"
    echo "  system_profiler SPAudioDataType | grep -A5 Klaar"
    echo ""
    echo "For debug logs:"
    echo "  log stream --predicate 'subsystem == \"com.apple.audio\"' --level debug"
fi
