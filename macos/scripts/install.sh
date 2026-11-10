#!/bin/bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
target="$HOME/Applications/ShareK.app"

"$root/scripts/build.sh"
"$root/scripts/quit.sh"
mkdir -p "$HOME/Applications"
rm -rf "$target"
cp -R "$root/build/ShareK.app" "$target"
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$target"
open "$target"
