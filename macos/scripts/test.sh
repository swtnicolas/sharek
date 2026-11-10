#!/bin/bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
plugins="$(dirname "$(xcrun --find swift)")/../lib/swift/host/plugins/testing"

swift test --package-path "$root" -Xswiftc -plugin-path -Xswiftc "$plugins" "$@"
