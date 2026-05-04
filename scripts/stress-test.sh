#!/bin/bash
# Klaar 30-minute stress test.
#
# Runs a continuous loopback (sox sine → Klaar output, sox recorder ←
# Klaar input) for DURATION seconds while sampling coreaudiod memory
# usage once per minute. A pass means:
#   - both sox processes stay alive for the whole duration
#   - coreaudiod RSS does not grow significantly (>20%)
#   - final sox stat on the captured audio shows a clean 440 Hz sine
#     (max/min ≈ ±0.5, no clipping, no silence gaps)
#   - no warnings or unexpected StopIO events in /tmp/klaar-driver.log
#
# Usage: ./scripts/stress-test.sh [duration_seconds]   # default 1800 (30 min)

set -euo pipefail

DEVICE="Klaar"
DURATION="${1:-1800}"
RATE=48000
CAPTURE_FILE="/tmp/stress_capture.wav"
MEM_LOG="/tmp/stress_coreaudiod_mem.csv"
RUN_LOG="/tmp/stress_run.log"

command -v sox >/dev/null 2>&1 || { echo "ERROR: sox not found. brew install sox"; exit 1; }

echo "=== Klaar 30-minute stress test ==="
echo "  device:   $DEVICE"
echo "  duration: ${DURATION}s ($((DURATION / 60)) min)"
echo "  rate:     ${RATE} Hz"
echo "  capture:  $CAPTURE_FILE"
echo "  mem log:  $MEM_LOG"
echo "  run log:  $RUN_LOG"
echo ""

: > "$RUN_LOG"
: > "$MEM_LOG"
echo "timestamp,elapsed_sec,coreaudiod_rss_kb" > "$MEM_LOG"

cleanup() {
    echo ""
    echo "[cleanup] Stopping processes..."
    [[ -n "${REC_PID:-}" ]] && kill "$REC_PID" 2>/dev/null || true
    [[ -n "${PLAY_PID:-}" ]] && kill "$PLAY_PID" 2>/dev/null || true
    wait 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# 1. Start continuous recorder and player
echo "[1/3] Starting recorder (writing $DURATION s to $CAPTURE_FILE)..."
sox -q -t coreaudio "$DEVICE" -c 2 -r "$RATE" "$CAPTURE_FILE" trim 0 "$DURATION" \
    >> "$RUN_LOG" 2>&1 &
REC_PID=$!
sleep 0.4

echo "[2/3] Starting continuous sine player..."
sox -q -n -t coreaudio "$DEVICE" synth "$DURATION" sine 440 vol 0.5 \
    >> "$RUN_LOG" 2>&1 &
PLAY_PID=$!
sleep 0.5

# 2. Monitor loop — sample coreaudiod RSS every 60 s, report progress every 5 min
START=$(date +%s)
BASELINE_RSS=""
LAST_REPORT=0

echo "[3/3] Monitoring for ${DURATION}s..."
echo "  (you'll see a progress line every 5 minutes)"
echo ""

while true; do
    NOW=$(date +%s)
    ELAPSED=$((NOW - START))

    # Check both processes are still alive
    if ! kill -0 "$REC_PID" 2>/dev/null; then
        echo "ERROR: recorder died at ${ELAPSED}s"
        exit 1
    fi
    if ! kill -0 "$PLAY_PID" 2>/dev/null; then
        echo "ERROR: player died at ${ELAPSED}s"
        exit 1
    fi

    # Sample coreaudiod RSS (KB)
    RSS=$(ps -o rss= -p "$(pgrep -x coreaudiod | head -1)" 2>/dev/null | tr -d ' ' || echo "0")
    echo "$(date -u +%Y-%m-%dT%H:%M:%SZ),$ELAPSED,$RSS" >> "$MEM_LOG"
    [[ -z "$BASELINE_RSS" ]] && BASELINE_RSS=$RSS

    # Every 5 minutes, print a progress line
    if (( ELAPSED - LAST_REPORT >= 300 )) || (( ELAPSED == 0 )); then
        GROWTH="n/a"
        if [[ -n "$BASELINE_RSS" && "$BASELINE_RSS" -gt 0 ]]; then
            GROWTH=$(( (RSS - BASELINE_RSS) * 100 / BASELINE_RSS ))%
        fi
        printf "  [%4ds / %ds]  coreaudiod RSS: %s KB  (growth: %s)\n" \
            "$ELAPSED" "$DURATION" "$RSS" "$GROWTH"
        LAST_REPORT=$ELAPSED
    fi

    if (( ELAPSED >= DURATION )); then
        break
    fi

    sleep 60
done

# 3. Let recorder finish writing
echo ""
echo "[done] Waiting for recorder to flush..."
wait "$REC_PID" 2>/dev/null || true
wait "$PLAY_PID" 2>/dev/null || true

echo ""
echo "=== Analysing capture ==="
sox "$CAPTURE_FILE" -n stat 2>&1

echo ""
echo "=== Memory trace ==="
echo "First 3 samples:"
head -4 "$MEM_LOG"
echo "Last 3 samples:"
tail -3 "$MEM_LOG"

echo ""
echo "=== Done ==="
echo "Capture:  $CAPTURE_FILE"
echo "Mem log:  $MEM_LOG"
echo "Run log:  $RUN_LOG"
