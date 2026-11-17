#!/usr/bin/env bash
# XComposite requires an EWMH window manager, including inside Xvfb.
set -euo pipefail
openbox >"${TMPDIR:-/tmp}/encore-test-window-manager.log" 2>&1 &
manager=$!
cleanup() { kill "$manager" 2>/dev/null || true; wait "$manager" 2>/dev/null || true; }
trap cleanup EXIT
for attempt in {1..50}; do
  if xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -q 'window id'; then break; fi
  sleep 0.1
done
"$@"
