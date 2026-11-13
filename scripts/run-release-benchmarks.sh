#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$repo_root/scripts/performance-toolchain.sh"
mode="${1:-}"
toolchain="${2:-}"
case "$mode" in
  record)
    if (( $# != 2 )); then
      echo "usage: run-release-benchmarks.sh record IMMUTABLE_TOOLCHAIN" >&2
      exit 2
    fi
    ;;
  verify-initial|compare-initial)
    if (( $# != 3 )); then
      echo "usage: run-release-benchmarks.sh $mode TOOLCHAIN ACCEPTED_BASELINE_JSON" >&2
      exit 2
    fi
    ;;
  verify-previous|compare-previous)
    if (( $# != 4 )); then
      echo "usage: run-release-benchmarks.sh $mode TOOLCHAIN ACCEPTED_JSON BASELINE_BINARY" >&2
      exit 2
    fi
    ;;
  *)
    echo "Mode must be one of record, verify-initial/compare-initial, or verify-previous/compare-previous." >&2
    exit 2
    ;;
esac
require_performance_toolchain "$toolchain" "$repo_root"

if [[ "${CHIGIRI_PERFORMANCE_ISOLATED:-0}" != "1" ]]; then
  cd "$repo_root"
  if [[ -n "$(git status --porcelain)" ]]; then
    echo "Run release benchmarks from the isolated clean-worktree revision." >&2
    exit 2
  fi
  baseline=""
  baseline_binary=""
  if [[ "$mode" != "record" ]]; then
    baseline="$(cd "$(dirname "$3")" && pwd)/$(basename "$3")"
  fi
  if [[ "$mode" == *-previous ]]; then
    baseline_binary="$(cd "$(dirname "$4")" && pwd)/$(basename "$4")"
  fi
  temporary_root="$(mktemp -d)"
  isolated_source="$temporary_root/source"
  cleanup() {
    git worktree remove --force "$isolated_source" >/dev/null 2>&1 || true
    rmdir "$temporary_root" >/dev/null 2>&1 || true
  }
  trap cleanup EXIT
  git worktree add --quiet --detach "$isolated_source" HEAD
  environment=(
    CHIGIRI_PERFORMANCE_ISOLATED=1
    CHIGIRI_PERFORMANCE_OUTPUT="$repo_root/target/production-evidence"
  )
  if [[ "$mode" == "record" ]]; then
    env "${environment[@]}" "$isolated_source/scripts/run-release-benchmarks.sh" \
      record "$toolchain"
  elif [[ "$mode" == *-previous ]]; then
    env "${environment[@]}" "$isolated_source/scripts/run-release-benchmarks.sh" \
      "$mode" "$toolchain" "$baseline" "$baseline_binary"
  else
    env "${environment[@]}" "$isolated_source/scripts/run-release-benchmarks.sh" \
      "$mode" "$toolchain" "$baseline"
  fi
  exit 0
fi

cd "$repo_root"
output_root="${CHIGIRI_PERFORMANCE_OUTPUT:-$repo_root/target/production-evidence}"
mkdir -p "$output_root"
if [[ "$mode" != "record" ]]; then
  python3 scripts/verify-performance-evidence.py baseline-toolchain "$3" \
    --toolchain "$toolchain"
fi

built_binary="$repo_root/target/release/production-benchmark"
baseline_binary="$output_root/benchmark-baseline-binary/production-benchmark"
candidate_binary="$output_root/benchmark-candidate-binary/production-benchmark"
if [[ "$mode" == "record" || "$mode" == *-previous ]]; then
  performance_cargo build --release --locked -p chigiri_books \
    --features production-validation --bin production-benchmark
fi
if [[ "$mode" == "record" ]]; then
  mkdir -p "$(dirname "$baseline_binary")"
  cp "$built_binary" "$baseline_binary"
  binary="$baseline_binary"
elif [[ "$mode" == *-initial ]]; then
  binary="$baseline_binary"
  if [[ ! -x "$binary" ]]; then
    echo "The initial comparison requires the binary saved with the baseline: $binary." >&2
    exit 2
  fi
else
  mkdir -p "$(dirname "$candidate_binary")"
  cp "$built_binary" "$candidate_binary"
  binary="$candidate_binary"
fi

if [[ "$mode" == "record" ]]; then
  "$binary" record \
    --toolchain "$toolchain" \
    --samples 31 \
    --iterations 100 \
    --output "$output_root/benchmark-baseline-unaccepted.json"
  echo "Saved the immutable baseline binary: $binary"
  exit 0
fi

comparison_mode="initial-candidate"
if [[ "$mode" == *-previous ]]; then
  comparison_mode="previous-release"
  python3 scripts/verify-performance-evidence.py baseline "$3" \
    --binary "$binary" --toolchain "$toolchain" \
    --comparison-mode "$comparison_mode" --baseline-binary "$4"
else
  python3 scripts/verify-performance-evidence.py baseline "$3" \
    --binary "$binary" --toolchain "$toolchain" \
    --comparison-mode "$comparison_mode"
fi
if [[ "$mode" == verify-* ]]; then
  exit 0
fi

if [[ "$comparison_mode" == "previous-release" ]]; then
  "$binary" compare \
    --toolchain "$toolchain" \
    --samples 31 \
    --iterations 100 \
    --comparison-mode "$comparison_mode" \
    --baseline "$3" \
    --baseline-binary "$4" \
    --output "$output_root/benchmark.json"
else
  "$binary" compare \
    --toolchain "$toolchain" \
    --samples 31 \
    --iterations 100 \
    --comparison-mode "$comparison_mode" \
    --baseline "$3" \
    --output "$output_root/benchmark.json"
fi
if [[ "$comparison_mode" == "previous-release" ]]; then
  python3 scripts/verify-performance-evidence.py benchmark "$output_root/benchmark.json" \
    --baseline "$3" --binary "$binary" --toolchain "$toolchain" \
    --comparison-mode "$comparison_mode" --baseline-binary "$4"
else
  python3 scripts/verify-performance-evidence.py benchmark "$output_root/benchmark.json" \
    --baseline "$3" --binary "$binary" --toolchain "$toolchain" \
    --comparison-mode "$comparison_mode"
fi
