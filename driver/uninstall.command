#!/bin/bash
# Klaar — manual driver uninstaller shipped in the DMG.
#
# Double-click this file from the mounted DMG to remove the Klaar
# Virtual Audio Driver from `/Library/Audio/Plug-Ins/HAL/`. You'll be asked
# for your Mac password; this is macOS confirming a privileged file deletion.
#
# Afterwards, communication apps will no longer see "Klaar"
# as a microphone. Reinstall by launching Klaar again — it'll offer to
# put the driver back.

set -e

echo "Klaar — remove the virtual audio driver"
echo "---------------------------------------------"
echo
echo "This will delete /Library/Audio/Plug-Ins/HAL/Klaar.driver"
echo "and restart the macOS audio daemon."
echo

sudo rm -rf /Library/Audio/Plug-Ins/HAL/Klaar.driver
sudo killall coreaudiod || true

echo
echo "Done. Klaar has been removed."
echo
read -n 1 -s -r -p "Press any key to close."
echo
