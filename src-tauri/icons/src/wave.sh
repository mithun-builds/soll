#!/usr/bin/env bash
# Generates one 22×22 tray icon SVG.
# Usage: wave.sh <bars_color> <cursor_color> <opacity> <out_file>
BARS=$1
CURSOR=$2
OPACITY=$3
OUT=$4

cat > "$OUT" << EOF
<svg width="22" height="22" viewBox="0 0 22 22" xmlns="http://www.w3.org/2000/svg" opacity="${OPACITY}">
  <!-- Wave bars -->
  <rect x="0.5"  y="9"   width="2"   height="4"  rx="1"    fill="${BARS}"/>
  <rect x="3.5"  y="7"   width="2"   height="8"  rx="1"    fill="${BARS}"/>
  <rect x="6.5"  y="3.5" width="2.5" height="15" rx="1.25" fill="${BARS}"/>
  <rect x="14"   y="4.5" width="2.5" height="13" rx="1.25" fill="${BARS}"/>
  <rect x="17.5" y="7"   width="2"   height="8"  rx="1"    fill="${BARS}"/>
  <rect x="20.5" y="9"   width="1.5" height="4"  rx="0.75" fill="${BARS}"/>
  <!-- I-beam cursor -->
  <rect x="9.5"  y="2.5" width="4"   height="1.5"          fill="${CURSOR}"/>
  <rect x="10.75" y="2.5" width="1.5" height="17"           fill="${CURSOR}"/>
  <rect x="9.5"  y="18"  width="4"   height="1.5"          fill="${CURSOR}"/>
</svg>
EOF
