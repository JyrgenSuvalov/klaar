#!/bin/bash
# Klaar driver loopback test.
#
# Plays a 440 Hz sine into the Klaar output stream and
# simultaneously records from its input stream. A successful loopback proves
# the ring buffer round-trips samples from WriteMix → ReadInput.
#
# Usage: ./scripts/loopback-test.sh [duration_seconds]

set -euo pipefail

DEVICE="Klaar"
DURATION="${1:-5}"
OUT_FILE="/tmp/loopback_test.wav"
RATE=48000

command -v sox >/dev/null 2>&1 || { echo "ERROR: sox not found. brew install sox"; exit 1; }

echo "=== Klaar loopback test ==="
echo "  device:   $DEVICE"
echo "  duration: ${DURATION}s"
echo "  rate:     ${RATE} Hz"
echo "  capture:  $OUT_FILE"
echo ""

# 1. Start recorder in the background. Give it a moment to open the device
#    and call StartIO before the player starts pushing samples.
echo "[1/3] Starting recorder..."
sox -q -t coreaudio "$DEVICE" -c 2 -r "$RATE" "$OUT_FILE" trim 0 "$DURATION" &
REC_PID=$!
sleep 0.4

# 2. Play a sine into the device output. Blocks for DURATION seconds.
echo "[2/3] Playing 440 Hz sine into device output..."
sox -q -n -t coreaudio "$DEVICE" synth "$DURATION" sine 440 vol 0.5

# 3. Wait for the recorder to finish writing.
wait "$REC_PID"
echo "[3/3] Recording complete. Analysing..."
echo ""

# Print sox stat on the captured file. Signal should have:
#   RMS amplitude ~= 0.354  (0.5 / sqrt(2))
#   Maximum amplitude ~= 0.5
#   Minimum amplitude ~= -0.5
# Near-zero values mean the ring buffer never delivered samples.
sox "$OUT_FILE" -n stat 2>&1

echo ""
echo "=== Done ==="
echo "Captured file: $OUT_FILE"
echo "Play it back:  afplay $OUT_FILE"
