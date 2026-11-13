#!/usr/bin/env bash
set -euo pipefail

# The receiver trace: closed-evidence-and-safe-publication-mutations.
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$repo_root/scripts/large-consumer-suite.sh"
source "$repo_root/scripts/large-consumer-evidence.sh"
temporary_root="$(mktemp -d "${TMPDIR:-/tmp}/chigiri-large-gates.XXXXXX")"
trap 'rm -rf -- "$temporary_root"' EXIT HUP INT TERM

python3 "$repo_root/scripts/test-large-consumer-diagnostics.py"
python3 "$repo_root/scripts/test-large-consumer-rustdoc.py"

raw="$temporary_root/raw"
retention="$temporary_root/retention"
mkdir -p "$raw" "$retention"
printf '%s\n' 'original rustc failure' >"$raw/doctor.log"
printf '%s\n' '{"exitCode":1}' >"$raw/doctor.json"
ln -s previous-success "$retention/large-consumer-current"
first="$(retain_large_consumer_failure "$raw" "$retention")"
second="$(retain_large_consumer_failure "$raw" "$retention")"
[[ "$first" != "$second" ]]
for retained in "$first" "$second"; do
  cmp "$raw/doctor.log" "$retained/doctor.log"
  cmp "$raw/doctor.json" "$retained/doctor.json"
  [[ ! -e "$retained/evidence.json" ]]
done
[[ "$(readlink "$retention/large-consumer-current")" == previous-success ]]

python3 - "$repo_root/scripts/check-large-consumer-conformance.sh" <<'PY'
import sys
from pathlib import Path

source = Path(sys.argv[1]).read_text(encoding="utf-8")
revision = 'revision="$(git -C "$repo_root" rev-parse HEAD)"'
status = 'if [[ -n "$(git -C "$repo_root" status --porcelain)" ]]'
temporary = 'temporary_root="$(mktemp -d '
if source.count(revision) != 1 or source.count(status) != 1:
    raise SystemExit("You only need one source provenance capture.")
if not source.index(revision) < source.index(status) < source.index(temporary):
    raise SystemExit("The temporary fixture must preserve its original source provenance.")
PY

write_fixture() {
  local mode="$1"
  local root="$temporary_root/$mode"
  mkdir -p "$root/src"
  printf '%s\n' \
    '[package]' \
    "name = \"large_gate_$mode\"" \
    'version = "0.0.0"' \
    'edition = "2024"' >"$root/Cargo.toml"
  case "$mode" in
    fmt) printf '%s\n' 'pub fn format_failed( )->bool{true}' >"$root/src/lib.rs" ;;
    check) printf '%s\n' 'pub fn check_fail() { let _ = missing_name; }' >"$root/src/lib.rs" ;;
    test) printf '%s\n' '#[test]' 'fn it_detects_failure() { assert_eq!(2, 3); }' >"$root/src/lib.rs" ;;
    clippy) printf '%s\n' 'pub fn clippy_you_failed() -> bool { if true { true } else { false } }' >"$root/src/lib.rs" ;;
    rustdoc) printf '%s\n' '/// Reference to missing [`MissingItem`].' 'pub struct RustdocFailure;' >"$root/src/lib.rs" ;;
  esac
  cargo +1.95.0 generate-lockfile --offline --manifest-path "$root/Cargo.toml" >/dev/null
}

expect_failure_and_full_collection() {
  local mode="$1"
  local evidence="$temporary_root/evidence/$mode"
  mkdir -p "$temporary_root/evidence"
  if run_large_consumer_suite 1.95.0 "$temporary_root/$mode/Cargo.toml" \
    "$temporary_root/$mode/target" "$evidence" 30 0; then
    echo "$mode mutation was not rejected." >&2
    exit 1
  fi
  local step
  for step in fmt check test clippy rustdoc; do
    test -f "${evidence}-1.95.0-${step}.json"
    test -f "${evidence}-1.95.0-${step}.log"
  done
  if [[ "$(jq -r '.exitCode' "${evidence}-1.95.0-${mode}.json")" == 0 ]]; then
    echo "$mode command missed the mutation." >&2
    exit 1
  fi
}

for mode in fmt check test clippy rustdoc; do
  write_fixture "$mode"
  expect_failure_and_full_collection "$mode"
done

timeout_pid="$temporary_root/timeout.pid"
timeout_prefix="$temporary_root/evidence/timeout"
timeout_status=0
run_large_consumer_command 1.95.0 timeout "$timeout_prefix" 1 \
  /bin/sh -c 'trap "" TERM; echo $$ >"$1"; while :; do sleep 30; done' \
  _ "$timeout_pid" || timeout_status=$?
if (( timeout_status != 124 )); then
  echo "Timeout command was not rejected." >&2
  exit 1
fi
child_pid="$(cat "$timeout_pid")"
for _ in $(seq 1 10); do
  kill -0 "$child_pid" 2>/dev/null || break
  sleep 0.1
done
if kill -0 "$child_pid" 2>/dev/null; then
  kill "$child_pid" 2>/dev/null || true
  echo "Child process remained after timeout." >&2
  exit 1
fi

write_evidence() {
  local stage="$1"
  local revision="$2"
  python3 - "$stage/evidence.json" "$revision" <<'PY'
import json
import sys

output, revision = sys.argv[1:]
commands = []
def command(label, toolchain="1.95.0", exit_code=0):
    commands.append({
        "argv": ["cargo", label], "elapsedSeconds": 0.01,
        "exitCode": exit_code, "label": label,
        "timedOut": False, "toolchain": toolchain,
    })
for label in (
    "codegen", "drift", "sdk-fmt",
    "provider-negative", "provider-official", "provider-third-party",
):
    command(label)
for toolchain in ("1.95.0",):
    command("doctor-positive", toolchain)
    for case in (
        "missing-group", "missing-field", "wrong-output",
        "missing-root", "generated-drift", "abi-mismatch",
    ):
        command(f"doctor-negative-{case}", toolchain, 1)
    for step in ("fmt", "check", "test", "clippy", "rustdoc"):
        command(step, toolchain)
diagnostics = []
for toolchain in ("1.95.0",):
    for case, rustc, code in (
        ("missing-group", "E0599", "SF4018"),
        ("missing-field", "E0046", "SF4018"),
        ("wrong-output", "E0271", "SF4019"),
        ("missing-root", "E0046", "SF4018"),
        ("generated-drift", None, "SF4016"),
        ("abi-mismatch", None, "SF4016"),
    ):
        diagnostics.append({
            "case": case, "exitCode": 1, "chigiriCode": code,
            "rustcCode": rustc, "toolchain": toolchain,
        })
document = {
    "acceptanceIds": [f"LCON-{number:02}" for number in range(1, 17) if number != 4],
    "commands": commands,
    "diagnostics": {
        "negative": diagnostics,
        "positiveExitCodes": {"1.95.0": 0},
    },
    "formatVersion": 5,
    "metrics": {
        "applicationMaxLines": 100, "generatedBytes": 42,
        "generatedFiles": 1, "generatedMaxLines": 500, "handwrittenMaxLines": 400,
        "handwrittenFiles": 1, "generatedExcludedFiles": 1,
    },
    "providers": {"negativeSuite": True, "officialSuite": True, "thirdPartyMinimal": True},
    "scope": {"formalPerformanceGate": "Not Passed", "externalProductDependency": False},
    "shape": {"customScalars": 5, "enums": 65, "fields": 750, "inputObjects": 19, "objects": 70},
    "source": {"dirty": False, "revision": revision},
}
with open(output, "w", encoding="utf-8") as target:
    json.dump(document, target, ensure_ascii=False, sort_keys=True)
    target.write("\n")
PY
  validate_large_consumer_evidence_stage "$stage" "$revision"
}

revision_a="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
revision_b="bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
publication_root="$temporary_root/publication"
stage_a="$(create_large_consumer_evidence_stage "$publication_root")"
stage_b="$(create_large_consumer_evidence_stage "$publication_root")"
write_evidence "$stage_a" "$revision_a"
write_evidence "$stage_b" "$revision_b"
run_a="${stage_a##*/}"
run_b="${stage_b##*/}"
publish_large_consumer_evidence "$stage_a" "$publication_root" "$run_a" >/dev/null
publish_large_consumer_evidence "$stage_b" "$publication_root" "$run_b" >/dev/null
[[ "$(readlink "$publication_root/large-consumer-current")" == "large-consumer-runs/$run_b" ]]
validate_large_consumer_evidence_stage \
  "$publication_root/large-consumer-runs/$run_a" "$revision_a"
validate_large_consumer_evidence_stage \
  "$publication_root/large-consumer-runs/$run_b" "$revision_b"

mutation_stage="$(create_large_consumer_evidence_stage "$publication_root")"
write_evidence "$mutation_stage" "$revision_a"
jq '.commands[0].elapsedSeconds = 181' "$mutation_stage/evidence.json" \
  >"$temporary_root/mutated.json"
mv "$temporary_root/mutated.json" "$mutation_stage/evidence.json"
if validate_large_consumer_evidence_stage "$mutation_stage" "$revision_a" 2>/dev/null; then
  echo "Elapsed-time evidence mutation was not rejected." >&2
  exit 1
fi

# Generated 500 lines of code, handwriting/possession number/byte/file upper limit will continue to be denied.
for expression in '.metrics.handwrittenMaxLines = 401' '.metrics.generatedFiles = 65537' \
  '.metrics.generatedBytes = 67108865' '.metrics.generatedExcludedFiles = 0' '.formatVersion = 1'; do
  write_evidence "$mutation_stage" "$revision_a"
  jq "$expression" "$mutation_stage/evidence.json" >"$temporary_root/mutated.json"
  mv "$temporary_root/mutated.json" "$mutation_stage/evidence.json"
  if validate_large_consumer_evidence_stage "$mutation_stage" "$revision_a" 2>/dev/null; then
    echo "Layout evidence mutation was not rejected: $expression." >&2
    exit 1
  fi
done

extra_stage="$(create_large_consumer_evidence_stage "$publication_root")"
write_evidence "$extra_stage" "$revision_a"
jq '.unexpected = true' "$extra_stage/evidence.json" >"$temporary_root/extra.json"
mv "$temporary_root/extra.json" "$extra_stage/evidence.json"
if validate_large_consumer_evidence_stage "$extra_stage" "$revision_a" 2>/dev/null; then
  echo "Closed evidence-key mutation was not rejected." >&2
  exit 1
fi

python3 - "$repo_root/scripts/check-large-consumer-conformance.sh" \
  "$temporary_root/finalization" <<'PY'
import os
import subprocess
import sys
from pathlib import Path

source_path = Path(sys.argv[1])
root = Path(sys.argv[2])
source = source_path.read_text(encoding="utf-8")
marker = "if (( overall_status != 0 )); then\n"
tail = source[source.index(marker):]
required = (
    'if ! bash "$repo_root/scripts/python-with-toml.sh" "$repo_root/scripts/large-consumer-evidence.py" create',
    'if ! validate_large_consumer_evidence_stage "$evidence_stage" "$revision"; then',
    'if ! published="$(publish_large_consumer_evidence',
)
if any(item not in tail for item in required):
    raise SystemExit("The evidence final is missing the fail-closed distribution.")

root.mkdir(parents=True)
harness = root / "entrypoint-tail.sh"
prefix = r'''#!/usr/bin/env bash
set -u
overall_status=0
repo_root="$CHIGIRI_TEST_ROOT"
work_evidence="$repo_root/work"
evidence_stage="$repo_root/stage"
evidence_root="$repo_root/publication"
generated_dir="$repo_root/generated"
temporary_root="$repo_root/temporary"
shape_json="$repo_root/shape.json"
doctor_fixed=0
revision=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
dirty=false
mkdir -p "$work_evidence" "$evidence_stage" "$evidence_root/large-consumer-runs"
printf '%s\n' '{}' >"$work_evidence/command.json"
printf '%s\n' '{}' >"$repo_root/diagnostic.json"
diagnostic_jsons=("$repo_root/diagnostic.json")
record_failure() { echo "$1" >&2; overall_status=1; }
bash() {
  [[ "$1" == "$repo_root/scripts/python-with-toml.sh" ]] || return 93
  [[ "$2" == "$repo_root/scripts/large-consumer-evidence.py" && "$3" == create ]] || return 94
  printf '%s\n' create >>"$CHIGIRI_TEST_TRACE"
  [[ "$CHIGIRI_FAIL_PHASE" != create ]]
}
validate_large_consumer_evidence_stage() {
  printf '%s\n' validate >>"$CHIGIRI_TEST_TRACE"
  [[ "$CHIGIRI_FAIL_PHASE" != validate ]]
}
publish_large_consumer_evidence() {
  printf '%s\n' publish >>"$CHIGIRI_TEST_TRACE"
  [[ "$CHIGIRI_FAIL_PHASE" != publish ]] || return 1
  printf '%s\n' "$evidence_root/large-consumer-runs/$3"
}
'''
harness.write_text(prefix + tail, encoding="utf-8")

expected_trace = {
    "create": ["create"],
    "validate": ["create", "validate"],
    "publish": ["create", "validate", "publish"],
}
for phase, expected in expected_trace.items():
    phase_root = root / phase
    phase_root.mkdir()
    current = phase_root / "publication" / "large-consumer-current"
    current.parent.mkdir(parents=True)
    current.symlink_to("sentinel")
    trace = phase_root / "trace"
    env = os.environ.copy()
    env.update({
        "CHIGIRI_FAIL_PHASE": phase,
        "CHIGIRI_TEST_ROOT": str(phase_root),
        "CHIGIRI_TEST_TRACE": str(trace),
    })
    result = subprocess.run(["bash", str(harness)], env=env, check=False)
    if result.returncode == 0:
        raise SystemExit("The expected failure command succeeded.")
    if trace.read_text(encoding="utf-8").splitlines() != expected:
        raise SystemExit("The failed update changed the trace unexpectedly.")
    if not current.is_symlink() or os.readlink(current) != "sentinel":
        raise SystemExit("A failed update modified the current symlink.")
PY

echo "Large-consumer conformance gate mutation checks passed."
