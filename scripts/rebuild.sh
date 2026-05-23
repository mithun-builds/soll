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

# ── 4. patch Info.plist + strip+re-sign + reset Accessibility ────────────────
# Three things happen here, in this exact order, to mirror the CI workflow
# in `.github/workflows/release.yml`:
#
# (a) Inject NSMicrophoneUsageDescription + NSAppleEventsUsageDescription
#     into Info.plist. Tauri's config has no slot for these; without them
#     macOS silently denies the mic prompt the very first time.
#
# (b) Strip the existing signature, then re-sign ad-hoc. Two reasons:
#       1. `cp -R` can drop the build-time signature, leaving Accessibility
#          broken (AXIsProcessTrusted returns false even when System
#          Settings shows ON).
#       2. `pnpm tauri build` ships a *linker-signed* binary whose
#          codesign identifier is a hash like `soll-dc34b220ba651dd5`
#          instead of `com.soll.app`. TCC keys grants by that identifier,
#          so the user's existing Accessibility grant doesn't apply to a
#          linker-signed local build. Stripping + re-signing forces
#          codesign to read Info.plist and use `com.soll.app`, matching
#          the brew install and any prior TCC entry.
#
# (c) Reset the Accessibility TCC entry. Each ad-hoc signature is unique
#     (different binary hash even with the same identifier), so the
#     previously-granted entry can still be stale. Resetting forces a
#     fresh grant against the new signature — toggle on once in Settings,
#     restart Soll, sticks.
#
# Mic and AppleEvents don't have the signature-binding fragility, so we
# leave those TCC entries alone — keeps the user's granted mic intact
# across rebuilds.
step "4/5  Patching Info.plist, re-signing, resetting Accessibility TCC"

APP=/Applications/Soll.app
PLIST="$APP/Contents/Info.plist"

MIC_DESC='Soll needs microphone access to transcribe your speech locally.'
AE_DESC='Soll uses AppleScript to paste transcribed text into the focused app.'

/usr/libexec/PlistBuddy -c "Add :NSMicrophoneUsageDescription string '$MIC_DESC'" "$PLIST" 2>/dev/null || \
/usr/libexec/PlistBuddy -c "Set :NSMicrophoneUsageDescription '$MIC_DESC'" "$PLIST"

/usr/libexec/PlistBuddy -c "Add :NSAppleEventsUsageDescription string '$AE_DESC'" "$PLIST" 2>/dev/null || \
/usr/libexec/PlistBuddy -c "Set :NSAppleEventsUsageDescription '$AE_DESC'" "$PLIST"

xattr -dr com.apple.quarantine "$APP"
codesign --remove-signature "$APP" 2>/dev/null || true
codesign --force --deep --sign - "$APP"

# Verify the identifier — should be com.soll.app, not soll-<hash>.
# `--identifier` is a *signing* flag; passing it to `-d` prints the usage
# banner and exits non-zero. Use `-dv` (verbose display) and pull the
# `Identifier=…` line out of stderr.
IDENT=$(codesign -dv "$APP/Contents/MacOS/soll" 2>&1 | awk -F= '/^Identifier=/{print $2; exit}')
note "signing identifier: $IDENT"
if [[ "$IDENT" != "com.soll.app" ]]; then
  echo "✗ unexpected signing identifier (got '$IDENT', expected com.soll.app)" >&2
  echo "  Accessibility grants won't apply. Check codesign + Info.plist." >&2
  exit 1
fi

tccutil reset Accessibility com.soll.app >/dev/null 2>&1 || true

# ── 5. launch ────────────────────────────────────────────────────────────────
step "5/5  Launching"
open /Applications/Soll.app

echo
echo "✓ Done. Soll is running with your previous state intact."
