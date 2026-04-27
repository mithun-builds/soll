#!/usr/bin/env bash
#
# End-to-end "fresh first-time install" test runner.
#
# Stops anything running, removes every prior installation (brew + manual),
# wipes app data + TCC entries, rebuilds the .app from source, installs to
# /Applications, strips quarantine, and launches. After it finishes you are
# the same as a brand-new user installing Soll for the first time.
#
#   ./scripts/test-fresh.sh             # keeps any pulled Ollama models
#   ./scripts/test-fresh.sh --ollama    # also removes the pulled Ollama model

set -euo pipefail
cd "$(dirname "$0")/.."

OLLAMA_FLAG="${1:-}"

step() { printf "\n\033[1;33m▸ %s\033[0m\n" "$1"; }
note() { printf "  %s\n" "$1"; }

# ── 1. stop any running Soll ──────────────────────────────────────────────────
step "1/7  Stopping any running Soll instance"
pkill -x Soll 2>/dev/null || note "(no Soll process)"
pkill -x soll 2>/dev/null || true
sleep 1

# ── 2. uninstall every prior install ──────────────────────────────────────────
step "2/7  Removing prior installations"
if brew list --cask soll >/dev/null 2>&1; then
  brew uninstall --cask soll
else
  note "(no brew cask installed)"
fi
if [[ -d /Applications/Soll.app ]]; then
  rm -rf /Applications/Soll.app
  note "removed /Applications/Soll.app"
else
  note "(no /Applications/Soll.app)"
fi

# ── 3. wipe app data + TCC ────────────────────────────────────────────────────
step "3/7  Wiping app data, TCC permissions, dictionary, settings"
./scripts/reset-onboarding.sh "$OLLAMA_FLAG" >/dev/null 2>&1 || true
note "done"

# ── 4. clean build artifacts ──────────────────────────────────────────────────
step "4/7  Clearing build artifacts"
rm -rf src-tauri/target/release/bundle
rm -rf dist
note "done"

# ── 5. build the .app bundle ──────────────────────────────────────────────────
# `--bundles app` skips DMG packaging — its bundle_dmg.sh uses AppleScript
# under the hood and trips Automation permission on macOS 16 beta. The
# release DMG is built by GitHub Actions on a clean runner, not here.
step "5/7  Building Soll.app (this takes a few minutes; no DMG)"
pnpm tauri build --bundles app

APP_SRC="src-tauri/target/release/bundle/macos/Soll.app"
if [[ ! -d "$APP_SRC" ]]; then
  echo "✗ build did not produce $APP_SRC" >&2
  exit 1
fi

# ── 6. install + de-quarantine + re-sign ──────────────────────────────────────
# Re-signing after the copy is critical: `cp -R` can lose the build-time
# ad-hoc signature on some macOS versions, which then makes the kernel-level
# AXIsProcessTrusted() check return false even when the toggle in System
# Settings → Privacy & Security → Accessibility is on. Force-re-signing
# ad-hoc rebinds the bundle to a coherent signature.
step "6/7  Installing into /Applications, stripping quarantine, re-signing"
cp -R "$APP_SRC" /Applications/
xattr -dr com.apple.quarantine /Applications/Soll.app
codesign --force --deep --sign - /Applications/Soll.app 2>/dev/null || true
note "installed at /Applications/Soll.app"

# ── 7. launch ─────────────────────────────────────────────────────────────────
step "7/7  Launching"
open /Applications/Soll.app

cat <<EOF

✓ Done. You are now a fresh first-time user.

What to verify:
  · Setup Guide opens automatically (0/5 steps)
  · Step 1: 4 model cards, "Small ★ Recommended" highlighted; toggle one to download
  · Step 2: Mic toggle → macOS dialog with Soll's usage description; allow it
  · Step 3: Accessibility toggle → Settings opens; tick Soll, then click "Restart Soll to apply"
  · Step 4 (Ollama, optional): if running with no model pulled, toggle on → "Pulling…"
  · Step 5: Hold ⌃⇧Space, dictate; text gets pasted

To redo this test from scratch later: rerun ./scripts/test-fresh.sh
To just reset state without rebuilding:  ./scripts/reset-onboarding.sh

EOF
