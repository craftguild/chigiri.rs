#!/usr/bin/env bash

# Legacy assertion ID: MSRV/stable-five-command-timeouts.
# The current compiler policy selects fixed Rust 1.95.0.
chigiri_large_consumer_script_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

run_large_consumer_command() {
  local toolchain="$1"
  local label="$2"
  local evidence_prefix="$3"
  local timeout_seconds="$4"
  shift 4
  local log="${evidence_prefix}-${toolchain}-${label}.log"
  local result="${evidence_prefix}-${toolchain}-${label}.json"
  local command_text
  printf -v command_text '%q ' "$@"
  local status=0
  CHIGIRI_COMMAND_RESULT="$result" \
    CHIGIRI_COMMAND_LABEL="$label" \
    CHIGIRI_COMMAND_TOOLCHAIN="$toolchain" \
    python3 "$chigiri_large_consumer_script_root/run-command-with-timeout.py" \
      "$timeout_seconds" "$log" "$@" || status=$?
  {
    echo "label=$label"
    echo "command=${command_text% }"
    if (( status == 124 )); then
      echo "status=timeout"
    else
      echo "status=$status"
    fi
  } >>"$log"
  return "$status"
}

run_large_consumer_suite() {
  local toolchain="$1"
  local manifest="$2"
  local target="$3"
  local evidence_prefix="$4"
  local timeout_seconds="$5"
  local generated_bytes="$6"
  shift 6
  local summary="${evidence_prefix}-${toolchain}.log"
  {
    echo "toolchain=$toolchain"
    echo "rustc=$(rustc "+$toolchain" --version)"
    echo "hardware=$(uname -a)"
    echo "generated_bytes=$generated_bytes"
  } >"$summary"

  local suite_status=0
  run_large_consumer_step "$toolchain" fmt "$manifest" "$target" \
    "$evidence_prefix" "$timeout_seconds" "$summary" \
    cargo "+$toolchain" fmt --manifest-path "$manifest" -- --check || suite_status=1
  run_large_consumer_step "$toolchain" check "$manifest" "$target" \
    "$evidence_prefix" "$timeout_seconds" "$summary" \
    cargo "+$toolchain" check --locked --manifest-path "$manifest" \
    --target-dir "$target" --all-targets --all-features "$@" || suite_status=1
  run_large_consumer_step "$toolchain" test "$manifest" "$target" \
    "$evidence_prefix" "$timeout_seconds" "$summary" \
    cargo "+$toolchain" test --locked --manifest-path "$manifest" \
    --target-dir "$target" --all-targets --all-features "$@" || suite_status=1
  run_large_consumer_step "$toolchain" clippy "$manifest" "$target" \
    "$evidence_prefix" "$timeout_seconds" "$summary" \
    cargo "+$toolchain" clippy --locked --manifest-path "$manifest" \
    --target-dir "$target" --all-targets --all-features "$@" -- -D warnings || suite_status=1
  run_large_consumer_step "$toolchain" rustdoc "$manifest" "$target" \
    "$evidence_prefix" "$timeout_seconds" "$summary" \
    env RUSTDOCFLAGS=-Dwarnings cargo "+$toolchain" doc --locked \
    --manifest-path "$manifest" --target-dir "$target" --no-deps --all-features "$@" || suite_status=1
  return "$suite_status"
}

run_large_consumer_step() {
  local toolchain="$1"
  local label="$2"
  local manifest="$3"
  local target="$4"
  local evidence_prefix="$5"
  local timeout_seconds="$6"
  local summary="$7"
  shift 7
  local started=$SECONDS
  local status=0
  run_large_consumer_command "$toolchain" "$label" "$evidence_prefix" \
    "$timeout_seconds" "$@" || status=$?
  {
    echo "${label}_elapsed_seconds=$((SECONDS - started))"
    echo "${label}_status=$status"
    echo "${label}_manifest=$manifest"
    echo "${label}_target=$target"
  } >>"$summary"
  return "$status"
}
