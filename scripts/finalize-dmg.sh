#!/usr/bin/env bash
# Inject driver/uninstall.command into the Tauri-produced DMG, with a Finder
# icon layout that places it visibly alongside Klaar.app and Applications.
#
# Tauri 2's DmgConfig only exposes background / windowSize / windowPosition
# and positions for `app` + `applicationFolder`. There is no first-class way
# to add extra files to the disk image, and (more importantly) no way to
# record an icon position for any extra file we inject ourselves. If we just
# copy `uninstall.command` into the volume after Tauri's bundler has already
# baked `.DS_Store`, Finder shows it at an undefined position — typically
# stacked behind Klaar.app — which makes it look like the file is
# missing entirely.
#
# This script post-processes the DMG by:
#
#   1. Converting the read-only UDZO image to read-write UDRW.
#   2. Attaching the read-write image under /Volumes/<volname> (no -nobrowse;
#      Finder needs to address the volume by name to set icon positions).
#   3. Copying `driver/uninstall.command` into the volume root, preserving +x.
#   4. Driving Finder via AppleScript to set window bounds and explicit icon
#      positions for Klaar.app, Applications, and uninstall.command — so
#      `.DS_Store` records all three.
#   5. Syncing, detaching.
#   6. Re-compressing back to UDZO and replacing the original DMG.
#
# Idempotent — safe to re-run; the copy overwrites any existing entry. Cleans
# up the mount on any failure path via trap.
#
# Usage:
#   bash scripts/finalize-dmg.sh <path-to-dmg>

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "[finalize-dmg] usage: finalize-dmg.sh <path-to-dmg>" >&2
  exit 2
fi

DMG_PATH="$1"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
UNINSTALL_SRC="$ROOT/driver/uninstall.command"

if [[ ! -f "$DMG_PATH" ]]; then
  echo "[finalize-dmg] DMG not found at $DMG_PATH" >&2
  exit 1
fi
if [[ ! -f "$UNINSTALL_SRC" ]]; then
  echo "[finalize-dmg] uninstall.command not found at $UNINSTALL_SRC" >&2
  exit 1
fi

echo "[finalize-dmg] injecting uninstall.command into $(basename "$DMG_PATH")"

WORK_DIR="$(mktemp -d -t klaar-finalize-dmg)"
RW_DMG="$WORK_DIR/rw.dmg"
FINAL_DMG="$WORK_DIR/final.dmg"

DEVICE=""
MOUNT_POINT=""

cleanup() {
  if [[ -n "$DEVICE" ]] && hdiutil info | grep -q "^$DEVICE"; then
    hdiutil detach "$DEVICE" -quiet >/dev/null 2>&1 \
      || hdiutil detach "$DEVICE" -force -quiet >/dev/null 2>&1 \
      || true
  fi
  rm -rf "$WORK_DIR"
}
trap cleanup EXIT

# 1. Convert UDZO → UDRW so we can mutate the volume contents.
hdiutil convert "$DMG_PATH" -format UDRW -o "$RW_DMG" -quiet

# 2. Attach under /Volumes/<volname> so Finder can address the volume by
#    name in the AppleScript step. -noautoopen keeps a Finder window from
#    flashing onto the screen during builds.
ATTACH_OUTPUT="$(hdiutil attach "$RW_DMG" -noverify -noautoopen)"
while IFS=$'\t' read -r dev _ mnt; do
  # Strip whitespace; the table is tab/space separated and trailing spaces
  # confuse downstream commands.
  mnt="$(echo "$mnt" | sed -e 's/^ *//' -e 's/ *$//')"
  if [[ "$mnt" == /Volumes/* ]]; then
    DEVICE="$(echo "$dev" | sed -e 's/^ *//' -e 's/ *$//')"
    MOUNT_POINT="$mnt"
  fi
done <<< "$ATTACH_OUTPUT"

if [[ -z "$MOUNT_POINT" || -z "$DEVICE" ]]; then
  echo "[finalize-dmg] failed to determine mount point from hdiutil output:" >&2
  echo "$ATTACH_OUTPUT" >&2
  exit 1
fi
VOLUME_NAME="$(basename "$MOUNT_POINT")"

# 3. Copy the uninstaller. -p preserves the +x bit; we re-chmod defensively
#    in case the source filesystem dropped it.
cp -p "$UNINSTALL_SRC" "$MOUNT_POINT/uninstall.command"
chmod +x "$MOUNT_POINT/uninstall.command"

# 4. Drive Finder to record icon positions for all three items in .DS_Store.
#    Window bounds: 660×400 (matches a comfortable 3-icon layout). Icons:
#      Klaar.app      → (160, 180)  upper-left
#      Applications        → (500, 180)  upper-right (drag-to-install)
#      uninstall.command   → (330, 320)  lower-centre (clearly visible)
#    The `delay 1` after `update` lets Finder flush .DS_Store to disk before
#    we detach.
osascript <<APPLESCRIPT
tell application "Finder"
  tell disk "$VOLUME_NAME"
    open
    set current view of container window to icon view
    set toolbar visible of container window to false
    set statusbar visible of container window to false
    set the bounds of container window to {400, 200, 1060, 600}
    set viewOptions to the icon view options of container window
    set arrangement of viewOptions to not arranged
    set icon size of viewOptions to 96
    set position of item "Klaar.app" of container window to {160, 180}
    set position of item "Applications" of container window to {500, 180}
    set position of item "uninstall.command" of container window to {330, 320}
    update without registering applications
    delay 1
    close
  end tell
end tell
APPLESCRIPT

# Belt-and-braces: make sure .DS_Store is on disk before unmount.
sync
sleep 1

# 5. Detach.
hdiutil detach "$DEVICE" -quiet
DEVICE=""

# 6. Recompress to UDZO and atomically replace the original via mv.
hdiutil convert "$RW_DMG" -format UDZO -imagekey zlib-level=9 -o "$FINAL_DMG" -quiet
mv "$FINAL_DMG" "$DMG_PATH"

# Sanity: the produced DMG must verify cleanly.
hdiutil verify "$DMG_PATH" -quiet

echo "[finalize-dmg] uninstall.command staged in $DMG_PATH (with icon layout)"
