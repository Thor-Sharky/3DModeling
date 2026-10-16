#!/usr/bin/env bash
# Regenerate Cargo.lock using only crates published at least 14 days before the cutoff.
# Usage: scripts/lock.sh [YYYY-MM-DDT00:00:00Z]
set -euo pipefail
cd "$(dirname "$0")/.."
CUTOFF="${1:-2026-10-02T00:00:00Z}"
RUSTC_BOOTSTRAP=1 cargo generate-lockfile -Zunstable-options --publish-time "$CUTOFF"
