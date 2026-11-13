#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$repo_root/scripts/performance-toolchain.sh"
toolchain="${1:-}"
if (( $# != 1 )); then
  echo "usage: run-release-load-soak.sh IMMUTABLE_TOOLCHAIN" >&2
  exit 2
fi
require_performance_toolchain "$toolchain" "$repo_root"

if [[ "${CHIGIRI_PERFORMANCE_ISOLATED:-0}" != "1" ]]; then
  cd "$repo_root"
  if [[ -n "$(git status --porcelain)" ]]; then
    echo "load/soak is running from the clean worktree's isolation head." >&2
    exit 2
  fi
  temporary_root="$(mktemp -d)"
  isolated_source="$temporary_root/source"
  cleanup() {
    git worktree remove --force "$isolated_source" >/dev/null 2>&1 || true
    rmdir "$temporary_root" >/dev/null 2>&1 || true
  }
  trap cleanup EXIT
  git worktree add --quiet --detach "$isolated_source" HEAD
  CHIGIRI_PERFORMANCE_ISOLATED=1 \
    CHIGIRI_PERFORMANCE_OUTPUT="$repo_root/target/production-evidence" \
    "$isolated_source/scripts/run-release-load-soak.sh" "$toolchain"
  exit 0
fi

cd "$repo_root"
output_root="${CHIGIRI_PERFORMANCE_OUTPUT:-$repo_root/target/production-evidence}"
mkdir -p "$output_root"
performance_cargo build --release --locked -p chigiri_books --bin chigiri_books
mkdir -p "$output_root/workload-binary"
cp "$repo_root/target/release/chigiri_books" "$output_root/workload-binary/chigiri_books"
binary="$output_root/workload-binary/chigiri_books"

python3 scripts/production-http-workload.py load \
  --binary "$binary" \
  --toolchain "$toolchain" \
  --requests 10000 \
  --concurrency 64 \
  --duration 900 \
  --rss-baseline-seconds 1.0 \
  --output "$output_root/load.json"
python3 scripts/verify-performance-evidence.py load "$output_root/load.json" \
  --binary "$binary" --toolchain "$toolchain"

python3 scripts/production-http-workload.py soak \
  --binary "$binary" \
  --toolchain "$toolchain" \
  --requests 10000 \
  --concurrency 32 \
  --duration 900 \
  --rss-baseline-seconds 1.0 \
  --output "$output_root/soak.json"
python3 scripts/verify-performance-evidence.py soak "$output_root/soak.json" \
  --binary "$binary" --toolchain "$toolchain"

echo "Release load/soak thresholds and evidence attribution passed."
