#!/usr/bin/env bash
# Converts SVG sources → PNG tray icons.
# Requires: npx svgexport  (installed automatically via npx)
#
# Run once after editing any SVG in src/:
#   cd src-tauri/icons && bash build_icons.sh

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SRC="$SCRIPT_DIR/src"
OUT="$SCRIPT_DIR"

conv() {
  local svg="$1" png="$2" size="$3"
  npx --yes svgexport "$svg" "$png" "${size}:${size}" 2>/dev/null
  echo "  $png"
}

echo "Building tray icons…"

conv "$SRC/wave_default.svg"     "$OUT/tray.png"               22
conv "$SRC/wave_default.svg"     "$OUT/tray@2x.png"            44

conv "$SRC/wave_blue.svg"        "$OUT/tray_blue.png"          22
conv "$SRC/wave_blue.svg"        "$OUT/tray_blue@2x.png"       44
conv "$SRC/wave_blue_dim.svg"    "$OUT/tray_blue_dim.png"      22
conv "$SRC/wave_blue_dim.svg"    "$OUT/tray_blue_dim@2x.png"   44

conv "$SRC/wave_yellow.svg"      "$OUT/tray_yellow.png"        22
conv "$SRC/wave_yellow.svg"      "$OUT/tray_yellow@2x.png"     44

conv "$SRC/wave_green.svg"       "$OUT/tray_green.png"         22
conv "$SRC/wave_green.svg"       "$OUT/tray_green@2x.png"      44

conv "$SRC/wave_red.svg"         "$OUT/tray_red.png"           22
conv "$SRC/wave_red.svg"         "$OUT/tray_red@2x.png"        44
conv "$SRC/wave_red_dim.svg"     "$OUT/tray_red_dim.png"       22
conv "$SRC/wave_red_dim.svg"     "$OUT/tray_red_dim@2x.png"    44

conv "$SRC/wave_orange.svg"      "$OUT/tray_orange.png"        22
conv "$SRC/wave_orange.svg"      "$OUT/tray_orange@2x.png"     44
conv "$SRC/wave_orange_dim.svg"  "$OUT/tray_orange_dim.png"    22
conv "$SRC/wave_orange_dim.svg"  "$OUT/tray_orange_dim@2x.png" 44

conv "$SRC/wave_loading.svg"     "$OUT/tray_loading.png"       22
conv "$SRC/wave_loading.svg"     "$OUT/tray_loading@2x.png"    44
conv "$SRC/wave_loading_dim.svg" "$OUT/tray_loading_dim.png"   22
conv "$SRC/wave_loading_dim.svg" "$OUT/tray_loading_dim@2x.png" 44

echo "Done."
