#!/bin/bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"

"$root/scripts/quit.sh"
rm -rf "$HOME/Applications/ShareK.app"
