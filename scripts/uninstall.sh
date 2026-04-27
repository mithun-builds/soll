#!/usr/bin/env bash
#
# Clean slate. Removes every trace of Soll from this machine: the brew cask,
# the brew tap, the .app, app data, and TCC permissions. Use this before
# `brew install --cask soll` to verify the install works as a brand-new user
# with nothing cached or pre-granted.

set -euo pipefail

step() { printf "\n\033[1;33m▸ %s\033[0m\n" "$1"; }
note() { printf "  %s\n" "$1"; }

# 1. Stop any running Soll
step "Stopping any running Soll"
pkill -x Soll 2>/dev/null && note "killed Soll" || note "(no Soll process)"
pkill -x soll 2>/dev/null || true
sleep 1

# 2. Brew cask
step "Removing brew cask"
if brew list --cask soll >/dev/null 2>&1; then
  brew uninstall --cask soll
else
  note "(not installed via brew)"
fi

# 3. Brew tap
step "Removing brew tap mithun-builds/soll"
if brew tap | grep -qi '^mithun-builds/soll$'; then
  brew untap mithun-builds/soll
else
  note "(tap not added)"
fi

# 4. Manual /Applications copy
step "Removing /Applications/Soll.app"
if [[ -d /Applications/Soll.app ]]; then
  rm -rf /Applications/Soll.app
  note "removed"
else
  note "(not present)"
fi

# 5. App data
step "Wiping ~/Library/Application Support/com.soll.app"
APP_DATA="$HOME/Library/Application Support/com.soll.app"
if [[ -d "$APP_DATA" ]]; then
  rm -rf "$APP_DATA"
  note "removed: models, settings, dictionary, skills"
else
  note "(no app data)"
fi

# 6. Logs
step "Wiping ~/Library/Logs/com.soll.app"
LOG_DIR="$HOME/Library/Logs/com.soll.app"
if [[ -d "$LOG_DIR" ]]; then
  rm -rf "$LOG_DIR"
fi

# 7. TCC permissions
step "Resetting TCC permissions for com.soll.app"
for service in Microphone Accessibility AppleEvents; do
  tccutil reset "$service" com.soll.app >/dev/null 2>&1 || true
done
note "done"

cat <<EOF

✓ Soll completely removed.

Test the user install path:

  brew tap mithun-builds/soll
  brew install --cask soll
  open /Applications/Soll.app

EOF
