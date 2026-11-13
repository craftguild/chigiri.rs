#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
temporary_root="$(mktemp -d "$repo_root/target/.dx-rate-trace.XXXXXX")"
trap 'rm -rf -- "$temporary_root"' EXIT HUP INT TERM

expected="$temporary_root/expected"
{
  for specification in DIAG:15 RSPI:12 RMEM:16 LCON:16; do
    prefix="${specification%%:*}"
    count="${specification##*:}"
    for number in $(seq 1 "$count"); do
      [[ "$prefix-$number" == "LCON-4" ]] && continue
      printf '%s-%02d\n' "$prefix" "$number"
    done
  done
} | sort >"$expected"

trace="$temporary_root/trace"
awk -F '|' '
  $2 ~ /^[[:space:]]*(DIAG|RSPI|RMEM|LCON)-[0-9][0-9][[:space:]]*$/ {
    if ($2 ~ /LCON-04/) next
    id = $2
    evidence = $3
    gsub(/^[[:space:]]+|[[:space:]]+$/, "", id)
    gsub(/^[[:space:]]+|[[:space:]]+$/, "", evidence)
    print id "\t" evidence
  }
' "$repo_root/docs/spec-conformance.md" | sort >"$trace"

cut -f1 "$trace" >"$trace.ids"
if ! diff -u "$expected" "$trace.ids"; then
  echo "DX/rate acceptance ID is not an exact set of 58" >&2
  exit 1
fi

python3 - "$trace" "$repo_root" <<'PY'
import re
import sys
from pathlib import Path

trace_path = Path(sys.argv[1])
repo_root = Path(sys.argv[2]).resolve()
trace = trace_path.read_text(encoding="utf-8")


def validate(candidate: str) -> None:
    rows = [line for line in candidate.splitlines() if line]
    if len(rows) != 58:
        raise ValueError("The trace must contain exactly 58 rows.")
    seen: set[str] = set()
    for line in rows:
        row_id, evidence = line.split("\t", 1)
        if row_id in seen:
            raise ValueError(f"Duplicate acceptance trace ID: {row_id}")
        seen.add(row_id)
        references = re.findall(r"`([^`]+)`", evidence)
        if len(references) != 1:
            raise ValueError("Exactly one proof is required.")
        reference = references[0]
        relative, separator, marker = reference.partition("#")
        if not separator or not marker:
            raise ValueError(f"path # is not the format of marker: {row_id}")
        relative_path = Path(relative)
        if relative_path.is_absolute() or ".." in relative_path.parts:
            raise ValueError(f"The repository is not a relative path: {row_id}")
        target = (repo_root / relative_path).resolve()
        try:
            target.relative_to(repo_root)
        except ValueError as error:
            raise ValueError(f"Evidence path escapes the repository: {row_id}") from error
        if not target.is_file():
            raise ValueError(f"There is no evidence file: {row_id}")
        source = target.read_text(encoding="utf-8")
        if marker not in source:
            raise ValueError(f"The evidence marker is missing: {row_id}")
        if "Planned" in evidence or "Scheduled" in evidence:
            raise ValueError(
                f"Planned evidence must identify an exact implementation owner: {row_id}"
            )


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
    raise SystemExit(f"I couldn't resist the mutation.")


expect_invalid("DIAG-01\t", "DIAG-02\t", "Duplicate ID.")
expect_invalid("crates/cli/tests/rust_diagnostics.rs", "crates/cli/tests/missing.rs", "Missing path.")
expect_invalid("#exposes_public_models_and_human_and_json_renderers", "#missing_public_models", "The missing marker.")
expect_invalid("`crates/cli/tests/rust_diagnostics.rs#", "`extra#marker`, `crates/cli/tests/rust_diagnostics.rs#", "Multiple evidence references.")
PY

echo "DX/rate acceptance trace passed: 58 IDs, exact path/marker coverage, and mutation checks"
