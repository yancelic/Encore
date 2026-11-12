#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
if [[ ! -f target/release/encore-ui ]]; then
  echo 'Build the Linux preview first: npm --prefix apps/ui run build:preview' >&2
  exit 1
fi
mkdir -p packaging/flatpak/input
install -m755 target/release/encore-ui packaging/flatpak/input/encore-ui
flatpak-builder --user --install-deps-from=flathub --repo=target/flatpak-repo \
  target/flatpak-build packaging/flatpak/com.encore.clipper.json
flatpak build-bundle target/flatpak-repo target/Encore-linux-x86_64-dev.flatpak com.encore.clipper dev
