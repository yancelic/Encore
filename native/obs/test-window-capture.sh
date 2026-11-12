#!/usr/bin/env bash
set -euo pipefail
target/native-obs-linux/bridge-build/encore-capture-fixture &
fixture=$!
cleanup() { kill "$fixture" 2>/dev/null || true; wait "$fixture" 2>/dev/null || true; }
trap cleanup EXIT
sleep 1
cargo run --locked -p encore-recorder -- --capture-self-test target/native-window-self-test
python3 native/obs/verify-self-test.py target/native-window-self-test/encore-self-test.mp4 --silent-audio
