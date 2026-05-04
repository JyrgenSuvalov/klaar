# Performance testing

An automated soak-test harness lives at `scripts/perf-test.sh`. It monitors
audio callback overruns (xruns), memory growth, leaks, and CPU usage over
an extended run.

## Prerequisites

- **sox** — `brew install sox`
- **Klaar driver** — installed via the app's onboarding flow (or
  `install_driver` IPC during dev). The engine's processed output goes here;
  see [`developing.md`](developing.md).
- **A third-party loopback driver** — one of [BlackHole](https://existential.audio/blackhole/),
  [VB-Cable](https://vb-audio.com/Cable/), [Loopback](https://rogueamoeba.com/loopback/),
  or similar. `sox` plays continuous pink noise into this device for the
  full soak duration so Klaar gets a stable, reproducible signal.

  > **Why a separate driver?** The Klaar driver itself can't double as the
  > test-signal source — the engine writes to it, so a parallel `sox` writer
  > would collide. The loopback driver feeds the signal *into* Klaar's input;
  > Klaar's output still goes to the Klaar driver as normal.

- **Xcode** (full install, not just Command Line Tools) — required for
  `xctrace` / Instruments CPU traces:

  ```bash
  sudo xcodebuild -license accept
  sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
  ```

Verify everything is in place:

```bash
TEST_INPUT_DEVICE="BlackHole 16ch" ./scripts/perf-test.sh check
```

`TEST_INPUT_DEVICE` must match the CoreAudio device name exactly. Other
common values: `"VB-Cable"`, `"Loopback Audio"`.

## Running a soak test

1. Start Klaar:

   ```bash
   pnpm tauri dev
   ```

2. In the Klaar UI, set the **input device** to the same loopback driver
   you'll pass via `TEST_INPUT_DEVICE`. Output is hard-routed to the Klaar
   driver and needs no configuration.

3. In a second terminal, start the test (defaults to 2 hours):

   ```bash
   TEST_INPUT_DEVICE="BlackHole 16ch" ./scripts/perf-test.sh start
   ```

   For a shorter run: `./scripts/perf-test.sh start --duration 900` (15 minutes).

4. The script automatically:
   - Takes a baseline heap snapshot, plus snapshots at t+5 min and near the
     end of the run
   - Captures the CoreAudio log stream (`subsystem == com.apple.coreaudio`)
     so xruns can be counted afterward
   - Feeds continuous pink noise through `sox` into the loopback driver

5. While the test runs you can:

   ```bash
   ./scripts/perf-test.sh status     # progress + process health
   ./scripts/perf-test.sh trace      # capture a 30s CPU trace
   ./scripts/perf-test.sh snapshot   # extra heap+leaks snapshot
   ```

6. When done (or to stop early):

   ```bash
   ./scripts/perf-test.sh stop
   ```

   This kills `sox`, the log capture, and the snapshot scheduler; takes a
   final heap+leaks snapshot; and prints a report covering xruns, memory
   growth, leaks, and any captured CPU traces.

All output is written to `/tmp/klaar-perf/<YYYYMMDD-HHMMSS>/`. Re-generate
the report later with:

```bash
./scripts/perf-test.sh report /tmp/klaar-perf/<run-dir>
```

## Environment variables

| Var | Default | Notes |
|---|---|---|
| `TEST_INPUT_DEVICE` | *(required)* | CoreAudio name of the loopback driver |
| `TEST_DURATION_SECS` | `7200` | Soak duration |
| `TRACE_DURATION_SECS` | `30` | CPU trace length |
| `EARLY_SNAPSHOT_SECS` | `300` | When to take the first mid-run heap snapshot |
| `PERF_STATE_BASE` | `/tmp/klaar-perf` | Output root |
| `APP_NAME` | `Klaar` | Process name to attach to |
| `DRIVER_DEVICE` | `Klaar` | Klaar HAL device name (for the prereq check) |

## Inspecting CPU traces

`./scripts/perf-test.sh trace` captures a 30 s Instruments Time Profiler
recording. Open it with:

```bash
open /tmp/klaar-perf/<run-dir>/cpu-*.trace
```

In Instruments:

1. Select the **Call Tree** view.
2. Filter by thread — look for **HALIOThread** (the CoreAudio real-time
   callback).
3. Verify DSP processing completes well within the buffer duration (~5.3 ms
   at 256 samples / 48 kHz).
4. Confirm FFT/spectrum work runs on the IPC thread, not the audio thread.
5. Watch for unexpected `malloc` or `pthread_mutex_lock` calls on the audio
   thread — both are bugs.

> Release builds are hardened, so `heap`, `leaks`, and `xctrace --attach`
> may refuse to attach. For profiling, use `pnpm tauri dev` — the dev build
> carries the `get-task-allow` entitlement.
