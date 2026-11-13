#!/usr/bin/env bash

chigiri_large_consumer_evidence_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

create_large_consumer_evidence_stage() {
  local evidence_root="$1"
  mkdir -p "$evidence_root"
  mktemp -d "$evidence_root/.large-consumer-evidence.XXXXXX"
}

retain_large_consumer_failure() {
  local work_evidence="$1"
  local evidence_root="$2"
  local failure_root="$evidence_root/large-consumer-failures"
  local retained
  mkdir -p "$failure_root" || return 1
  retained="$(mktemp -d "$failure_root/run.XXXXXX")" || return 1
  cp -R -- "$work_evidence/." "$retained/" || return 1
  echo "$retained"
}

validate_large_consumer_evidence_stage() {
  local stage="$1"
  local revision="$2"
  local file_count
  file_count="$(find "$stage" -mindepth 1 -maxdepth 1 -type f | wc -l | tr -d ' ')"
  [[ "$file_count" == "1" && -f "$stage/evidence.json" ]] || return 1
  [[ -z "$(find "$stage" -mindepth 1 -maxdepth 1 ! -type f -print -quit)" ]] || return 1
  python3 "$chigiri_large_consumer_evidence_root/large-consumer-evidence.py" \
    validate "$stage/evidence.json" || return 1
  [[ "$(jq -r '.source.revision' "$stage/evidence.json")" == "$revision" ]] || return 1
}

publish_large_consumer_evidence() {
  local stage="$1"
  local evidence_root="$2"
  local run_id="$3"
  local runs_root="$evidence_root/large-consumer-runs"
  local destination="$runs_root/$run_id"
  local current="$evidence_root/large-consumer-current"
  local next="$evidence_root/.large-consumer-current.$run_id"
  mkdir -p "$runs_root"
  if [[ -e "$destination" ]]; then
    echo "Large-consumer evidence run ID is duplicated: $run_id." >&2
    return 1
  fi
  mv "$stage" "$destination"
  ln -s "large-consumer-runs/$run_id" "$next"
  if ! python3 - "$next" "$current" <<'PY'
import os
import sys

os.replace(sys.argv[1], sys.argv[2])
PY
  then
    rm -f -- "$next"
    return 1
  fi
  echo "$current"
}
