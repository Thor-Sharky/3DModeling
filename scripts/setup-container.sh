#!/usr/bin/env bash
# One-time setup for a fresh Linux dev container (Ubuntu 24.04):
# winit needs libxkbcommon-x11 to open windows under Xvfb.
set -euo pipefail
if ldconfig -p | grep -q libxkbcommon-x11.so.0; then
  echo "libxkbcommon-x11 already installed"
  exit 0
fi
if command -v apt-get >/dev/null && [ "$(id -u)" = 0 ]; then
  apt-get update -qq && apt-get install -y -qq libxkbcommon-x11-0 xvfb xauth
else
  # Fallback: unpack the .debs locally and point LD_LIBRARY_PATH at them.
  mkdir -p vendor-libs && cd vendor-libs
  apt-get download libxkbcommon-x11-0 libxcb-xkb1
  for d in *.deb; do dpkg-deb -x "$d" .; done
  echo "export LD_LIBRARY_PATH=$PWD/usr/lib/x86_64-linux-gnu:\${LD_LIBRARY_PATH:-}"
fi
