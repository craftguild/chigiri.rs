#!/bin/sh
set -eu

# Test the backend-neutral consumer independently on the MSRV toolchain.
repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
fixture_manifest="$repository_root/tests/fixtures/observability-opt-in-consumer/Cargo.toml"
cargo +1.95.0 check --manifest-path "$fixture_manifest" --locked

# Tracing or OpenTelemetry in normal dependencies violates the opt-in boundary.
dependency_tree=$(cargo +1.95.0 tree --locked --manifest-path "$fixture_manifest" --edges normal --prefix none)
if printf '%s\n' "$dependency_tree" | grep -E '(tracing|opentelemetry) v'; then
  echo "An observability-backend dependency leaked into the backend-neutral consumer." >&2
  exit 1
fi

# The positive/negative test for runtime is run by the validate MSRV/stable workspace suite.
