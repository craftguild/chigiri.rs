#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

for target in parser sdl_composition http_decode json_conversion persisted_id config_parser; do
  corpus="fuzz/corpus/$target"
  if [[ ! -d "$corpus" ]] || [[ -z "$(find "$corpus" -type f -maxdepth 1 -print -quit)" ]]; then
    echo "Required fixed fuzz corpus is missing: $corpus" >&2
    exit 1
  fi
done

start=$SECONDS
cargo +1.95.0 test --manifest-path fuzz/Cargo.toml --lib --locked &
test_pid=$!
while kill -0 "$test_pid" 2>/dev/null; do
  if (( SECONDS - start > 120 )); then
    kill "$test_pid" 2>/dev/null || true
    wait "$test_pid" || true
    echo "Fuzz regression is over 120 seconds." >&2
    exit 1
  fi
  sleep 1
done
wait "$test_pid"
echo "The secondary fuzz-regression phase completed successfully."
