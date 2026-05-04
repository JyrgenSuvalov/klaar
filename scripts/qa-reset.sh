#!/usr/bin/env bash
# Reset this Mac to a "never-launched-Klaar" state for QA scenarios.
#
# Wipes per-user app state (profile dir, webview storage, prefs, saved state,
# logs), the system-wide driver bundle, and the TCC microphone-permission
# entry so the next launch behaves identically to a fresh-install run on a
# brand-new user account.
#
# Equivalent to creating a new local user purely for QA, minus the login
# friction. Used to drive QA scenarios (fresh install, cancel-admin during
# install, mid-session driver removal, stress reruns, etc.) without
# accumulating state between iterations.
#
# Usage:
#   bash scripts/qa-reset.sh                # full wipe, prompts for confirm
#   bash scripts/qa-reset.sh --yes          # skip confirmation
#   bash scripts/qa-reset.sh --keep-driver  # leave the .driver bundle alone
#                                             (use for version-gate tests)
#   bash scripts/qa-reset.sh --keep-tcc     # leave mic permission granted
#                                             (skip the mic-prompt scenario)
#   bash scripts/qa-reset.sh --keep-app     # leave /Applications/Klaar.app
#                                             in place (skip the re-install
#                                             scenario)
#
# Driver removal and /Applications removal may need sudo; you'll be prompted
# once. The TCC reset uses `tccutil` which is unprivileged.

set -euo pipefail

BUNDLE_ID="eu.jyrkki.Klaar"
DRIVER_PATH="/Library/Audio/Plug-Ins/HAL/Klaar.driver"
APP_PATH="/Applications/Klaar.app"

CONFIRM=1
KEEP_DRIVER=0
KEEP_TCC=0
KEEP_APP=0

for arg in "$@"; do
  case "$arg" in
    --yes|-y) CONFIRM=0 ;;
    --keep-driver) KEEP_DRIVER=1 ;;
    --keep-tcc) KEEP_TCC=1 ;;
    --keep-app) KEEP_APP=1 ;;
    -h|--help)
      sed -n '2,/^set -/p' "$0" | sed 's/^# \{0,1\}//' | head -n -1
      exit 0
      ;;
    *)
      echo "[qa-reset] unknown arg: $arg" >&2
      exit 2
      ;;
  esac
done

echo "[qa-reset] target bundle: $BUNDLE_ID"
echo "[qa-reset] will remove:"
[[ $KEEP_DRIVER -eq 0 ]] && echo "  - $DRIVER_PATH (sudo)"
[[ $KEEP_APP -eq 0 ]]    && echo "  - $APP_PATH (sudo if needed)"
echo "  - ~/Library/Application Support/$BUNDLE_ID"
echo "  - ~/Library/WebKit/$BUNDLE_ID"
echo "  - ~/Library/Caches/$BUNDLE_ID"
echo "  - ~/Library/Preferences/$BUNDLE_ID.plist"
echo "  - ~/Library/Saved Application State/$BUNDLE_ID.savedState"
echo "  - ~/Library/Logs/Klaar"
[[ $KEEP_TCC -eq 0 ]] && echo "  - TCC microphone grant for $BUNDLE_ID"
[[ $KEEP_APP -eq 0 ]] && echo "  - LaunchServices Gatekeeper-approval cache for $BUNDLE_ID (path + cdhash)"
echo

if [[ $CONFIRM -eq 1 ]]; then
  read -r -p "Proceed? [y/N] " ans
  case "$ans" in
    y|Y|yes|YES) ;;
    *) echo "[qa-reset] aborted."; exit 1 ;;
  esac
fi

# 1. Quit the app if it's running so file handles don't pin anything.
#    `osascript` returns non-zero if the app isn't running — swallow.
osascript -e 'tell application "Klaar" to quit' >/dev/null 2>&1 || true
# Belt-and-braces: a hard kill in case the menu-bar process is wedged.
pkill -x Klaar >/dev/null 2>&1 || true

# 2. /Applications/Klaar.app removal + Gatekeeper-approval cache flush.
#    Without this, fresh-install reruns are not actually fresh: the prior bundle
#    is still in /Applications with stale quarantine state, and macOS
#    LaunchServices remembers any "Open Anyway" decision the user made
#    last time, suppressing the Gatekeeper prompt on the next launch.
if [[ $KEEP_APP -eq 0 ]]; then
  if [[ -d "$APP_PATH" ]]; then
    # Drop the Gatekeeper assessment record FIRST, while the bundle still
    # exists on disk — `spctl --remove` resolves the bundle's cdhash from
    # the path it's given, so calling it after `rm -rf` is a no-op (the
    # original script's bug: assessment record stayed cached → no
    # Gatekeeper prompt on re-install).
    sudo spctl --remove "$APP_PATH" >/dev/null 2>&1 || true

    # Try without sudo first — admin users own /Applications by default.
    # Fall back to sudo if the bundle is root-owned for any reason.
    if ! rm -rf "$APP_PATH" 2>/dev/null; then
      echo "[qa-reset] $APP_PATH not user-removable; retrying with sudo"
      sudo rm -rf "$APP_PATH"
    fi
  else
    echo "[qa-reset] app already absent at $APP_PATH"
  fi

  # Belt-and-braces: also wipe syspolicyd's cdhash-keyed approval cache.
  # `spctl --remove <path>` only clears the path-keyed entry; the
  # ExecPolicy DB independently remembers "user clicked Open Anyway" by
  # cdhash, which survives a path wipe and suppresses Gatekeeper on the
  # next install of the same signed bundle. This is per-team-id /
  # cdhash, so we scope the delete to entries that mention our bundle id
  # to avoid nuking unrelated approvals.
  #
  # SQLite write requires sudo. Failures here are non-fatal (e.g. SIP
  # locked the DB on a managed Mac) — print a warning and continue.
  if sudo sqlite3 /var/db/SystemPolicyConfiguration/ExecPolicy \
       "DELETE FROM policy_scan_cache WHERE filepath LIKE '%/Klaar.app%' OR filepath LIKE '%${BUNDLE_ID}%';" \
       >/dev/null 2>&1; then
    echo "[qa-reset] cleared syspolicyd ExecPolicy cache entries for Klaar"
  else
    echo "[qa-reset] WARN: could not clear ExecPolicy DB (SIP-locked?). Gatekeeper may stay silent on re-install." >&2
  fi
fi

# 3. System-wide driver removal. Skipped on --keep-driver.
if [[ $KEEP_DRIVER -eq 0 ]]; then
  if [[ -d "$DRIVER_PATH" ]]; then
    echo "[qa-reset] removing driver (sudo) — you may be prompted for your password"
    sudo rm -rf "$DRIVER_PATH"
    # Restart coreaudiod so its HAL plug-in cache is rescanned and the
    # Klaar device disappears from system enumeration.
    #
    # Strategy: try `launchctl kickstart` first — it's the cleaner reload
    # path. SIP blocks it on system services on most modern Macs (returns
    # 150: Operation not permitted), so fall back to `killall coreaudiod`
    # under launchd's auto-respawn.
    #
    # `killall` is normally dangerous here: if a malformed
    # `.driver` bundle is sitting in HAL when coreaudiod respawns, the
    # rescan can peg CPU. That pathology requires a broken bundle to be
    # PRESENT, though — and we just deleted the only one we care about
    # one line above. The HAL dir is now in a known-clean state, so
    # respawning coreaudiod is safe.
    if ! sudo launchctl kickstart -k system/com.apple.audio.coreaudiod 2>/dev/null; then
      echo "[qa-reset] launchctl kickstart blocked by SIP — falling back to killall (safe: HAL is clean)"
      sudo killall coreaudiod || true
    fi
  else
    echo "[qa-reset] driver already absent at $DRIVER_PATH"
  fi
fi

# 4. Per-user app state. `rm -rf` on missing paths is a no-op, so we don't
#    need to test for existence first.
#
# IMPORTANT: Tauri's AppData uses the bundle identifier ($BUNDLE_ID), NOT
# the productName ("Klaar"). The previous "Application Support/Klaar" path
# was a no-op — it left `onboarding-state.json` (and its `reboot_pending`
# flag) on disk, so the next launch would seed `reboot_pending=true` via
# `seed_reboot_pending_from_disk()` and the tray would claim the driver
# needed a reboot even on a fresh install.
# Logs is the one outlier that uses productName, not bundle id.
rm -rf "$HOME/Library/Application Support/$BUNDLE_ID"
rm -rf "$HOME/Library/WebKit/$BUNDLE_ID"
rm -rf "$HOME/Library/Caches/$BUNDLE_ID"
rm -f  "$HOME/Library/Preferences/$BUNDLE_ID.plist"
rm -rf "$HOME/Library/Saved Application State/$BUNDLE_ID.savedState"
rm -rf "$HOME/Library/Logs/Klaar"

# `defaults` keeps an in-memory cfprefsd cache that can re-materialise the
# plist after a plain `rm`. Explicit `defaults delete` evicts the cache.
defaults delete "$BUNDLE_ID" >/dev/null 2>&1 || true

# 5. TCC mic permission. Without this, macOS silently re-grants on next
#    launch and the proactive mic-permission gate (commit a73c81c) never
#    fires the system prompt.
if [[ $KEEP_TCC -eq 0 ]]; then
  tccutil reset Microphone "$BUNDLE_ID" >/dev/null 2>&1 || true
fi

echo
echo "[qa-reset] Done. Mac is back to a never-launched-Klaar state."
[[ $KEEP_DRIVER -eq 1 ]] && echo "[qa-reset] driver bundle was preserved (--keep-driver)."
[[ $KEEP_TCC -eq 1 ]]    && echo "[qa-reset] TCC mic grant was preserved (--keep-tcc)."
[[ $KEEP_APP -eq 1 ]]    && echo "[qa-reset] app bundle was preserved (--keep-app)."
