#!/usr/bin/env bash
set -euo pipefail

# Test a large consumer outside its dedicated SDK while reusing the Cargo cache.
repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
fixture_root="$repository_root/tests/fixtures/vnext-large-product"
source "$repository_root/scripts/large-consumer-suite.sh"
mkdir -p "$repository_root/target/sibling-generator"
temporary_root="$(mktemp -d "$repository_root/target/sibling-generator/.vnext-consumer.XXXXXX")"
trap 'rm -rf -- "$temporary_root"' EXIT HUP INT TERM

cp -R "$fixture_root/consumer" "$temporary_root/consumer"
cp -R "$fixture_root/graphql" "$temporary_root/graphql"
cp "$fixture_root/chigiri.toml" "$temporary_root/chigiri.toml"
consumer_manifest="$temporary_root/consumer/Cargo.toml"
consumer_target="$repository_root/target/vnext-consumer"
generated_dir="$temporary_root/generated"
evidence_prefix="$temporary_root/evidence"
export CARGO_TARGET_DIR="$consumer_target"

for action in codegen check; do
    run_large_consumer_command 1.95.0 "$action" "$evidence_prefix" 180 \
        cargo +1.95.0 run --locked --quiet --manifest-path "$repository_root/Cargo.toml" \
        -p chigiri_cli --bin chigiri -- rust \
        --manifest "$temporary_root/chigiri.toml" "$action" --deny-warnings
done
generated_bytes="$(python3 -c 'from pathlib import Path; import sys; print(sum(p.stat().st_size for p in Path(sys.argv[1]).rglob("*.rs")))' "$generated_dir")"
for toolchain in 1.95.0; do
    run_large_consumer_suite "$toolchain" "$consumer_manifest" "$consumer_target" \
        "$evidence_prefix" 180 "$generated_bytes" \
        -p chigiri_vnext_large_consumer -p chigiri-large-contract
done
run_large_consumer_command 1.95.0 sdk-fmt "$evidence_prefix" 180 \
    cargo +1.95.0 fmt --manifest-path "$generated_dir/Cargo.toml" -- --check
