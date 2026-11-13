#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

cargo +1.95.0 build --locked -p chigiri_books --bin chigiri_books
target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
if [[ "$target_dir" != /* ]]; then
  target_dir="$repo_root/$target_dir"
fi
python3 scripts/books-quick-start-smoke.py \
  --binary "$target_dir/debug/chigiri_books" \
  --output target/production-evidence/books-quick-start.json
echo "Books quick start and active-request drain checks passed."
