#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if (( $# > 1 )); then
  echo "usage: check-production-performance-harness.sh [IMMUTABLE_TOOLCHAIN]" >&2
  exit 2
fi
toolchain="${1:-1.95.0}"
if [[ ! "$toolchain" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "The short workload requires an immutable x.y.z toolchain." >&2
  exit 2
fi
# Test the current candidate in the ordinary Cargo environment; this is not
# evidence for a formal performance comparison.
output="target/production-evidence/harness-short-soak.json"
cargo "+$toolchain" build --release --locked -p chigiri_books --bin chigiri_books
target_dir="$(cargo "+$toolchain" metadata --locked --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
binary="$target_dir/release/chigiri_books"
python3 scripts/production-http-workload.py short-soak \
  --binary "$binary" \
  --toolchain "$toolchain" \
  --requests 10000 \
  --concurrency 2 \
  --duration 2 \
  --rss-baseline-seconds 0.5 \
  --output "$output"
python3 scripts/verify-performance-evidence.py short-soak "$output" \
  --binary "$binary" --toolchain "$toolchain"

python3 - "$output" <<'PY'
import json
import sys

evidence = json.load(open(sys.argv[1], encoding="utf-8"))
if evidence["querySuccesses"] <= 0 or evidence["mutationSuccesses"] <= 0:
    raise SystemExit("The short soak requires Query/Mutation to reach both resolvers.")
PY
