#!/usr/bin/env bash
# Run every scenario script in tests/scenarios headlessly. Fails if any fails.
# Screenshots and report.json land in target/shots/<scenario>/.
set -uo pipefail
cd "$(dirname "$0")/.."
cargo build --locked -p shapeshop || exit 1
failed=()
for s in tests/scenarios/*.ron; do
  name=$(basename "$s" .ron)
  printf '%-28s' "$name"
  if timeout 600 scripts/headless.sh --msaa 0 --script "$s" >"target/shots-$name.log" 2>&1; then
    echo "passed"
  else
    echo "FAILED (see target/shots-$name.log and target/shots/$name/report.json)"
    failed+=("$name")
  fi
done
if ((${#failed[@]})); then
  echo "failed: ${failed[*]}"
  exit 1
fi
echo "all scenarios passed"
