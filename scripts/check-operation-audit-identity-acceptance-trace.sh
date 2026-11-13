#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
temporary_root="$(mktemp -d "$repo_root/target/.audit-identity-trace.XXXXXX")"
cleanup() {
  rm -rf -- "$temporary_root"
}
trap cleanup EXIT HUP INT TERM

expected="$temporary_root/expected"
for number in $(seq 1 8); do
  printf 'AUDIT-ID-%02d\n' "$number"
done >"$expected"

trace="$temporary_root/trace"
awk -F '|' '
  $2 ~ /^[[:space:]]*AUDIT-ID-[0-9][0-9][[:space:]]*$/ {
    id = $2
    evidence = $3
    gsub(/^[[:space:]]+|[[:space:]]+$/, "", id)
    gsub(/^[[:space:]]+|[[:space:]]+$/, "", evidence)
    print id "\t" evidence
  }
' "$repo_root/docs/spec-conformance.md" | sort >"$trace"

cut -f1 "$trace" >"$trace.ids"
if ! diff -u "$expected" "$trace.ids"; then
  echo "The operation audit identity IDs are not the exact eight-ID set." >&2
  exit 1
fi

inventory="$temporary_root/inventory"
{
  cargo +1.95.0 test --locked -p chigiri_operation --test audit_identity -- --list \
    | awk '/: test$/ {
        sub(/: test$/, "")
        print "chigiri_operation::audit_identity::" $0
      }'
  cargo +1.95.0 test --locked -p chigiri_testkit --test operation_http -- --list \
    | awk '/: test$/ {
        sub(/: test$/, "")
        print "chigiri_testkit::operation_http::" $0
      }'
} >"$inventory"

python3 - "$trace" "$inventory" <<'PY'
import re
import sys
from pathlib import Path

trace = Path(sys.argv[1]).read_text(encoding="utf-8")
inventory = set(Path(sys.argv[2]).read_text(encoding="utf-8").splitlines())
owners = {
    "AUDIT-ID-01": "chigiri_operation::audit_identity::hit_identity",
    "AUDIT-ID-02": "chigiri_operation::audit_identity::miss_identity",
    "AUDIT-ID-03": "chigiri_operation::audit_identity::provider_failure_identity",
    "AUDIT-ID-04": "chigiri_operation::audit_identity::sink_failure_identity",
    "AUDIT-ID-05": "chigiri_operation::audit_identity::missing_telemetry",
    "AUDIT-ID-06": "chigiri_operation::audit_identity::sampling_drop",
    "AUDIT-ID-07": "chigiri_operation::audit_identity::parallel_identity",
    "AUDIT-ID-08": "chigiri_testkit::operation_http::audit_id_08_axum_response_and_standard_diagnostic_are_redacted",
}


def validate(candidate: str) -> None:
    rows = [line for line in candidate.splitlines() if line]
    if len(rows) != 8:
        raise ValueError("The operation-audit trace must contain exactly eight rows.")
    seen: set[str] = set()
    for line in rows:
        row_id, evidence = line.split("\t", 1)
        if row_id in seen:
            raise ValueError(f"Duplicate operation-audit trace ID: {row_id}")
        seen.add(row_id)
        if row_id not in owners:
            raise ValueError(f"Unknown operation-audit trace ID: {row_id}")
        references = re.findall(r"`([^`]+)`", evidence)
        if references != [owners[row_id]]:
            raise ValueError(f"The exact owner does not match for {row_id}.")
        if references[0] not in inventory:
            raise ValueError(f"Owner test is missing for {row_id}.")
        if "Planned" in evidence or "Scheduled" in evidence:
            raise ValueError(f"Planned evidence lacks an exact owner for {row_id}.")
    if seen != set(owners):
        raise ValueError("The operation-audit trace IDs do not match the exact eight-ID set.")


validate(trace)


def expect_invalid(candidate: str, label: str) -> None:
    try:
        validate(candidate)
    except ValueError:
        return
    raise SystemExit(f"{label} mutation was not rejected.")


first = trace.splitlines()[0] + "\n"
expect_invalid(trace.replace(first, "", 1), "A required value is missing.")
expect_invalid(trace.replace("AUDIT-ID-01\t", "AUDIT-ID-02\t", 1), "Duplicate ID.")
expect_invalid(trace.replace("AUDIT-ID-01\t", "AUDIT-ID-09\t", 1), "unknown ID")
expect_invalid(trace.replace("::hit_identity`", "::missing_owner`", 1), "The owner is gone.")
PY

echo "Operation audit identity acceptance trace passed: 8 IDs, exact owners, and mutation checks"
