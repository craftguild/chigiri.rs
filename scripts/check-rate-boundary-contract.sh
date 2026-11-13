#!/usr/bin/env bash
set -euo pipefail

# The receiving trace: chigiri_rate-normal-dependency-zero-and-toolchains.
# forbidden-SPI-surface-scan, policy-composition-owner-scan,
# facade-rate-reexport-and-usability, legacy-policy-SPI-zero,
# generated-ABI-7, chigiri_rate_memory-only-atomic-waker-rate-and-toolchains,
# legacy-memory-surface-zero.
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
temporary_root="$(mktemp -d "${TMPDIR:-/tmp}/chigiri-rate-contract.XXXXXX")"
cleanup() {
  rm -rf -- "$temporary_root"
}
trap cleanup EXIT HUP INT TERM

normal_dependencies() {
  local manifest="$1"
  local package="$2"
  cargo metadata --locked --format-version 1 --no-deps \
    --manifest-path "$manifest" | jq -r --arg package "$package" \
    '.packages[] | select(.name == $package) | .dependencies[] |
     select(.kind == null or .kind == "normal") | .name' | LC_ALL=C sort
}

assert_exact_lines() {
  local expected="$1"
  local actual="$2"
  if ! diff -u "$expected" "$actual"; then
    echo "The rate crate normal dependency set is inconsistent." >&2
    exit 1
  fi
}

: >"$temporary_root/empty"
normal_dependencies "$repo_root/crates/rate/Cargo.toml" chigiri_rate \
  >"$temporary_root/rate-dependencies"
assert_exact_lines "$temporary_root/empty" "$temporary_root/rate-dependencies"
printf '%s\n' atomic-waker chigiri_rate >"$temporary_root/memory-expected"
normal_dependencies "$repo_root/crates/rate-memory/Cargo.toml" chigiri_rate_memory \
  >"$temporary_root/memory-dependencies"
assert_exact_lines "$temporary_root/memory-expected" \
  "$temporary_root/memory-dependencies"
printf '%s\n' chigiri_rate >"$temporary_root/provider-expected"
normal_dependencies \
  "$repo_root/tests/fixtures/rate-provider-minimal/Cargo.toml" \
  chigiri-rate-provider-minimal >"$temporary_root/provider-dependencies"
assert_exact_lines "$temporary_root/provider-expected" \
  "$temporary_root/provider-dependencies"

for forbidden in chigiri_http chigiri_runtime chigiri_policy chigiri_rate_memory \
  tokio serde InMemory; do
  if rg -n -- "$forbidden" crates/rate/src crates/rate/Cargo.toml; then
    echo "chigiri_rate contains a forbidden dependency or implementation: $forbidden." >&2
    exit 1
  fi
done
if rg -n 'pub (fn|trait) (check|consume|reserve|commit|permit|release)\b' \
  crates/rate/src; then
  echo "A public entry point splits the atomic provider SPI." >&2
  exit 1
fi
if rg -n 'pub use (chigiri_rate|crate::.*RateLimit|crate::.*QuotaKey)' \
  crates/policy/src; then
  echo "The obsolete SPI re-export is still present in chigiri_policy." >&2
  exit 1
fi
for removed in clock.rs key.rs rate.rs rate/fixed_window.rs rate/token_bucket.rs; do
  if [[ -e "$repo_root/crates/policy/src/$removed" ]]; then
    echo "Obsolete rate implementation remains in chigiri_policy: $removed" >&2
    exit 1
  fi
done
if rg -n '\bFakeClock\b|chigiri_policy::(OperationCost|QuotaKey|QuotaSubject|RateLimiter|RateLimitRequest|RateLimitDecision|RateLimitSnapshot|RateLimitProviderError|RateLimitRejectReason|RateLimitPolicyError|RatePolicy|TokenBucketPolicy|FixedWindowPolicy|SlidingWindowPolicy|LeakyBucketPolicy|MonotonicTick|Clock|SystemClock|InMemory)' \
  --glob '*.rs' crates examples tests; then
  echo "A repository consumer still uses the obsolete rate surface." >&2
  exit 1
fi
if rg -n -U 'InMemory(TokenBucket|FixedWindow|SlidingWindowCounter|LeakyBucket)::new\([^)]*,' \
  --glob '*.rs' crates examples tests; then
  echo "The obsolete in-memory constructor is still present." >&2
  exit 1
fi
rg -Fqx 'pub const GENERATED_ABI_VERSION: u32 = 8;' \
  crates/runtime/src/generated.rs
rg -Fq 'pub use chigiri_rate::{' crates/facade/src/rate.rs
rg -Fq 'pub mod memory {' crates/facade/src/rate.rs

# rmem-12-bounded-waiter-wake-drop-official: Workspace tests cover both the mutex and the official common suite.
# Provider independence is covered by both toolchain suites and the large-consumer gate.

echo "Rate boundary contract checks passed."
