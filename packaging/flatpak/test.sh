#!/usr/bin/env bash
# Validate the installed runtime, not libraries inherited from the build SDK.
set -euo pipefail
cd "$(dirname "$0")/../.."
timeout 90s dbus-run-session -- xvfb-run -a flatpak run \
  --env=LIBGL_ALWAYS_SOFTWARE=1 --env=GDK_BACKEND=x11 \
  --command=sh com.encore.clipper -c '
    set -eu
    for binary in /app/bin/encore-ui /app/bin/encore-recorder /app/lib/libencore_capture.so; do
      echo "Checking runtime dependencies: $binary"
      dependencies=$(ldd "$binary")
      printf "%s\n" "$dependencies"
      if printf "%s\n" "$dependencies" | grep -F "not found"; then
        echo "Missing Flatpak runtime dependency: $binary" >&2
        exit 1
      fi
    done
    exec encore-recorder --self-test "$XDG_CACHE_HOME/self-test"
  '
python3 native/obs/verify-self-test.py "$HOME/.var/app/com.encore.clipper/cache/self-test/encore-self-test.mp4"
