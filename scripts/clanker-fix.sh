#!/usr/bin/env bash
# clanker-fix — interactive macOS permission/troubleshooting helper for
# ClankerClocker. Installed automatically to ~/.local/bin/clanker-fix on
# first launch of the app (see src-tauri/src/lib.rs, install_clanker_fix_cli)
# — this file is the source of truth; edit it here, not the installed copy,
# since the app overwrites the installed copy on every launch.
#
# See docs/macos-troubleshooting.md for why this is needed at all (unsigned
# build => TCC grants don't survive updates).
#
# Usage:
#   clanker-fix                 interactive menu
#   clanker-fix --relaunch      de-quarantine + reopen the app
#   clanker-fix --reset-tcc     reset this app's TCC grants (last resort)
#   clanker-fix --open-privacy  open the relevant Privacy & Security panes

set -euo pipefail

APP_NAME="ClankerClocker.app"
APP_PROC_NAME="ClankerClocker"
BUNDLE_ID="com.arcodify.clankerclocker"

find_app() {
  local candidate
  for candidate in "/Applications/${APP_NAME}" "${HOME}/Applications/${APP_NAME}"; do
    if [[ -d "$candidate" ]]; then
      echo "$candidate"
      return 0
    fi
  done
  return 1
}

relaunch() {
  local app_path
  if ! app_path=$(find_app); then
    echo "Could not find ${APP_NAME} in /Applications or ~/Applications." >&2
    echo "Install it first (drag it into Applications), then try again." >&2
    return 1
  fi
  echo "Found ${APP_NAME} at ${app_path}"

  echo "Quitting ${APP_PROC_NAME} if it's running..."
  osascript -e "tell application \"${APP_PROC_NAME}\" to quit" >/dev/null 2>&1 || true
  sleep 1
  pkill -x "${APP_PROC_NAME}" >/dev/null 2>&1 || true

  echo "Removing quarantine / extended attributes..."
  xattr -cr "$app_path"

  echo "Launching ${APP_NAME}..."
  open "$app_path"

  cat <<'EOF'

Grant every permission ClankerClocker asks for now:
  - Accessibility
  - Apple Events (Automation)
  - Input Monitoring / "Listen Event", if prompted

Then quit ClankerClocker completely and open it again — permissions
sometimes only take effect after a fresh launch.
EOF
}

reset_tcc() {
  echo
  echo "This resets ClankerClocker's own TCC permission grants only"
  echo "(scoped to ${BUNDLE_ID} — no other app's permissions are affected)."
  read -r -p "Continue? [y/N] " reply
  if [[ ! "$reply" =~ ^[Yy]$ ]]; then
    echo "Cancelled."
    return 0
  fi

  echo "You'll be asked for your administrator password."
  sudo tccutil reset ListenEvent "$BUNDLE_ID"
  sudo tccutil reset Accessibility "$BUNDLE_ID"
  sudo tccutil reset AppleEvents "$BUNDLE_ID"

  echo
  echo "Done. macOS keeps an in-memory cache of these grants, so a reboot is"
  echo "strongly recommended before ClankerClocker will re-prompt correctly."
  read -r -p "Reboot now? [y/N] " reply2
  if [[ "$reply2" =~ ^[Yy]$ ]]; then
    echo "Rebooting..."
    sudo shutdown -r now
  else
    echo "Skipped reboot — restart the Mac manually before your next launch attempt."
  fi
}

open_privacy_panes() {
  echo "Opening Privacy & Security settings..."
  open "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
  sleep 1
  open "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation"
  sleep 1
  open "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
}

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "clanker-fix is only for macOS." >&2
  exit 1
fi

case "${1:-}" in
  --relaunch) relaunch; exit $? ;;
  --reset-tcc) reset_tcc; exit $? ;;
  --open-privacy) open_privacy_panes; exit $? ;;
  -h|--help)
    echo "Usage: clanker-fix [--relaunch|--reset-tcc|--open-privacy]"
    echo "Run with no arguments for an interactive menu."
    exit 0
    ;;
  "") ;; # fall through to menu
  *)
    echo "Unknown option: $1" >&2
    echo "Usage: clanker-fix [--relaunch|--reset-tcc|--open-privacy]" >&2
    exit 1
    ;;
esac

while true; do
  cat <<'EOF'

ClankerClocker permission troubleshooter
=========================================
  1) Relaunch app (de-quarantine + reopen)
  2) Reset permissions (last resort — asks for admin password)
  3) Open Privacy & Security settings
  4) Exit
EOF
  read -r -p "Choose an option [1-4]: " choice
  case "$choice" in
    1) relaunch ;;
    2) reset_tcc ;;
    3) open_privacy_panes ;;
    4) exit 0 ;;
    *) echo "Please enter 1, 2, 3, or 4." ;;
  esac
done
