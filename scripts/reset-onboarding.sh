#!/usr/bin/env bash
#
# Reset Soll to a clean "first-time-user" state, so the onboarding flow can
# be tested end-to-end. Works for both `pnpm tauri dev` and the installed
# /Applications/Soll.app — they share the bundle id com.soll.app.
#
#   ./scripts/reset-onboarding.sh           # soft reset (keeps Ollama models)
#   ./scripts/reset-onboarding.sh --ollama  # also remove pulled Ollama models

set -euo pipefail

BUNDLE_ID="com.soll.app"
APP_DATA="$HOME/Library/Application Support/$BUNDLE_ID"

# ── kill any running Soll instance ───────────────────────────────────────────
if pgrep -x Soll >/dev/null 2>&1; then
  echo "→ Killing running Soll process"
  pkill -x Soll 2>/dev/null || true
  sleep 0.5
fi
if pgrep -x soll >/dev/null 2>&1; then
  echo "→ Killing running soll dev binary"
  pkill -x soll 2>/dev/null || true
  sleep 0.5
fi

# ── revoke macOS TCC permissions ─────────────────────────────────────────────
# tccutil errors when there is no entry — that just means it was never granted,
# which is fine. Suppress the noise.
echo "→ Revoking macOS privacy permissions"
for service in Microphone Accessibility AppleEvents; do
  tccutil reset "$service" "$BUNDLE_ID" >/dev/null 2>&1 || true
done

# ── nuke app data: models, settings db, dictionary, skills ───────────────────
if [[ -d "$APP_DATA" ]]; then
  echo "→ Deleting $APP_DATA"
  rm -rf "$APP_DATA"
else
  echo "→ No app data to delete (already clean)"
fi

# ── optionally remove pulled Ollama models ───────────────────────────────────
if [[ "${1:-}" == "--ollama" ]]; then
  if command -v ollama >/dev/null 2>&1; then
    echo "→ Removing Soll's default Ollama model"
    ollama rm llama3.2:3b >/dev/null 2>&1 || echo "  (not pulled)"
  else
    echo "→ Skipping Ollama cleanup (ollama CLI not installed)"
  fi
fi

echo
echo "✓ Reset complete. Bundle $BUNDLE_ID has no permissions, no data."
echo
echo "Now test as a fresh user:"
echo "  • Dev mode:        cd $(dirname "$0")/.. && pnpm tauri dev"
echo "  • Installed app:   open /Applications/Soll.app"
