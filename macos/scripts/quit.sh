#!/bin/bash
set -euo pipefail

osascript -l JavaScript -e 'ObjC.import("Foundation"); $.NSDistributedNotificationCenter.defaultCenter.postNotificationNameObjectUserInfoDeliverImmediately($("local.sharek.quit"), $.nil, $.nil, true)' >/dev/null 2>&1 || true
for _ in $(seq 50); do
    pgrep -x ShareK >/dev/null || break
    sleep 0.1
done
pkill -x ShareK || true
for _ in $(seq 50); do
    pgrep -x ShareK >/dev/null || break
    sleep 0.1
done
