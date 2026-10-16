#!/usr/bin/env bash
# Run Shapeshop under a virtual X display with Mesa's software OpenGL (llvmpipe).
# Usage: scripts/headless.sh [shapeshop args...]
# Env: SHAPESHOP_BIN (default target/debug/shapeshop), SHAPESHOP_SCREEN (default 1600x900x24)
set -euo pipefail
cd "$(dirname "$0")/.."
BIN="${SHAPESHOP_BIN:-target/debug/shapeshop}"
SCREEN="${SHAPESHOP_SCREEN:-1600x900x24}"
DATA_DIR="${SHAPESHOP_DATA_DIR:-$(mktemp -d)}"
has_data_dir=0
for a in "$@"; do [[ "$a" == "--data-dir" ]] && has_data_dir=1; done
extra=()
[[ $has_data_dir == 0 ]] && extra+=(--data-dir "$DATA_DIR")
exec xvfb-run -a -s "-screen 0 ${SCREEN} -nolisten tcp" \
  env -u WAYLAND_DISPLAY XMODIFIERS=@im=none LIBGL_ALWAYS_SOFTWARE=1 GALLIUM_DRIVER=llvmpipe __GLX_VENDOR_LIBRARY_NAME=mesa \
  "$BIN" "${extra[@]}" "$@"
