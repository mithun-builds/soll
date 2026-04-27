#!/usr/bin/env python3
"""
Render the exact Soll app logo SVG (from SettingsApp.tsx) into a 1024×1024 icon.
Background: dark rounded square matching macOS icon conventions.
Logo geometry is taken verbatim from src/settings/SettingsApp.tsx viewBox="0 0 28 22".
"""

import cairosvg
import subprocess
import os
import shutil

# --- Scale the 28×22 viewBox logo to fill ~58% of 1024×1024 icon ---
ICON = 1024
SCALE = 20.5          # 28 × 20.5 = 574px wide, 22 × 20.5 = 451px tall
LOGO_W = 28 * SCALE   # 574
LOGO_H = 22 * SCALE   # 451
X0 = (ICON - LOGO_W) / 2   # 225
Y0 = (ICON - LOGO_H) / 2   # 286.5

def s(v):  return str(v * SCALE)
def sx(v): return str(X0 + v * SCALE)
def sy(v): return str(Y0 + v * SCALE)

# Background rounded-rect radius: macOS standard is ~22.37% of icon size
BG_R = round(ICON * 0.2237)   # 229

svg = f"""<?xml version="1.0" encoding="UTF-8"?>
<svg width="{ICON}" height="{ICON}" viewBox="0 0 {ICON} {ICON}"
     xmlns="http://www.w3.org/2000/svg">

  <!-- Dark background — matches macOS icon rounded-square style -->
  <rect width="{ICON}" height="{ICON}" rx="{BG_R}" ry="{BG_R}" fill="#18181b"/>

  <!-- ── Wave bars (exact from SettingsApp.tsx) ── -->
  <!-- outer left  -->
  <rect x="{sx(0.5)}"  y="{sy(9)}"   width="{s(2.5)}" height="{s(4)}"  rx="{s(1.25)}" fill="rgba(255,255,255,0.9)"/>
  <!-- left mid -->
  <rect x="{sx(4)}"    y="{sy(7)}"   width="{s(2.5)}" height="{s(8)}"  rx="{s(1.25)}" fill="rgba(255,255,255,0.9)"/>
  <!-- inner left (tallest) -->
  <rect x="{sx(7.5)}"  y="{sy(3.5)}" width="{s(3)}"   height="{s(15)}" rx="{s(1.5)}"  fill="rgba(255,255,255,0.9)"/>
  <!-- inner right -->
  <rect x="{sx(17.5)}" y="{sy(4.5)}" width="{s(3)}"   height="{s(13)}" rx="{s(1.5)}"  fill="rgba(255,255,255,0.9)"/>
  <!-- right mid -->
  <rect x="{sx(21.5)}" y="{sy(7)}"   width="{s(2.5)}" height="{s(8)}"  rx="{s(1.25)}" fill="rgba(255,255,255,0.9)"/>
  <!-- outer right -->
  <rect x="{sx(25)}"   y="{sy(9)}"   width="{s(2.5)}" height="{s(4)}"  rx="{s(1.25)}" fill="rgba(255,255,255,0.9)"/>

  <!-- ── I-beam cursor (exact from SettingsApp.tsx) ── -->
  <!-- top serif -->
  <rect x="{sx(11.5)}"  y="{sy(2.5)}" width="{s(5)}"   height="{s(1.5)}" fill="#fde047"/>
  <!-- stem -->
  <rect x="{sx(13.25)}" y="{sy(2.5)}" width="{s(1.5)}" height="{s(17)}"  fill="#fde047"/>
  <!-- bottom serif -->
  <rect x="{sx(11.5)}"  y="{sy(18)}"  width="{s(5)}"   height="{s(1.5)}" fill="#fde047"/>

</svg>
"""

# Write SVG for inspection
svg_path = "/tmp/soll_icon.svg"
with open(svg_path, "w") as f:
    f.write(svg)
print("SVG written to", svg_path)

# Render to 1024×1024 PNG
png_path = "/tmp/soll_icon_1024.png"
cairosvg.svg2png(url=svg_path, write_to=png_path, output_width=ICON, output_height=ICON)
print("PNG rendered to", png_path)

# Copy to icon location
dest = "/Users/mithun/Documents/mithun-builds/soll/src-tauri/icons/icon.png"
shutil.copy(png_path, dest)
print("Copied to", dest)

# Build .icns from the PNG
icns_dir = "/tmp/soll.iconset"
os.makedirs(icns_dir, exist_ok=True)

from PIL import Image
img = Image.open(png_path)

sizes = [16, 32, 64, 128, 256, 512, 1024]
for sz in sizes:
    resized = img.resize((sz, sz), Image.LANCZOS)
    resized.save(f"{icns_dir}/icon_{sz}x{sz}.png")
    if sz <= 512:
        resized2 = img.resize((sz * 2, sz * 2), Image.LANCZOS)
        resized2.save(f"{icns_dir}/icon_{sz}x{sz}@2x.png")

subprocess.run(["iconutil", "-c", "icns", icns_dir,
                "-o", "/Users/mithun/Documents/mithun-builds/soll/src-tauri/icons/icon.icns"],
               check=True)
print("icon.icns generated")
print("Done ✓")
