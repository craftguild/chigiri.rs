#!/usr/bin/env bash
set -euo pipefail

# consumer-quality-five-commands: Run five quality checks on path-dependent consumers.

if (( $# < 3 )); then
  echo "usage: check-cargo-consumer.sh TOOLCHAIN MANIFEST TARGET [PACKAGE_ARGUMENTS...]" >&2
  exit 2
fi
toolchain="$1"
manifest="$2"
target="$3"
shift 3

# Shared consumer quality checks that do not require a distribution archive or clean HEAD.

cargo "+$toolchain" fmt --manifest-path "$manifest" -- --check
cargo "+$toolchain" check --locked --manifest-path "$manifest" \
  --target-dir "$target" --all-targets --all-features "$@"
cargo "+$toolchain" test --locked --manifest-path "$manifest" \
  --target-dir "$target" --all-targets --all-features "$@"
cargo "+$toolchain" clippy --locked --manifest-path "$manifest" \
  --target-dir "$target" --all-targets --all-features "$@" -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo "+$toolchain" doc --locked \
  --manifest-path "$manifest" --target-dir "$target" --no-deps --all-features "$@"
