#!/bin/bash
# Builds Promptagotchi.app (Rust core via Tauri) and a drag-to-Applications disk image:
#   dist/Promptagotchi-<version>-macOS-<arch>.dmg
#
#   npm run dist:mac                 # for this Mac (arm64 or x64)
#   ARCH=x64 npm run dist:mac        # for Intel Macs
#
# Needs Rust, Node (for the Tauri CLI) and Xcode's command line tools (swiftc, iconutil, hdiutil).
# The app is ad-hoc signed, not
# notarized, so the disk image explains how to open it the first time.
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
cd "$here"
version=$(node -p "require('./package.json').version")
arch="${ARCH:-$(uname -m | sed 's/x86_64/x64/')}"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

echo "› Icon"
xcrun swiftc -O -o "$work/MakeIcon" build/mac/MakeIcon.swift 2>/dev/null
"$work/MakeIcon" pets/pip/pet.json "$work/AppIcon.iconset"
iconutil -c icns "$work/AppIcon.iconset" -o build/mac/AppIcon.icns

echo "› App ($arch, Rust + Tauri)"
cp build/mac/AppIcon.icns src-tauri/icons/icon.icns
case "$arch" in arm64) target=aarch64-apple-darwin ;; x64) target=x86_64-apple-darwin ;; *) target=$arch ;; esac
if [ "$target" = "$(rustc -vV | sed -n 's/^host: //p')" ]; then
  npx tauri build --bundles app 2>&1 | grep -E "Finished|Bundling|error" || true
  app="$here/src-tauri/target/release/bundle/macos/Promptagotchi.app"
else
  npx tauri build --bundles app --target "$target" 2>&1 | grep -E "Finished|Bundling|error" || true
  app="$here/src-tauri/target/$target/release/bundle/macos/Promptagotchi.app"
fi
[ -d "$app" ] || { echo "build failed: no $app"; exit 1; }
codesign --force --deep -s - "$app" 2>/dev/null

echo "› Disk image"
mkdir -p dist
dmg="$here/dist/Promptagotchi-$version-macOS-$arch.dmg"
stage="$work/stage"
vol="Promptagotchi"
mkdir -p "$stage"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
xcrun swiftc -O -o "$work/MakeDMGArt" build/mac/MakeDMGArt.swift 2>/dev/null
"$work/MakeDMGArt" "$stage"
tiffutil -cathidpicheck "$stage/.background/background.png" "$stage/.background/background@2x.png" -out "$stage/.background/background.tiff" 2>/dev/null
rm "$stage/.background/"*.png
rw="$work/rw.dmg"
# Room to spare: the Finder needs some to save the window's layout (it's compressed away after).
size=$(( $(du -sm "$stage" | cut -f1) + 40 ))
hdiutil create -quiet -volname "$vol" -srcfolder "$stage" -fs HFS+ -format UDRW -size ${size}m -ov "$rw"
[[ -d "/Volumes/$vol" ]] && hdiutil detach -quiet "/Volumes/$vol" -force
hdiutil attach -quiet -readwrite -noverify -noautoopen "$rw"
# The Finder lays the window out, so a Finder window flashes open for a moment while it does.
osascript <<APPLESCRIPT || echo "  (couldn't arrange the window: Finder automation wasn't allowed. The disk image still works.)"
tell application "Finder"
    tell disk "$vol"
        open
        set current view of container window to icon view
        set toolbar visible of container window to false
        set statusbar visible of container window to false
        set bounds of container window to {200, 120, 860, 648}
        set opts to icon view options of container window
        set arrangement of opts to not arranged
        set icon size of opts to 112
        set text size of opts to 13
        set background picture of opts to file ".background:background.tiff"
        set position of item "Promptagotchi.app" of container window to {170, 150}
        set position of item "Applications" of container window to {490, 150}
        set position of item "Open Security Settings" of container window to {545, 390}
        close
        open
        update without registering applications
        delay 1
        close
    end tell
end tell
APPLESCRIPT
# Pip as the disk's own icon (after the Finder is done: it clears it otherwise).
cp build/mac/AppIcon.icns "/Volumes/$vol/.VolumeIcon.icns"
SetFile -a C "/Volumes/$vol"
sync
hdiutil detach -quiet "/Volumes/$vol"
rm -f "$dmg"
hdiutil convert -quiet "$rw" -format UDZO -imagekey zlib-level=9 -o "$dmg"
echo "✓ Built $dmg ($(du -h "$dmg" | cut -f1))"
