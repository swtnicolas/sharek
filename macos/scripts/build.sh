#!/bin/bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
bundle="$root/build/ShareK.app"

swift build --package-path "$root" --configuration release

rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "$root/.build/release/ShareK" "$bundle/Contents/MacOS/ShareK"
cp "$root/Resources/Info.plist" "$bundle/Contents/Info.plist"
cp "$root/assets/icon/AppIcon.icns" "$bundle/Contents/Resources/AppIcon.icns"

identity="-"
if security find-identity -p codesigning | grep -q '"Nico Local"'; then
    identity="Nico Local"
fi
codesign --force --sign "$identity" "$bundle"

echo "$bundle"
