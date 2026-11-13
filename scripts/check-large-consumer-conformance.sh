#!/usr/bin/env bash
set -u

# The receiving trace: generated-drift/abi-mismatch,
# Legacy assertion IDs below remain stable; execution now uses fixed Rust 1.95.0.
# MSRV/stable-positive-negative-doctor, codegen-and-drift,
# MSRV/stable-positive-doctor, public-rustdoc-hidden-scan,
# provider-third-party, provider-negative, provider-official,
# fixture-and-formal-performance-scope.
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
revision="$(git -C "$repo_root" rev-parse HEAD)"
dirty=false
if [[ -n "$(git -C "$repo_root" status --porcelain)" ]]; then
  dirty=true
fi
source "$repo_root/scripts/large-consumer-suite.sh"
source "$repo_root/scripts/large-consumer-evidence.sh"
fixture_root="$repo_root/tests/fixtures/vnext-large-product"
evidence_root="$repo_root/target/production-evidence"
mkdir -p "$repo_root/target/sibling-generator" || exit 1
temporary_root="$(mktemp -d "$repo_root/target/sibling-generator/.large-conformance.XXXXXX")" || exit 1
evidence_stage="$(create_large_consumer_evidence_stage "$evidence_root")"
work_evidence="$temporary_root/evidence"
mkdir -p "$work_evidence"
case_roots=()

cleanup() {
  local exit_status=$?
  local retained
  if (( exit_status != 0 || ${overall_status:-0} != 0 )) && [[ -d "$work_evidence" ]]; then
    if retained="$(retain_large_consumer_failure "$work_evidence" "$evidence_root")"; then
      echo "Failed large-consumer raw evidence retained: $retained" >&2
    else
      echo "Could not retain failed large-consumer raw evidence." >&2
    fi
  fi
  rm -rf -- "$temporary_root"
  local case_root
  set +u
  for case_root in "${case_roots[@]}"; do
    rm -rf -- "$case_root"
  done
  set -u
  if [[ -n "$evidence_stage" ]]; then
    rm -rf -- "$evidence_stage"
  fi
}
trap cleanup EXIT HUP INT TERM

cp -R "$fixture_root/consumer" "$temporary_root/consumer"
cp -R "$fixture_root/graphql" "$temporary_root/graphql"
cp "$fixture_root/chigiri.toml" "$temporary_root/chigiri.toml"
consumer_manifest="$temporary_root/consumer/Cargo.toml"
generated_dir="$temporary_root/generated"
consumer_target="$repo_root/target/large-consumer"
export CARGO_TARGET_DIR="$consumer_target"
evidence_prefix="$work_evidence/large-consumer"
tool_manifest="$repo_root/Cargo.toml"
overall_status=0

if ! CARGO_TARGET_DIR="$repo_root/target" sh "$repo_root/tools/standards.sh" \
  build --artifact chigiri-cli --profile debug; then
  echo "The Chigiri CLI build failed." >&2
  exit 1
fi
chigiri_bin="$(python3 "$repo_root/scripts/product_cli.py")" || exit 1

record_failure() {
  echo "$1" >&2
  overall_status=1
}

run_from_fixture() {
  local label="$1"
  local timeout="$2"
  shift 2
  run_large_consumer_command 1.95.0 "$label" "$evidence_prefix" "$timeout" \
    /bin/sh -c 'cd "$1" && shift && exec "$@"' _ "$temporary_root" "$@"
}

codegen_command=("$chigiri_bin" rust)
if ! run_from_fixture codegen 180 "${codegen_command[@]}" --manifest chigiri.toml codegen; then
  record_failure "The large consumer generation failed."
  exit "$overall_status"
fi
if ! run_from_fixture drift 180 "${codegen_command[@]}" --manifest chigiri.toml check; then
  record_failure "The large consumer generated drift test failed."
fi

shape_json="$work_evidence/shape.json"
if ! python3 "$repo_root/scripts/large-consumer-contract.py" shape \
  "$temporary_root/graphql/schema.graphql" "$temporary_root/consumer" \
  --output "$shape_json"; then
  record_failure "The large consumer shape test failed."
fi

doctor_fixed=0
for toolchain in 1.95.0; do
  doctor_status=0
  run_large_consumer_command "$toolchain" doctor-positive "$evidence_prefix" 180 \
    /bin/sh -c \
    'cd "$1" && CARGO_TARGET_DIR="$2" RUSTUP_TOOLCHAIN="$3" "$4" rust --manifest chigiri.toml doctor --format json --deny-warnings' \
    _ "$temporary_root" "$consumer_target" \
    "$toolchain" "$chigiri_bin" || doctor_status=$?
  doctor_fixed=$doctor_status
  if (( doctor_status != 0 )); then
    record_failure "The large consumer $toolchain positive doctor failed: $doctor_status."
  fi
  doctor_log="$evidence_prefix-$toolchain-doctor-positive.log"
  if ! python3 "$repo_root/scripts/large-consumer-contract.py" positive "$doctor_log"; then
    record_failure "Large-consumer positive doctor lacks a Cargo diagnostic on $toolchain."
  fi
done

for toolchain in 1.95.0; do
  if ! run_large_consumer_suite "$toolchain" "$consumer_manifest" "$consumer_target" \
    "$evidence_prefix" 180 "$(python3 -c 'from pathlib import Path; import sys; print(sum(p.stat().st_size for p in Path(sys.argv[1]).rglob("*.rs")))' "$generated_dir")" \
    -p chigiri_vnext_large_consumer -p chigiri-large-contract; then
    record_failure "The large consumer $toolchain five-command suite failed."
    cat "$evidence_prefix-$toolchain.log" >&2
    for step in fmt check test clippy rustdoc; do
      result="$evidence_prefix-$toolchain-$step.json"
      if [[ -f "$result" && "$(jq -r '.exitCode' "$result")" != 0 ]]; then
        tail -n 80 "$evidence_prefix-$toolchain-$step.log" >&2
      fi
    done
  fi
done
if ! run_large_consumer_command 1.95.0 sdk-fmt "$evidence_prefix" 180 \
  cargo +1.95.0 fmt --manifest-path "$generated_dir/Cargo.toml" -- --check; then
  record_failure "The pinned-format check of the large generated SDK failed."
fi
if ! python3 "$repo_root/scripts/large_consumer_rustdoc.py" "$consumer_target/doc"; then
  record_failure "Large-consumer public rustdoc visibility check failed."
fi

provider_target="$repo_root/target/large-consumer-provider"
provider_status=0
run_large_consumer_command 1.95.0 provider-third-party "$evidence_prefix" 180 \
  /bin/sh -c 'set -e; cargo +1.95.0 test --locked --manifest-path "$1" --target-dir "$2"' \
  _ "$repo_root/tests/fixtures/rate-provider-minimal/Cargo.toml" "$provider_target" || provider_status=$?
provider_third_party=false
if (( provider_status == 0 )) && \
  [[ "$(cargo metadata --locked --no-deps --format-version 1 \
    --manifest-path "$repo_root/tests/fixtures/rate-provider-minimal/Cargo.toml" | \
    jq -r '.packages[0].dependencies[] | select(.kind == null) | .name')" == "chigiri_rate" ]]; then
  provider_third_party=true
else
  record_failure "The third-party minimal provider contract failed."
fi

provider_status=0
run_large_consumer_command 1.95.0 provider-negative "$evidence_prefix" 180 \
  cargo +1.95.0 test --locked --manifest-path "$tool_manifest" -p chigiri_testkit \
  --test provider_conformance -- --skip official || provider_status=$?
provider_negative=false
if (( provider_status == 0 )); then
  provider_negative=true
else
  record_failure "The negative provider conformance failed."
fi

provider_status=0
run_large_consumer_command 1.95.0 provider-official "$evidence_prefix" 180 \
  /bin/sh -c 'set -e; cargo +1.95.0 test --locked --manifest-path "$1" -p chigiri_testkit --test provider_conformance official::; cargo +1.95.0 test --locked --manifest-path "$1" -p chigiri_rate_memory --tests' \
  _ "$tool_manifest" || provider_status=$?
provider_official=false
if (( provider_status == 0 )); then
  provider_official=true
else
  record_failure "The official provider conformance failed."
fi

diagnostic_jsons=()
for case_name in missing-group missing-field wrong-output \
  missing-root generated-drift abi-mismatch; do
  case_root="$(mktemp -d "$repo_root/target/sibling-generator/.large-negative-${case_name}.XXXXXX")"
  case_roots+=("$case_root")
  rsync -a --exclude target --exclude evidence "$temporary_root/" "$case_root/"
  if ! python3 "$repo_root/scripts/large-consumer-contract.py" mutate \
    "$case_name" "$case_root"; then
    record_failure "Could not create negative doctor mutation: $case_name"
    continue
  fi
  negative_prefix="$work_evidence/doctor-negative"
  for toolchain in 1.95.0; do
    negative_status=0
    run_large_consumer_command "$toolchain" "doctor-negative-$case_name" \
      "$negative_prefix" 180 /bin/sh -c \
      'cd "$1" && CARGO_TARGET_DIR="$2" RUSTUP_TOOLCHAIN="$3" "$4" rust --manifest chigiri.toml doctor --format json --deny-warnings' \
      _ "$case_root" "$consumer_target" \
      "$toolchain" "$chigiri_bin" || negative_status=$?
    negative_log="$negative_prefix-$toolchain-doctor-negative-$case_name.log"
    diagnostic_json="$work_evidence/diagnostic-$toolchain-$case_name.json"
    if ! python3 "$repo_root/scripts/large-consumer-contract.py" diagnostic \
      "$case_name" "$toolchain" "$negative_log" "$negative_status" \
      --output "$diagnostic_json"; then
      record_failure "Negative doctor diagnostic contract failed: $toolchain $case_name"
    else
      diagnostic_jsons+=("$diagnostic_json")
    fi
  done
done

# Fixture isolation uses source inputs; formal qualification has separate evidence.
if rg -n '(/Users/|/Volumes/|\.\./lab[0-9]+)' "$fixture_root" >/dev/null; then
  record_failure "Large-consumer scope boundary is invalid."
fi

if (( overall_status != 0 )); then
  exit "$overall_status"
fi
command_args=()
while IFS= read -r command_json; do
  command_args+=(--command "$command_json")
done < <(find "$work_evidence" -maxdepth 1 -type f -name '*.json' \
  ! -name 'shape.json' ! -name 'diagnostic-*.json' | sort)
diagnostic_args=()
for diagnostic_json in "${diagnostic_jsons[@]}"; do
  diagnostic_args+=(--diagnostic "$diagnostic_json")
done
if ! bash "$repo_root/scripts/python-with-toml.sh" "$repo_root/scripts/large-consumer-evidence.py" create \
  --output "$evidence_stage/evidence.json" --repository "$repo_root" \
  --generated "$generated_dir" --application "$temporary_root/consumer" \
  --revision "$revision" --dirty "$dirty" --shape "$shape_json" \
  --positive "$doctor_fixed" \
  "${diagnostic_args[@]}" "${command_args[@]}" \
  --provider-third-party --provider-negative --provider-official; then
  record_failure "Failed to produce large-consumer evidence."
  exit "$overall_status"
fi
if ! validate_large_consumer_evidence_stage "$evidence_stage" "$revision"; then
  record_failure "We couldn't verify the large consumer evidence."
  exit "$overall_status"
fi
run_id="${evidence_stage##*/}"
if ! published="$(publish_large_consumer_evidence \
  "$evidence_stage" "$evidence_root" "$run_id")"; then
  record_failure "Failed to publish large-consumer evidence."
  exit "$overall_status"
fi
evidence_stage=""
echo "Large-consumer conformance passed: $published/evidence.json"
