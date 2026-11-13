#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
temporary_root="$(mktemp -d "$repo_root/target/.usability-trace.XXXXXX")"
cleanup() {
  rm -rf -- "$temporary_root"
}
trap cleanup EXIT HUP INT TERM

spec_trace="$temporary_root/spec.trace"
python3 "$repo_root/scripts/usability_acceptance_trace.py" >"$spec_trace"

inventory="$temporary_root/test.inventory"
: >"$inventory"

append_inventory() {
  local test_prefix="$1"
  shift
  cargo +1.95.0 test "$@" -- --list \
    | awk -v test_prefix="$test_prefix" '/: test$/ {
        sub(/: test$/, "")
        print test_prefix "::" $0
      }' >>"$inventory"
}

append_inventory chigiri_testkit::usability \
  --locked -p chigiri_testkit --test usability
append_inventory chigiri --locked -p chigiri --lib
append_inventory chigiri_testkit::provider_conformance \
  --locked -p chigiri_testkit --test provider_conformance
# The rate SPI is the historical trace name left before the move.
cargo +1.95.0 test --locked -p chigiri_rate_memory --test algorithms -- --list \
  | awk '/: test$/ {
      sub(/: test$/, "")
      sub(/^store_clock::store::/, "limiters::store::")
      sub(/^store_clock::clock::/, "limiters::clock::")
      print "chigiri_policy::" $0
    }' >>"$inventory"
cargo +1.95.0 test --locked -p chigiri_cli --test rust_traits -- --list \
  | awk '/: test$/ {
      sub(/: test$/, "")
      print "chigiri_cli::rust_traits::" $0
    }' >>"$inventory"
append_inventory chigiri_projection_consumer --offline --locked \
  --manifest-path crates/codegen/tests/compile/pass/Cargo.toml
append_inventory chigiri_custom_scalar_output --offline --locked \
  --manifest-path crates/codegen/tests/compile/custom_scalar_output/Cargo.toml
append_inventory chigiri_books::end_to_end \
  --locked -p chigiri_books --test end_to_end
append_inventory chigiri_axum --locked -p chigiri_axum --lib
append_inventory chigiri_hyper --locked -p chigiri_hyper --lib

python3 - "$spec_trace" "$inventory" "$repo_root" <<'PY'
import re
import sys
from pathlib import Path

trace = Path(sys.argv[1]).read_text(encoding="utf-8")
inventory = {
    line
    for line in Path(sys.argv[2]).read_text(encoding="utf-8").splitlines()
    if line
}
repo_root = Path(sys.argv[3])
contract_path = repo_root / "scripts/check-usability-contract.sh"
contract_source = contract_path.read_text(encoding="utf-8")


def validate_automatic_contract(candidate: str) -> None:
    required = {
        "facade full-source inventory": 'facade.rglob("*.rs")',
        "Factory reference path fixed": 'reference_files != ["endpoint/request_identity.rs"]',
        "direct constructor mutation": 'changed["components.rs"] += "\\nRandomRequestIdFactory::new();\\n"',
        "alias constructor mutation": "as AutomaticIds",
        "initializer relocation mutation": '"initialize_before_build("',
    }
    for label, fragment in required.items():
        if fragment not in candidate:
            raise ValueError(f"automatic contract evidence missing: {label}")


def validate(candidate: str, current_inventory: set[str] = inventory) -> None:
    script_only_rows = {
        "API-10": "scripts/check-usability-contract.sh",
        "INJ-12": "scripts/check-usability-contract.sh",
        "PROJ-06": "scripts/check-usability-contract.sh",
        "PROJ-12": "scripts/check-large-consumer-conformance.sh",
        "PROJ-13": "scripts/check-usability-contract.sh",
        "LIMIT-16": "scripts/check-usability-contract.sh",
    }
    mixed_script_rows = {"API-06": "scripts/check-usability-contract.sh"}
    known_scripts = set(script_only_rows.values()) | set(mixed_script_rows.values())
    remapped = {
        "PROJ-07": "trait_completion_and_policy_not_circumvented",
        "PROJ-09": "ri_01_08_private_adapters_and_assembly_stages_are_hidden_from_rustdoc",
        "PROJ-10": "ri_07_detects_private_dispatch_drift_without_writes",
        "PROJ-11": "ri_07_large_generated_semantic_units_keep_resource_bounds",
    }
    for line in candidate.splitlines():
        row_id, evidence = line.split("\t", 1)
        references = re.findall(r"`([^`]+)`", evidence)
        expected_script = script_only_rows.get(row_id)
        if expected_script is not None:
            if references != [expected_script]:
                raise ValueError(f"trace script evidence mismatch: {row_id}")
            if not (repo_root / expected_script).is_file():
                raise ValueError(f"trace script evidence missing: {row_id}")
        else:
            required_script = mixed_script_rows.get(row_id)
            if required_script is not None and required_script not in references:
                raise ValueError(f"trace mixed script evidence missing: {row_id}")
            unexpected_scripts = sorted(
                set(references) & (known_scripts - {required_script})
            )
            if unexpected_scripts:
                raise ValueError(f"trace script evidence is in wrong row: {row_id}")
            cargo_references = [
                reference for reference in references if reference not in known_scripts
            ]
            unknown = sorted(set(cargo_references) - current_inventory)
            if unknown:
                raise ValueError(
                    "trace evidence is not in the closed inventory: "
                    + ", ".join(unknown)
                )
            if not cargo_references:
                raise ValueError(f"trace cargo evidence missing: {row_id}")
            prefix, number = row_id.lower().split("-")
            marker = remapped.get(row_id, f"{prefix}_{number}")
            if not any(
                marker in reference.rsplit("::", 1)[-1]
                for reference in cargo_references
            ):
                raise ValueError(f"trace row ID mismatch: {row_id}")
    validate_automatic_contract(contract_source)


try:
    validate(trace)
except ValueError as error:
    raise SystemExit(str(error)) from error

def expect_invalid(before: str, after: str, label: str) -> None:
    mutated = trace.replace(before, after, 1)
    if mutated == trace:
        raise SystemExit("The fixture is not a mutation target.")
    try:
        validate(mutated)
    except ValueError:
        return
    raise SystemExit("Acceptance-trace mutation was not rejected.")


expect_invalid(
    "chigiri_cli::rust_traits::ri_01_08_private_adapters",
    "chigiri_cli::rust_traitx::ri_01_08_private_adapters",
    "trace module",
)
expect_invalid(
    "chigiri_policy::limiters::store::quota_reject",
    "chigiri_policy::limiters::store::quota_rejecx",
    "trace auxiliary",
)
expect_invalid(
    "chigiri_policy::limiters::store::quota_reject",
    "qythia_policy::limiters::store::quota_reject",
    "trace auxiliary prefix",
)
expect_invalid(
    "chigiri_testkit::usability::endpoint_builder::api_02_applies_safe_baseline_without_additional_calls",
    "chigiri_testkit::usability::endpoint_builder::api_01_single_builder_reaches_query_mutation_get",
    "trace row ID",
)
expect_invalid("::ri_01_08_private_adapters", "::invalid_private_stage", "trace prefix")
expect_invalid(
    "`chigiri_policy::limiters::store::quota_rejection_keeps_active_keys_from_idle_expiry_for_all_four_algorithms`",
    "`chigiri_policy::limiters::store::quota_rejection_keeps_active_keys_from_idle_expiry_for_all_four_algorithms`, `unknown_reference`",
    "trace unknown reference",
)

custom_scalar_inventory = {
    reference
    for reference in inventory
    if not reference.startswith("chigiri_custom_scalar_output::")
}
try:
    validate(trace, custom_scalar_inventory)
except ValueError:
    pass
else:
    raise SystemExit("Missing custom-scalar consumer inventory was not rejected.")

script_reference = "`scripts/check-usability-contract.sh`"
wrong_row = []
for line in trace.splitlines():
    if line.startswith("API-06\t"):
        line = line.replace(", " + script_reference, "", 1)
    elif line.startswith("API-05\t"):
        line += ", " + script_reference
    wrong_row.append(line)
try:
    validate("\n".join(wrong_row) + "\n")
except ValueError:
    pass
else:
    raise SystemExit("Incorrect API-06 script evidence row was not rejected.")

for fragment in [
    'changed["components.rs"] += "\\nRandomRequestIdFactory::new();\\n"',
    "as AutomaticIds",
    '"initialize_before_build("',
]:
    mutated = contract_source.replace(fragment, "", 1)
    try:
        validate_automatic_contract(mutated)
    except ValueError:
        continue
    raise SystemExit("Deleted automatic source-mutation evidence was not rejected.")
PY

echo "Usability acceptance trace passed."
