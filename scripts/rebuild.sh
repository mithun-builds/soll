#!/usr/bin/env bash
#
# Quick rebuild + reinstall, preserving app state (granted permissions,
# downloaded models, settings). Use this when iterating on code that you
# want to test against your already-onboarded local install.
#
# For a clean first-time-user test instead, use ./scripts/test-fresh.sh
# which also wipes TCC entries and ~/Library/Application Support/com.soll.app.

set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf "\n\033[1;33m▸ %s\033[0m\n" "$1"; }
note() { printf "  %s\n" "$1"; }

# ── 1. stop any running Soll ──────────────────────────────────────────────────
step "1/5  Stopping any running Soll instance"
if pgrep -x Soll >/dev/null 2>&1; then
  pkill -x Soll 2>/dev/null || true
  note "killed Soll"
elif pgrep -x soll >/dev/null 2>&1; then
  pkill -x soll 2>/dev/null || true
  note "killed soll dev binary"
else
  note "(no Soll process running)"
fi
# Give launchd / kernel a beat to release the bundle path before we overwrite it.
sleep 1

# ── 2. build the .app ────────────────────────────────────────────────────────
# `--bundles app` skips DMG packaging, which uses AppleScript-via-osascript
# under the hood and trips Automation permission on macOS 16 beta. Release
# DMGs are built by the GitHub Actions workflow on a clean macOS runner; we
# don't need them for local iteration.
step "2/5  Building Soll.app (no DMG — local build)"
pnpm tauri build --bundles app

APP_SRC="src-tauri/target/release/bundle/macos/Soll.app"
if [[ ! -d "$APP_SRC" ]]; then
  echo "✗ build did not produce $APP_SRC" >&2
  exit 1
fi

# ── 3. install ───────────────────────────────────────────────────────────────
step "3/5  Replacing /Applications/Soll.app"
rm -rf /Applications/Soll.app
cp -R "$APP_SRC" /Applications/

# ── 4. strip quarantine + re-sign ad-hoc + reset Accessibility ───────────────
# Re-signing after copy: `cp -R` can lose the build-time ad-hoc signature,
# which makes the kernel reject Accessibility (AXIsProcessTrusted returns
# false even when the System Settings toggle is on). Re-binding the bundle
# to a fresh ad-hoc signature here keeps that path working.
#
# But there's a second pitfall on rebuilds: each ad-hoc signature is unique,
# so the previously-granted Accessibility TCC entry is bound to the OLD
# binary's signature. System Settings keeps showing Soll toggled on but
# AXIsProcessTrusted still returns false, because the kernel sees a
# signature mismatch. Resetting the entry forces a fresh grant against the
# new signature — the user toggles on once in Settings, restarts Soll
# (via the onboarding's "Restart Soll to apply" button), and it sticks.
#
# Mic and AppleEvents don't have this signature-binding fragility, so we
# leave those alone — the user keeps their granted mic across rebuilds.
step "4/5  Stripping quarantine, re-signing, resetting Accessibility TCC"
xattr -dr com.apple.quarantine /Applications/Soll.app
codesign --force --deep --sign - /Applications/Soll.app 2>/dev/null || true
tccutil reset Accessibility com.soll.app >/dev/null 2>&1 || true

# ── 5. launch ────────────────────────────────────────────────────────────────
step "5/5  Launching"
open /Applications/Soll.app

echo
echo "✓ Done. Soll is running with your previous state intact."
