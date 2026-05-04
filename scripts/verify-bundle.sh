#!/usr/bin/env bash
# Verify a macOS bundle (`.driver` or `.app`) is shippable.
#
# Runs four checks against the bundle's main Mach-O binary:
#   1. Mach-O slice type — drivers must be MH_BUNDLE on every slice (CoreAudio
#      HAL requirement), apps must be executables.
#   2. Architecture coverage — when KLAAR_DRIVER_UNIVERSAL=1 is set in the
#      environment, both x86_64 and arm64 must be present; otherwise at least
#      the host arch must be present.
#   3. Codesign validity — `codesign --verify --deep --strict` must exit 0.
#      Catches the "unsigned .app" failure mode where Gatekeeper rejects the
#      bundle as "damaged".
#   4. Codesign diagnostic — non-fatal `codesign -dv` dump so build logs record
#      what was signed with what identity.
#
# Exits 0 on success, non-zero with a human-readable message identifying the
# failing check on the first failure. Bundle kind (driver vs app) is inferred
# from the structure of Contents/MacOS/.
#
# Usage:
#   scripts/verify-bundle.sh <path-to-bundle>
#
# Invoked automatically by:
#   - scripts/build-driver.sh (on the driver bundle, post-codesign)
#   - pnpm build:release      (on the produced .app, post-tauri-bundle)

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <path-to-bundle>" >&2
  exit 2
fi

BUNDLE="$1"

if [[ ! -d "$BUNDLE" ]]; then
  echo "ERROR: bundle not found or not a directory: $BUNDLE" >&2
  exit 2
fi

MACOS_DIR="$BUNDLE/Contents/MacOS"
if [[ ! -d "$MACOS_DIR" ]]; then
  echo "ERROR: expected $MACOS_DIR to exist (is this a macOS bundle?)" >&2
  exit 2
fi

# Pick the main executable. Both .driver and .app conventionally have exactly
# one Mach-O under Contents/MacOS/.
BINARY_PATH=""
BINARY_COUNT=0
for f in "$MACOS_DIR"/*; do
  if [[ -f "$f" ]]; then
    BINARY_PATH="$f"
    BINARY_COUNT=$((BINARY_COUNT + 1))
  fi
done

if [[ $BINARY_COUNT -eq 0 ]]; then
  echo "ERROR: no Mach-O binary found in $MACOS_DIR" >&2
  exit 2
fi
if [[ $BINARY_COUNT -gt 1 ]]; then
  echo "ERROR: expected exactly 1 binary in $MACOS_DIR, found $BINARY_COUNT" >&2
  exit 2
fi

# Infer bundle kind from the file suffix. .driver is a HAL plug-in (MH_BUNDLE);
# .app is a normal macOS application (MH_EXECUTE).
BUNDLE_BASENAME="$(basename "$BUNDLE")"
case "$BUNDLE_BASENAME" in
  *.driver) BUNDLE_KIND="driver" ;;
  *.app)    BUNDLE_KIND="app" ;;
  *)
    echo "ERROR: unrecognised bundle kind: $BUNDLE_BASENAME (expected *.driver or *.app)" >&2
    exit 2
    ;;
esac

echo "[verify-bundle] target: $BUNDLE"
echo "[verify-bundle] kind:   $BUNDLE_KIND"
echo "[verify-bundle] binary: $BINARY_PATH"

# ---------------------------------------------------------------------------
# Check 1: Mach-O slice type.
# ---------------------------------------------------------------------------
# `file` emits one line per slice for fat binaries, or a single line for
# thin. For drivers, every slice must be "Mach-O 64-bit bundle". For apps,
# "Mach-O 64-bit executable".
FILE_OUTPUT="$(file "$BINARY_PATH")"

case "$BUNDLE_KIND" in
  driver) EXPECTED_TYPE="Mach-O 64-bit bundle" ;;
  app)    EXPECTED_TYPE="Mach-O 64-bit executable" ;;
esac

# Collect the per-slice lines. For a fat binary `file` prints a header like
# "Mach-O universal binary with N architectures:" followed by indented lines
# like "(for architecture x86_64): Mach-O 64-bit bundle x86_64". For a thin
# binary it's a single line. We validate that every line containing a Mach-O
# descriptor matches the expected type.
MISMATCH=0
MISMATCH_DETAILS=""
while IFS= read -r line; do
  # Skip the universal-header line and any empty lines.
  if [[ "$line" == *"universal binary"* || -z "$line" ]]; then
    continue
  fi
  # Only inspect lines that actually describe a Mach-O object.
  if [[ "$line" == *"Mach-O"* ]]; then
    if [[ "$line" != *"$EXPECTED_TYPE"* ]]; then
      MISMATCH=1
      MISMATCH_DETAILS+="  $line"$'\n'
    fi
  fi
done <<< "$FILE_OUTPUT"

if [[ $MISMATCH -ne 0 ]]; then
  echo "FAIL: Mach-O slice type check (expected '$EXPECTED_TYPE' on every slice)" >&2
  echo "      Raw \`file\` output:" >&2
  echo "$FILE_OUTPUT" | sed 's/^/        /' >&2
  echo "      Mismatching slice(s):" >&2
  printf '%s' "$MISMATCH_DETAILS" | sed 's/^/      /' >&2
  if [[ "$BUNDLE_KIND" == "driver" ]]; then
    echo "      This is the cross-compile failure mode — a slice was emitted" >&2
    echo "      as MH_DYLIB instead of MH_BUNDLE. Check" >&2
    echo "      driver/.cargo/config.toml has a linker entry for every target." >&2
  fi
  exit 1
fi
echo "[verify-bundle] ✓ slice-type: every slice is '$EXPECTED_TYPE'"

# ---------------------------------------------------------------------------
# Check 2: Architecture coverage.
# ---------------------------------------------------------------------------
LIPO_INFO="$(lipo -info "$BINARY_PATH")"
echo "[verify-bundle]   $LIPO_INFO"

if [[ "${KLAAR_DRIVER_UNIVERSAL:-0}" == "1" ]]; then
  MISSING=""
  for arch in x86_64 arm64; do
    if [[ "$LIPO_INFO" != *"$arch"* ]]; then
      MISSING+="$arch "
    fi
  done
  if [[ -n "$MISSING" ]]; then
    echo "FAIL: architecture check (KLAAR_DRIVER_UNIVERSAL=1 but missing: $MISSING)" >&2
    echo "      Raw \`lipo -info\` output:" >&2
    echo "        $LIPO_INFO" >&2
    exit 1
  fi
  echo "[verify-bundle] ✓ architectures: x86_64 + arm64 both present"
else
  HOST_ARCH="$(uname -m)"
  # uname reports "arm64" or "x86_64" on macOS, matching lipo's spelling.
  if [[ "$LIPO_INFO" != *"$HOST_ARCH"* ]]; then
    echo "FAIL: architecture check (host is $HOST_ARCH but not present in binary)" >&2
    echo "      Raw \`lipo -info\` output:" >&2
    echo "        $LIPO_INFO" >&2
    exit 1
  fi
  echo "[verify-bundle] ✓ architectures: host $HOST_ARCH present"
fi

# ---------------------------------------------------------------------------
# Check 3: Codesign validity (fatal).
# ---------------------------------------------------------------------------
# This is the check that catches an unsigned .app that Gatekeeper rejects as
# "damaged". --deep verifies nested bundles (embedded
# .driver inside the .app), --strict tightens the evaluation to match how
# Gatekeeper actually loads the bundle.
if ! CODESIGN_OUT="$(codesign --verify --deep --strict "$BUNDLE" 2>&1)"; then
  echo "FAIL: codesign --verify --deep --strict rejected the bundle" >&2
  echo "      Raw codesign output:" >&2
  echo "$CODESIGN_OUT" | sed 's/^/        /' >&2
  if [[ "$BUNDLE_KIND" == "app" ]]; then
    echo "      This is the unsigned-app failure mode — the .app was not" >&2
    echo "      codesigned. Check src-tauri/tauri.conf.json has" >&2
    echo "      bundle.macOS.signingIdentity set (at minimum '-' for ad-hoc)." >&2
  fi
  exit 1
fi
echo "[verify-bundle] ✓ codesign --verify --deep --strict: OK"

# ---------------------------------------------------------------------------
# Check 4: Hardened-runtime entitlements (apps only, fatal).
# ---------------------------------------------------------------------------
# Tauri-bundled .apps are hardened-runtime even under ad-hoc signing
# (flags=0x10002 includes the runtime bit). Without these two entitlements,
# macOS Tahoe (and later) silently rejects:
#   - AVCaptureDevice.requestAccess(.audio) — the proactive mic-permission
#     gate cannot show a TCC prompt; it just returns false instantly.
#   - open x-apple.systempreferences://...?Privacy_Microphone — the deep-link
#     lands on the wrong pane.
# Both reproduce the same TCC failure mode with a different proximate cause;
# both unblock with the entitlements file referenced from
# src-tauri/tauri.conf.json -> bundle.macOS.entitlements.
#
# Skipped for .driver bundles — they're loaded by coreaudiod and have no TCC
# surface of their own.
if [[ "$BUNDLE_KIND" == "app" ]]; then
  ENT_OUTPUT="$(codesign -d --entitlements - "$BUNDLE" 2>&1 || true)"
  MISSING_ENTS=""
  for key in com.apple.security.device.audio-input com.apple.security.automation.apple-events; do
    if [[ "$ENT_OUTPUT" != *"$key"* ]]; then
      MISSING_ENTS+="$key "
    fi
  done
  if [[ -n "$MISSING_ENTS" ]]; then
    echo "FAIL: hardened-runtime entitlements check (missing: $MISSING_ENTS)" >&2
    echo "      Raw \`codesign -d --entitlements -\` output:" >&2
    if [[ -z "$ENT_OUTPUT" ]]; then
      echo "        (empty — bundler did not embed an entitlements plist)" >&2
    else
      echo "$ENT_OUTPUT" | sed 's/^/        /' >&2
    fi
    echo "      This is the Tahoe entitlements failure mode — the" >&2
    echo "      proactive mic-permission gate cannot show a TCC prompt and" >&2
    echo "      the Privacy Settings deep-link lands on the wrong pane." >&2
    echo "      Check that src-tauri/Entitlements.plist exists and that" >&2
    echo "      src-tauri/tauri.conf.json -> bundle.macOS.entitlements" >&2
    echo "      points to it." >&2
    exit 1
  fi
  echo "[verify-bundle] ✓ entitlements: audio-input + apple-events present"
fi

# ---------------------------------------------------------------------------
# Check 5: Ad-hoc signer assertion (fatal).
# ---------------------------------------------------------------------------
# Project intent (src-tauri/tauri.conf.json -> bundle.macOS.signingIdentity:
# "-" and driver/build.rs -> codesign -s -) is ad-hoc signing. `codesign -dv`
# emits "Signature=adhoc" for ad-hoc bundles and an "Authority=..." chain for
# Developer-ID-signed bundles. Asserting Signature=adhoc here prevents an
# accidental local override (e.g. an envvar leaking a real identity into the
# bundler) from shipping in a release artifact unnoticed.
#
# If/when this project moves to a real Developer ID signature, gate this
# check behind an opt-in env var rather than deleting it.
CODESIGN_DV="$(codesign -dv "$BUNDLE" 2>&1 || true)"
if [[ "$CODESIGN_DV" != *"Signature=adhoc"* ]]; then
  echo "FAIL: ad-hoc signer assertion (Signature=adhoc not found in codesign -dv output)" >&2
  echo "      Raw \`codesign -dv\` output:" >&2
  echo "$CODESIGN_DV" | sed 's/^/        /' >&2
  echo "      Project intent is ad-hoc (signingIdentity: \"-\"). If you've" >&2
  echo "      intentionally moved to Developer ID signing, gate this check" >&2
  echo "      behind an opt-in env var in scripts/verify-bundle.sh." >&2
  exit 1
fi
echo "[verify-bundle] ✓ signer: Signature=adhoc"

# ---------------------------------------------------------------------------
# Check 6: Codesign diagnostic (non-fatal).
# ---------------------------------------------------------------------------
# Record what was signed with what identity. Helps diagnose regressions where
# the bundle verifies but was signed with the wrong identity, or where
# "Sealed Resources" mysteriously becomes "none".
echo "[verify-bundle] codesign -dv:"
echo "$CODESIGN_DV" \
  | grep -E '^(Authority|Signature|Identifier|Sealed Resources|TeamIdentifier|Format)=' \
  | sed 's/^/  /' \
  || true

echo "[verify-bundle] ✓ all checks passed for $BUNDLE"
