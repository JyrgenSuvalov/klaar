#!/usr/bin/env bash
# ==============================================================================
#  Klaar Performance Test Suite  v1.0
# ==============================================================================
#  Automates the soak-test, xrun monitoring, heap snapshots, and CPU trace
#  capture for release-candidate validation. See `docs/performance-testing.md`
#  for usage.
#
#  Prerequisites (all standard on macOS + Homebrew):
#    - sox          (brew install sox)
#    - Klaar driver installed (the engine's virtual-mic output sink)
#    - A loopback audio device (VB-Cable, BlackHole, Loopback, etc.) named via the
#      TEST_INPUT_DEVICE env var. sox plays pink noise into it so Klaar
#      (with its input pointed at the same device) gets a continuous,
#      reproducible test signal for the full soak duration.
#    - xcrun / xctrace  (Xcode Command Line Tools)
#    - heap, leaks, log, pgrep  (macOS built-ins)
#
#  Note: the Klaar driver cannot be used as the test-signal source. The
#  engine writes to it and apps read from it — having sox also write to it
#  would collide with the engine's writes. Use a separate loopback device
#  for signal injection.
#
#  Usage:
#    ./scripts/perf-test.sh check
#    ./scripts/perf-test.sh start [--duration SECS]
#    ./scripts/perf-test.sh stop
#    ./scripts/perf-test.sh trace [--duration SECS]
#    ./scripts/perf-test.sh snapshot [LABEL]
#    ./scripts/perf-test.sh report [RUN_DIR]
#    ./scripts/perf-test.sh status
# ==============================================================================

SCRIPT_VERSION="1.0"
SCRIPT_PATH="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"

# ── Defaults (override via env) ────────────────────────────────────────────────
APP_NAME="${APP_NAME:-Klaar}"
DRIVER_DEVICE="${DRIVER_DEVICE:-Klaar}"          # the Klaar HAL driver / engine output
TEST_INPUT_DEVICE="${TEST_INPUT_DEVICE:-}"       # required loopback for sox test signal — must be set by caller
TEST_DURATION_SECS="${TEST_DURATION_SECS:-7200}"    # 2 hours
TRACE_DURATION_SECS="${TRACE_DURATION_SECS:-30}"    # 30s CPU trace
EARLY_SNAPSHOT_SECS="${EARLY_SNAPSHOT_SECS:-300}"   # take first heap snap at t+5min
STATE_BASE="${PERF_STATE_BASE:-/tmp/klaar-perf}"
CURRENT_LINK="$STATE_BASE/current"

# ── Terminal colours (suppressed when not a TTY) ───────────────────────────────
if [[ -t 1 ]]; then
  R='\033[0;31m' G='\033[0;32m' Y='\033[1;33m'
  B='\033[0;34m' C='\033[0;36m' W='\033[1m' N='\033[0m'
else
  R='' G='' Y='' B='' C='' W='' N=''
fi

_info()    { echo -e "${G}✓${N}  $*"; }
_warn()    { echo -e "${Y}⚠${N}  $*"; }
_error()   { echo -e "${R}✗${N}  $*" >&2; }
_section() { echo -e "\n${W}${B}── $* ──${N}"; }
_step()    { echo -e "  ${C}→${N} $*"; }
_die()     { _error "$*"; exit 1; }


# ══════════════════════════════════════════════════════════════════════════════
#  State helpers
# ══════════════════════════════════════════════════════════════════════════════

_state_file() { echo "$CURRENT_LINK/.state"; }

state_get() {
  local key="$1"
  local sf
  sf="$(_state_file)"
  [[ -f "$sf" ]] || return 1
  grep "^${key}=" "$sf" 2>/dev/null | head -1 | cut -d= -f2-
}

state_set() {
  local key="$1" val="$2"
  local sf
  sf="$(_state_file)"
  [[ -f "$sf" ]] || return 1
  local tmp
  tmp=$(mktemp)
  grep -v "^${key}=" "$sf" > "$tmp" 2>/dev/null || true
  echo "${key}=${val}" >> "$tmp"
  mv "$tmp" "$sf"
}

run_is_active() {
  [[ -L "$CURRENT_LINK" && -d "$CURRENT_LINK" && -f "$CURRENT_LINK/.state" ]]
}

get_app_pid() {
  # Try exact name match (release bundle), then lowercase (dev build), then substring
  pgrep -x "$APP_NAME" 2>/dev/null | head -1 \
    || pgrep -x "$(echo "$APP_NAME" | tr '[:upper:]' '[:lower:]')" 2>/dev/null | head -1 \
    || pgrep -f "$APP_NAME" 2>/dev/null | grep -v "perf-test" | head -1 \
    || true
}


# ══════════════════════════════════════════════════════════════════════════════
#  check — verify prerequisites
# ══════════════════════════════════════════════════════════════════════════════

cmd_check() {
  _section "Prerequisite Check"
  local missing=0

  # sox
  if command -v sox &>/dev/null; then
    _info "sox $(sox --version 2>&1 | grep -oE 'v[0-9.]+' | head -1)"
  else
    _error "sox not found  (brew install sox)"
    (( missing++ )) || true
  fi

  # Klaar driver (engine's virtual-mic output)
  if system_profiler SPAudioDataType 2>/dev/null | grep -q "$DRIVER_DEVICE"; then
    _info "\"$DRIVER_DEVICE\" driver present"
  else
    _warn "\"$DRIVER_DEVICE\" driver not found — install via the app (Settings → Install driver)"
    _step "Or run: cargo run --manifest-path driver/Cargo.toml ... and stage-driver.sh"
  fi

  # Required test-signal loopback
  if [[ -z "$TEST_INPUT_DEVICE" ]]; then
    _error "TEST_INPUT_DEVICE is unset — required for soak testing"
    _step "Set it to a CoreAudio loopback device name, e.g.:"
    _step "  TEST_INPUT_DEVICE=\"BlackHole 16ch\" ./scripts/perf-test.sh check"
    (( missing++ )) || true
  elif system_profiler SPAudioDataType 2>/dev/null | grep -q "$TEST_INPUT_DEVICE"; then
    _info "\"$TEST_INPUT_DEVICE\" loopback present (sox will inject pink noise)"
  else
    _error "\"$TEST_INPUT_DEVICE\" loopback not found"
    _step "Install BlackHole, VB-Cable, Loopback, or similar — or fix the device name."
    (( missing++ )) || true
  fi

  # xctrace (Instruments) — requires full Xcode, not just Command Line Tools
  if xctrace version &>/dev/null; then
    _info "xctrace $(xctrace version 2>&1 | head -1)"
  elif command -v xctrace &>/dev/null; then
    _warn "xctrace present but needs full Xcode (not just Command Line Tools)"
    _step "Fix: sudo xcode-select -s /Applications/Xcode.app/Contents/Developer"
    _step "CPU traces will fail until this is resolved"
  else
    _warn "xctrace not found — CPU traces unavailable  (install Xcode)"
  fi

  # heap
  if command -v heap &>/dev/null; then
    _info "heap  $(which heap)"
  else
    _warn "heap not found — memory snapshots unavailable"
    _step "Should be present at /usr/bin/heap on macOS 12+"
  fi

  # leaks
  if command -v leaks &>/dev/null; then
    _info "leaks  $(which leaks)"
  else
    _warn "leaks not found — leak detection unavailable"
  fi

  # log stream
  if command -v log &>/dev/null; then
    _info "log  (CoreAudio xrun monitoring available)"
  else
    _error "log command missing — unexpected on macOS"
    (( missing++ )) || true
  fi

  # Klaar
  local pid
  pid="$(get_app_pid)"
  if [[ -n "$pid" ]]; then
    _info "$APP_NAME running  (PID $pid)"
  else
    _warn "$APP_NAME not running — start the app before running 'start'"
  fi

  echo ""
  if [[ $missing -eq 0 ]]; then
    _info "All required tools present."
  else
    _error "$missing required tool(s) missing — see above."
    return 1
  fi
}


# ══════════════════════════════════════════════════════════════════════════════
#  Heap / leaks snapshot helpers
# ══════════════════════════════════════════════════════════════════════════════

do_heap_snapshot() {
  local label="$1" out_dir="$2" pid="$3"
  local outfile="$out_dir/heap-${label}.txt"

  _step "Heap snapshot  → heap-${label}.txt"

  if heap "$pid" > "$outfile" 2>&1; then
    # Extract the summary line — format varies by macOS version
    local summary
    summary=$(grep -E "^(Total bytes|AllocationsSummary)" "$outfile" 2>/dev/null | head -1 || echo "")
    [[ -n "$summary" ]] && echo -e "     ${summary}" || true
  else
    local rc=$?
    _warn "  heap failed (exit $rc) — binary may be hardened"
    _warn "  Tip: dev builds are always profileable; release builds need"
    _warn "       the com.apple.security.get-task-allow entitlement."
    echo "heap failed: exit $rc" > "$outfile"
  fi
}

do_leaks_snapshot() {
  local label="$1" out_dir="$2" pid="$3"
  local outfile="$out_dir/leaks-${label}.txt"

  _step "Leaks check    → leaks-${label}.txt"

  leaks "$pid" > "$outfile" 2>&1
  local rc=$?

  if [[ $rc -eq 0 ]]; then
    _info "  0 leaks"
  elif [[ $rc -eq 1 ]]; then
    # leaks exits 1 when leaks are actually found
    local count
    count=$(grep -cE "^Leak" "$outfile" 2>/dev/null || echo "?")
    _warn "  ${count} leak(s) found — see leaks-${label}.txt"
  else
    _warn "  leaks failed (exit $rc) — binary may be hardened (same issue as heap)"
    echo "leaks failed: exit $rc" > "$outfile"
  fi
}


# ══════════════════════════════════════════════════════════════════════════════
#  snapshot — take a heap+leaks snapshot right now
# ══════════════════════════════════════════════════════════════════════════════

cmd_snapshot() {
  local label="${1:-$(date +%H%M%S)}"

  local pid
  pid="$(get_app_pid)"
  [[ -n "$pid" ]] || _die "$APP_NAME is not running"

  local out_dir
  if run_is_active; then
    out_dir="$(state_get RUN_DIR)"
  else
    out_dir="$STATE_BASE/snapshots"
    mkdir -p "$out_dir"
  fi

  do_heap_snapshot "$label" "$out_dir" "$pid"
  do_leaks_snapshot "$label" "$out_dir" "$pid"
}


# ══════════════════════════════════════════════════════════════════════════════
#  Background snapshot scheduler
#  Runs as a detached subshell started by `start`.
#  Sources this script to reuse snapshot helpers.
# ══════════════════════════════════════════════════════════════════════════════

_run_scheduler() {
  local run_dir="$1"
  local early_secs="$2"
  local late_secs="$3"    # seconds-from-start for the late snapshot
  local pid="$4"

  # Helper functions (do_heap_snapshot, do_leaks_snapshot) are defined earlier
  # in this file and are available in subshells forked from this process —
  # no need to re-source.

  # Wait for early snapshot
  sleep "$early_secs" || return 0
  do_heap_snapshot "t-5min"  "$run_dir" "$pid"
  do_leaks_snapshot "t-5min" "$run_dir" "$pid"

  # Wait for late snapshot (near end of test)
  local remaining=$(( late_secs - early_secs ))
  if [[ $remaining -gt 0 ]]; then
    sleep "$remaining" || return 0
    do_heap_snapshot "t-end"  "$run_dir" "$pid"
    do_leaks_snapshot "t-end" "$run_dir" "$pid"
  fi

  echo "SCHEDULER_DONE=1" >> "$run_dir/.state"
}


# ══════════════════════════════════════════════════════════════════════════════
#  start — begin the soak test
# ══════════════════════════════════════════════════════════════════════════════

cmd_start() {
  local duration=$TEST_DURATION_SECS
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --duration) duration="$2"; shift 2 ;;
      *) _die "Unknown option: $1" ;;
    esac
  done

  # Guard: app must be running
  local app_pid
  app_pid="$(get_app_pid)"
  [[ -n "$app_pid" ]] || _die "$APP_NAME is not running. Start the app first."

  # Guard: loopback device required for a meaningful soak
  if [[ -z "$TEST_INPUT_DEVICE" ]]; then
    _die "TEST_INPUT_DEVICE is unset.
  A loopback device is required so sox can inject a stable pink-noise signal
  for the full soak duration. Set it to the CoreAudio device name, e.g.:
    TEST_INPUT_DEVICE=\"BlackHole 16ch\" ./scripts/perf-test.sh start"
  fi
  if ! system_profiler SPAudioDataType 2>/dev/null | grep -q "$TEST_INPUT_DEVICE"; then
    _die "Loopback device \"$TEST_INPUT_DEVICE\" not found.
  Install BlackHole, VB-Cable, Loopback, or another loopback driver, then re-run."
  fi

  # Guard: no concurrent run
  if run_is_active; then
    local existing
    existing="$(state_get RUN_DIR)"
    _die "A test is already active in:\n  $existing\nRun 'perf-test.sh stop' first."
  fi

  # Create timestamped run directory
  mkdir -p "$STATE_BASE"
  local ts run_dir
  ts="$(date +%Y%m%d-%H%M%S)"
  run_dir="$STATE_BASE/$ts"
  mkdir -p "$run_dir"
  ln -sfn "$run_dir" "$CURRENT_LINK"

  # Write initial state
  cat > "$run_dir/.state" <<EOF
RUN_DIR=$run_dir
START_TIME=$(date +%s)
DURATION_SECS=$duration
APP_PID=$app_pid
SOX_PID=
LOG_PID=
SCHEDULER_PID=
EOF

  _section "Starting Soak Test"
  echo -e "  ${W}Run directory:${N}  $run_dir"
  echo -e "  ${W}Duration:${N}       $(( duration / 60 )) minutes"
  echo -e "  ${W}$APP_NAME PID:${N}  $app_pid"
  echo ""

  # ── 1. Baseline heap snapshot ──────────────────────────────────────────────
  _step "Taking baseline heap snapshot..."
  do_heap_snapshot "t-start" "$run_dir" "$app_pid"

  # ── 2. CoreAudio xrun log capture ─────────────────────────────────────────
  _step "Starting CoreAudio log capture..."
  local log_file="$run_dir/coreaudio.log"

  log stream \
    --predicate 'subsystem == "com.apple.coreaudio"' \
    --level debug \
    >> "$log_file" 2>&1 &
  local log_pid=$!
  disown "$log_pid" 2>/dev/null || true
  state_set LOG_PID "$log_pid"
  _info "  CoreAudio log capture running  (PID $log_pid)  → coreaudio.log"

  # ── 3. Test signal via sox → loopback device ──────────────────────────────
  # The Klaar driver itself can't serve here: the engine writes to it, so a
  # parallel sox writer would collide. A separate loopback (BlackHole,
  # Loopback, etc.) declared via TEST_INPUT_DEVICE is required.
  _step "Starting pink noise test signal → \"$TEST_INPUT_DEVICE\"..."
  local signal_dur=$(( duration + 120 ))  # run slightly past end of test
  sox -n -t coreaudio "$TEST_INPUT_DEVICE" \
    synth "$signal_dur" pinknoise vol 0.3 \
    >> "$run_dir/sox.log" 2>&1 &
  local sox_pid=$!
  disown "$sox_pid" 2>/dev/null || true
  state_set SOX_PID "$sox_pid"
  _info "  Pink noise running  (PID $sox_pid)"
  _warn "  Set Klaar's input device to \"$TEST_INPUT_DEVICE\" if not already done."

  # ── 4. Background snapshot scheduler ──────────────────────────────────────
  local late_snap_secs=$(( duration - EARLY_SNAPSHOT_SECS ))
  [[ $late_snap_secs -le $EARLY_SNAPSHOT_SECS ]] && late_snap_secs=$(( duration - 60 ))

  _step "Scheduling heap snapshots at t=5min and t=$(( duration / 60 ))min..."
  (
    _run_scheduler "$run_dir" "$EARLY_SNAPSHOT_SECS" "$late_snap_secs" "$app_pid"
  ) &
  local sched_pid=$!
  disown "$sched_pid" 2>/dev/null || true
  state_set SCHEDULER_PID "$sched_pid"
  _info "  Snapshot scheduler running  (PID $sched_pid)"

  # ── Summary ────────────────────────────────────────────────────────────────
  echo ""
  echo -e "  ${G}Soak test is running.${N} Leave it for $(( duration / 60 )) minutes."
  echo ""
  echo -e "  While it runs you can:"
  echo -e "    ${C}./scripts/perf-test.sh trace${N}     — capture 30s Instruments CPU trace"
  echo -e "    ${C}./scripts/perf-test.sh snapshot${N}  — take an extra heap snapshot"
  echo -e "    ${C}./scripts/perf-test.sh status${N}    — check progress and process health"
  echo ""
  echo -e "  When done (or early):"
  echo -e "    ${C}./scripts/perf-test.sh stop${N}      — stop everything and generate report"
  echo ""
}


# ══════════════════════════════════════════════════════════════════════════════
#  stop — end test, take final snapshot, generate report
# ══════════════════════════════════════════════════════════════════════════════

cmd_stop() {
  run_is_active || _die "No active test found. Nothing to stop."

  local run_dir
  run_dir="$(state_get RUN_DIR)"

  _section "Stopping Soak Test"

  # Kill scheduler
  local sched_pid
  sched_pid="$(state_get SCHEDULER_PID)"
  if [[ -n "$sched_pid" ]] && kill -0 "$sched_pid" 2>/dev/null; then
    kill "$sched_pid" 2>/dev/null || true
    _info "Snapshot scheduler stopped"
  fi

  # Kill log stream
  local log_pid
  log_pid="$(state_get LOG_PID)"
  if [[ -n "$log_pid" ]] && kill -0 "$log_pid" 2>/dev/null; then
    kill "$log_pid" 2>/dev/null || true
    _info "CoreAudio log capture stopped"
  fi

  # Kill sox
  local sox_pid
  sox_pid="$(state_get SOX_PID)"
  if [[ -n "$sox_pid" ]] && kill -0 "$sox_pid" 2>/dev/null; then
    kill "$sox_pid" 2>/dev/null || true
    _info "Test signal (sox) stopped"
  fi

  # Final heap + leaks if app is still up
  local current_pid
  current_pid="$(get_app_pid)"
  if [[ -n "$current_pid" ]]; then
    echo ""
    _step "Taking final snapshots..."
    do_heap_snapshot  "t-final" "$run_dir" "$current_pid"
    do_leaks_snapshot "t-final" "$run_dir" "$current_pid"
  else
    _warn "$APP_NAME is no longer running — skipping final snapshot"
  fi

  # Mark run as complete — write directly to run_dir state file
  # (don't rely on symlink which we're about to remove)
  echo "END_TIME=$(date +%s)" >> "$run_dir/.state"
  rm -f "$CURRENT_LINK"

  echo ""
  _info "Test complete."
  echo ""

  cmd_report "$run_dir"
}


# ══════════════════════════════════════════════════════════════════════════════
#  trace — attach Instruments Time Profiler to running app
# ══════════════════════════════════════════════════════════════════════════════

cmd_trace() {
  local duration=$TRACE_DURATION_SECS
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --duration) duration="$2"; shift 2 ;;
      *) _die "Unknown option: $1" ;;
    esac
  done

  local pid
  pid="$(get_app_pid)"
  [[ -n "$pid" ]] || _die "$APP_NAME is not running"

  command -v xctrace &>/dev/null || xcrun --find xctrace &>/dev/null \
    || _die "xctrace not found (install Xcode Command Line Tools)"

  local out_dir
  if run_is_active; then
    out_dir="$(state_get RUN_DIR)"
  else
    out_dir="$STATE_BASE/traces"
    mkdir -p "$out_dir"
  fi

  local trace_file="$out_dir/cpu-$(date +%H%M%S).trace"

  _section "CPU Trace  (${duration}s)"
  echo -e "  Attaching to $APP_NAME  (PID $pid)..."
  echo -e "  Output: $trace_file"
  echo -e "  ${Y}Note: may prompt for authorization.${N}"
  echo ""

  if xctrace record \
      --template "Time Profiler" \
      --attach "$pid" \
      --time-limit "${duration}000ms" \
      --output "$trace_file" 2>&1; then
    echo ""
    _info "Trace captured: ${trace_file##*/}"
    echo ""
    echo -e "  Open in Instruments:"
    echo -e "    ${C}open \"$trace_file\"${N}"
    echo ""
    echo -e "  ${W}In Instruments:${N}"
    echo -e "    1. Select the Call Tree view"
    echo -e "    2. Filter by thread — look for HALIOThread or the thread with steady CPU"
    echo -e "    3. That thread is your audio callback — its call tree shows where time goes"
  else
    local rc=$?
    echo ""
    _warn "xctrace exited $rc"
    _warn "Common causes:"
    _warn "  • Hardened binary (release build) — try the dev build instead: pnpm tauri dev"
    _warn "  • SIP blocking attachment — check System Preferences → Privacy & Security"
    _warn "  • Xcode tools out of date — try: xcode-select --install"
  fi
}


# ══════════════════════════════════════════════════════════════════════════════
#  report — analyse collected data and print summary
# ══════════════════════════════════════════════════════════════════════════════

cmd_report() {
  local run_dir="${1:-}"

  # Resolve run directory
  if [[ -z "$run_dir" ]]; then
    if run_is_active; then
      run_dir="$(state_get RUN_DIR)"
    else
      # Most recently modified completed run dir
      run_dir="$(ls -td "$STATE_BASE"/[0-9]* 2>/dev/null | head -1 || true)"
    fi
  fi

  [[ -n "$run_dir" && -d "$run_dir" ]] \
    || _die "No run directory found. Run a test first, or pass a path: report /tmp/klaar-perf/20260409-120000"

  local state_file="$run_dir/.state"
  [[ -f "$state_file" ]] || _die "No .state file in $run_dir"

  _section "Performance Report"
  echo -e "  ${W}Run dir:${N}  $run_dir"

  # Timing — grep the state file directly (the symlink may have been removed by stop)
  local start_ts end_ts
  start_ts="$(grep "^START_TIME=" "$state_file" | tail -1 | cut -d= -f2)"
  end_ts="$(grep "^END_TIME=" "$state_file" 2>/dev/null | tail -1 | cut -d= -f2 || true)"

  if [[ -n "$end_ts" ]]; then
    local elapsed=$(( end_ts - start_ts ))
    echo -e "  ${W}Duration:${N} $(( elapsed / 60 ))m $(( elapsed % 60 ))s"
  else
    local now elapsed
    now=$(date +%s)
    elapsed=$(( now - start_ts ))
    echo -e "  ${W}Status:${N}   ${Y}still running${N}  ($(( elapsed / 60 ))m elapsed)"
  fi

  # ── 1. Xrun analysis ────────────────────────────────────────────────────────
  _section "P1 · Audio Callback Overruns"
  local log_file="$run_dir/coreaudio.log"
  if [[ -f "$log_file" ]]; then
    local lines xruns
    lines=$(wc -l < "$log_file" | tr -d ' ')
    xruns=$(grep -ciE "overload|IOProc took too long|missed deadline" "$log_file" 2>/dev/null || echo 0)

    echo -e "  CoreAudio log entries:  $lines"

    if [[ "$xruns" -eq 0 ]]; then
      _info "Xruns:  0    ✓ PASS"
    else
      _error "Xruns:  $xruns    ← investigate"
      echo ""
      echo -e "  ${Y}First matching log lines:${N}"
      grep -iE "overload|IOProc took too long|missed deadline" "$log_file" \
        2>/dev/null | head -5 | sed 's/^/    /' || true
    fi
  else
    _warn "coreaudio.log not found (log capture may have failed)"
  fi

  # ── 2. Memory growth ────────────────────────────────────────────────────────
  _section "P2 · Memory Growth (heap snapshots)"
  heap_files=()
  while IFS= read -r f; do heap_files+=("$f"); done < <(ls -tr "$run_dir"/heap-*.txt 2>/dev/null)

  if [[ ${#heap_files[@]} -eq 0 ]]; then
    _warn "No heap snapshots found"
  elif [[ ${#heap_files[@]} -eq 1 ]]; then
    _warn "Only one snapshot — need at least two to detect growth"
    echo -e "  ${heap_files[0]##*/}:"
    grep -E "Total" "${heap_files[0]}" 2>/dev/null | head -3 | sed 's/^/    /' || true
  else
    local first="${heap_files[0]}"
    local last="${heap_files[${#heap_files[@]}-1]}"
    echo -e "  Comparing:  ${first##*/}  →  ${last##*/}"
    echo ""

    # Extract total bytes — try several patterns for different macOS versions:
    #   macOS 15+:  "All zones: 71995 nodes (13737240 bytes)"
    #   older:      "71995 nodes for 13737240 bytes"
    #   older:      "Total bytes: 13737240"
    _extract_heap_bytes() {
      local file="$1"
      grep -oE '\([0-9]+ bytes\)' "$file" 2>/dev/null | grep -oE '[0-9]+' | head -1 \
        || grep -oE 'for [0-9]+ bytes' "$file" 2>/dev/null | grep -oE '[0-9]+' | head -1 \
        || grep -oE 'Total bytes: [0-9]+' "$file" 2>/dev/null | grep -oE '[0-9]+' | head -1 \
        || echo ""
    }
    local first_bytes last_bytes
    first_bytes=$(_extract_heap_bytes "$first")
    last_bytes=$(_extract_heap_bytes "$last")

    if [[ -n "$first_bytes" && -n "$last_bytes" ]]; then
      local delta=$(( last_bytes - first_bytes ))
      local first_mb last_mb delta_mb
      first_mb=$(awk "BEGIN{printf \"%.1f\", $first_bytes/1048576}")
      last_mb=$(awk "BEGIN{printf \"%.1f\", $last_bytes/1048576}")
      delta_mb=$(awk "BEGIN{printf \"%.2f\", $delta/1048576}")

      echo -e "  Baseline:  ${W}${first_mb} MB${N}"
      echo -e "  Final:     ${W}${last_mb} MB${N}"

      if [[ $delta -lt 1048576 ]]; then   # < 1 MB growth
        _info "Growth:    ${delta_mb} MB    ✓ within normal range"
      else
        _warn "Growth:    ${delta_mb} MB    ← worth investigating"
        echo -e "  ${Y}Check Instruments Allocations template for the source.${N}"
      fi
    else
      _warn "Could not parse byte counts (heap may have failed — see files)"
      echo -e "  ${Y}Snapshot files:${N}"
      for f in "${heap_files[@]}"; do
        echo -e "    ${f##*/}:  $(head -3 "$f" | tr '\n' ' ')"
      done
    fi
  fi

  # ── 3. Leak checks ──────────────────────────────────────────────────────────
  _section "P2 · Leak Checks"
  leaks_files=()
  while IFS= read -r f; do leaks_files+=("$f"); done < <(ls "$run_dir"/leaks-*.txt 2>/dev/null | sort)

  if [[ ${#leaks_files[@]} -eq 0 ]]; then
    _warn "No leak snapshots found"
  else
    local any_leaks=false
    for lf in "${leaks_files[@]}"; do
      local label="${lf##*/leaks-}"; label="${label%.txt}"
      local summary
      summary=$(grep -E "leaks for|^Process" "$lf" 2>/dev/null | tail -1 || \
                head -1 "$lf" 2>/dev/null || echo "see file")
      if grep -q "failed" "$lf" 2>/dev/null; then
        _warn "  $label: skipped (hardened binary)"
      elif grep -qiE "^Leak|[1-9][0-9]* leak" "$lf" 2>/dev/null; then
        _error "  $label: $summary"
        any_leaks=true
      else
        _info  "  $label: $summary"
      fi
    done
    $any_leaks && echo -e "\n  ${Y}Run: open the .memgraph or rerun with Instruments Leaks template${N}" || true
  fi

  # ── 4. CPU traces ───────────────────────────────────────────────────────────
  _section "P3 · CPU Traces"
  trace_files=()
  while IFS= read -r f; do trace_files+=("$f"); done < <(ls -d "$run_dir"/*.trace 2>/dev/null)

  if [[ ${#trace_files[@]} -eq 0 ]]; then
    _warn "No traces captured during this run"
    echo -e "  Run this during a future test:"
    echo -e "    ${C}./scripts/perf-test.sh trace${N}"
  else
    for tf in "${trace_files[@]}"; do
      _info "${tf##*/}"
      echo -e "    ${C}open \"$tf\"${N}"
    done
  fi

  # ── Files manifest ──────────────────────────────────────────────────────────
  _section "All Files"
  ls -lh "$run_dir/" 2>/dev/null | grep -v "^total" | sed 's/^/  /' || true

  echo ""
}


# ══════════════════════════════════════════════════════════════════════════════
#  status — show current test progress
# ══════════════════════════════════════════════════════════════════════════════

cmd_status() {
  if ! run_is_active; then
    echo "No active soak test."
    echo ""
    echo "Start one:  ./scripts/perf-test.sh start"
    return
  fi

  local run_dir start_ts duration_secs
  run_dir="$(state_get RUN_DIR)"
  start_ts="$(state_get START_TIME)"
  duration_secs="$(state_get DURATION_SECS)"

  local now elapsed remaining
  now=$(date +%s)
  elapsed=$(( now - start_ts ))
  remaining=$(( duration_secs - elapsed ))

  _section "Soak Test Status"
  echo -e "  ${W}Run dir:${N}    $run_dir"
  echo -e "  ${W}Elapsed:${N}    $(( elapsed / 60 ))m $(( elapsed % 60 ))s"
  if [[ $remaining -gt 0 ]]; then
    echo -e "  ${W}Remaining:${N}  $(( remaining / 60 ))m $(( remaining % 60 ))s"
    # Simple ASCII progress bar (40 chars wide)
    local pct=$(( elapsed * 40 / duration_secs ))
    local bar
    bar="$(printf '%*s' "$pct" | tr ' ' '█')$(printf '%*s' $(( 40 - pct )) | tr ' ' '░')"
    echo -e "  ${W}Progress:${N}   [${bar}] $(( elapsed * 100 / duration_secs ))%%"
  else
    echo -e "  ${W}Status:${N}     ${Y}overtime — run 'perf-test.sh stop'${N}"
  fi

  # Process health checks
  echo ""
  local app_pid sox_pid log_pid sched_pid
  app_pid="$(state_get APP_PID)"
  sox_pid="$(state_get SOX_PID)"
  log_pid="$(state_get LOG_PID)"
  sched_pid="$(state_get SCHEDULER_PID)"

  _check_process() {
    local name="$1" pid="$2"
    if [[ -z "$pid" ]]; then
      _warn "$name  (not started)"
    elif kill -0 "$pid" 2>/dev/null; then
      _info "$name  (PID $pid — running)"
    else
      _error "$name  (PID $pid — DEAD)"
    fi
  }

  _check_process "$APP_NAME" "$app_pid"
  _check_process "Test signal (sox)" "$sox_pid"
  _check_process "CoreAudio log capture" "$log_pid"
  _check_process "Snapshot scheduler" "$sched_pid"

  # Snapshot count
  echo ""
  local snap_count trace_count xruns log_lines
  snap_count=$(ls "$run_dir"/heap-*.txt 2>/dev/null | wc -l | tr -d ' ')
  trace_count=$(ls "$run_dir"/*.trace 2>/dev/null | wc -l | tr -d ' ')

  echo -e "  ${W}Heap snapshots:${N}  $snap_count"
  echo -e "  ${W}CPU traces:${N}      $trace_count"

  local log_file="$run_dir/coreaudio.log"
  if [[ -f "$log_file" ]]; then
    log_lines=$(wc -l < "$log_file" | tr -d ' ')
    xruns=$(grep -ciE "overload|IOProc took too long|missed deadline" "$log_file" 2>/dev/null || echo 0)
    echo -e "  ${W}CoreAudio log:${N}   $log_lines lines, ${xruns} xrun(s) so far"
  fi

  echo ""
  echo -e "  Available now:"
  echo -e "    ${C}./scripts/perf-test.sh trace${N}     — capture 30s CPU trace"
  echo -e "    ${C}./scripts/perf-test.sh snapshot${N}  — take extra heap snapshot"
  echo -e "    ${C}./scripts/perf-test.sh stop${N}      — finish and generate report"
}


# ══════════════════════════════════════════════════════════════════════════════
#  help
# ══════════════════════════════════════════════════════════════════════════════

cmd_help() {
  cat <<EOF

${W}Klaar Performance Test Suite  v${SCRIPT_VERSION}${N}

${W}USAGE${N}
  ./scripts/perf-test.sh <command> [options]

${W}COMMANDS${N}
  ${C}check${N}                  Verify all prerequisites
  ${C}start${N} [--duration N]   Start 2-hour soak test (N = seconds, default 7200)
                          Klaar must already be running.
  ${C}stop${N}                   Stop all background processes, generate report
  ${C}trace${N} [--duration N]   Capture N-second CPU trace via Instruments (default 30s)
  ${C}snapshot${N} [LABEL]       Take heap + leaks snapshot immediately
  ${C}report${N} [RUN_DIR]       Generate/re-generate report for a run directory
  ${C}status${N}                 Show current test progress and process health

${W}TYPICAL WORKFLOW${N}
  1. Start Klaar (pnpm tauri dev  or  open the .app)
  2. ${C}./scripts/perf-test.sh check${N}
  3. ${C}./scripts/perf-test.sh start${N}
  4. ${C}./scripts/perf-test.sh trace${N}   ← any time, grab a 30s CPU trace
  5. Wait 2 hours
  6. ${C}./scripts/perf-test.sh stop${N}    ← stops everything, prints full report

${W}ENVIRONMENT${N}
  TEST_DURATION_SECS    Soak duration in seconds (default: 7200)
  TRACE_DURATION_SECS   CPU trace capture length (default: 30)
  EARLY_SNAPSHOT_SECS   When to take first heap snapshot (default: 300 = 5min)
  PERF_STATE_BASE       Output base directory (default: /tmp/klaar-perf)
  APP_NAME              Process name to find (default: Klaar)
  DRIVER_DEVICE         Klaar HAL driver device name (default: "Klaar")
  TEST_INPUT_DEVICE     Loopback device for sox pink-noise injection
                        (no default — required). Examples: "BlackHole 16ch",
                        "VB-Cable", "Loopback Audio". The soak needs a stable
                        signal, not whatever ambient noise a real mic picks
                        up over 2 hours. The Klaar driver itself cannot be
                        used here — the engine writes to
                        it and a parallel sox writer would collide.

${W}OUTPUT FILES${N}
  All data → /tmp/klaar-perf/<YYYYMMDD-HHMMSS>/

  heap-t-start.txt     Baseline memory snapshot
  heap-t-5min.txt      Snapshot at 5 minutes
  heap-t-end.txt       Snapshot near end of test
  heap-t-final.txt     Snapshot when stop is called
  leaks-*.txt          Corresponding leak checks
  coreaudio.log        CoreAudio xrun log (grep for "overload")
  cpu-<time>.trace     Instruments Time Profiler capture (open in Xcode)
  sox.log              Test signal output / errors

EOF
}


# ══════════════════════════════════════════════════════════════════════════════
#  Entry point
# ══════════════════════════════════════════════════════════════════════════════

# When sourced by the scheduler subshell, skip main()
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  set -euo pipefail

  cmd="${1:-help}"
  shift || true

  case "$cmd" in
    check)    cmd_check    "$@" ;;
    start)    cmd_start    "$@" ;;
    stop)     cmd_stop     "$@" ;;
    trace)    cmd_trace    "$@" ;;
    snapshot) cmd_snapshot "$@" ;;
    report)   cmd_report   "$@" ;;
    status)   cmd_status   "$@" ;;
    help|--help|-h) cmd_help ;;
    *) _error "Unknown command: $cmd"; echo ""; cmd_help; exit 1 ;;
  esac
fi
