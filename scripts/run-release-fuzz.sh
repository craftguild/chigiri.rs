#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

check_fuzz_toolchain() {
  if ! cargo +nightly --version >/dev/null 2>&1; then
    echo "A nightly toolchain is required: rustup toolchain install nightly" >&2
    return 2
  fi
  if ! cargo +nightly fuzz --version >/dev/null 2>&1; then
    echo "cargo-fuzz is required: cargo +nightly install cargo-fuzz --locked" >&2
    return 2
  fi
}

if [[ "${1:-}" == "--check-toolchain" ]]; then
  if (( $# != 1 )); then
    echo "usage: run-release-fuzz.sh --check-toolchain" >&2
    exit 2
  fi
  check_fuzz_toolchain
  echo "Release fuzz toolchain is available."
  exit 0
fi
targets=(parser sdl_composition http_decode json_conversion persisted_id config_parser)
if (( $# == 2 )) && [[ "$1" == "--target" ]]; then
  case "$2" in
    parser|sdl_composition|http_decode|json_conversion|persisted_id|config_parser)
      targets=("$2")
      ;;
    *) echo "Unknown fuzz target." >&2; exit 2 ;;
  esac
elif (( $# != 0 )); then
  echo "usage: run-release-fuzz.sh [--check-toolchain | --target TARGET]" >&2
  exit 2
fi

seconds="${CHIGIRI_FUZZ_SECONDS:-1800}"
if [[ ! "$seconds" =~ ^[1-9][0-9]*$ ]]; then
  echo "Please specify CHIGIRI_FUZZ_SECONDS in positive integers." >&2
  exit 2
fi
check_fuzz_toolchain

mkdir -p target/production-evidence/fuzz
output_root="$(mktemp -d "$repo_root/target/production-evidence/fuzz/run-XXXXXX")"
git rev-parse HEAD >"$output_root/revision.txt" 2>/dev/null || echo unknown >"$output_root/revision.txt"
rustc +nightly --version --verbose >"$output_root/rustc.txt"
cargo +nightly fuzz --version >"$output_root/cargo-fuzz.txt"
uname -srm >"$output_root/platform.txt"

snapshot() {
  python3 - "$repo_root" "$output_root/source.json" <<'PY'
import json
import sys
from pathlib import Path
root, path = map(Path, sys.argv[1:])
sys.path.insert(0, str(root / "scripts"))
from validation_evidence import source_snapshot
current = source_snapshot(root)
if path.exists():
    if json.loads(path.read_text()) != current:
        raise SystemExit("Source changed during fuzz qualification.")
else:
    path.write_text(json.dumps(current, indent=2) + "\n")
PY
}

snapshot
for target in "${targets[@]}"; do
  log="$output_root/$target.log"
  corpus="$output_root/corpus/$target"
  artifacts="$output_root/artifacts/$target"
  mkdir -p "$corpus" "$artifacts"
  python3 - "$repo_root" "$target" "$corpus" <<'PY'
import sys
from pathlib import Path
root = Path(sys.argv[1])
sys.path.insert(0, str(root / "scripts"))
from validation_sources import copy_corpus
copy_corpus(root, sys.argv[2], Path(sys.argv[3]))
PY
  cargo +nightly fuzz run "$target" "$corpus" -- \
    -max_len=262144 -max_total_time="$seconds" -timeout=10 -rss_limit_mb=2048 \
    -artifact_prefix="$artifacts/" \
    2>&1 | tee "$log"
  if grep -En -- "ERROR:|SUMMARY: .*Sanitizer|crash-|timeout-|oom-" "$log"; then
    echo "Fuzzing detected a failure in $target." >&2
    exit 1
  fi
done
snapshot
echo "All ${#targets[@]} selected release fuzz targets completed; each ran for ${seconds} seconds."
