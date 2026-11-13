#!/bin/sh
set -eu

# Test the neutral consumer as an independent package outside the workspace on the MSRV.
repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
fixture_manifest="$repository_root/tests/fixtures/neutral-endpoint-consumer/Cargo.toml"
cargo +1.95.0 check --manifest-path "$fixture_manifest" --locked

# Fail if framework or runtime crates enter the normal dependency graph.
dependency_tree=$(cargo +1.95.0 tree --locked --manifest-path "$fixture_manifest" --edges normal --prefix none)
if printf '%s\n' "$dependency_tree" | grep -E '^(axum|hyper|hyper-util|tokio|tower) v'; then
  echo "The neutral endpoint consumer has a forbidden framework/runtime dependency." >&2
  exit 1
fi

# The positive/negative test for runtime is run by the validate MSRV/stable workspace suite.
