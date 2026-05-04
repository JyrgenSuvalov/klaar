#!/bin/bash
set -e
echo "Removing Klaar virtual audio driver..."
sudo rm -rf /Library/Audio/Plug-Ins/HAL/Klaar.driver
sudo killall -9 coreaudiod
echo "Done. Klaar audio driver has been removed."
