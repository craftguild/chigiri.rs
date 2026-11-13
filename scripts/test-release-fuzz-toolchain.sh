#!/usr/bin/env bash

# Test both unavailable and available release fuzz toolchains.

set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
readonly RUNNER_SOURCE="${SCRIPT_DIR}/run-release-fuzz.sh"
readonly TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/chigiri-fuzz-toolchain.XXXXXX")"
readonly CASE_ROOT="${TEST_ROOT}/repository"
readonly FAKE_BIN="${TEST_ROOT}/bin"
readonly OUTPUT_PATH="${TEST_ROOT}/output.txt"
readonly LOG_PATH="${TEST_ROOT}/cargo.log"

cleanup() {
  rm -rf -- "${TEST_ROOT}"
}

trap cleanup EXIT HUP INT TERM

fail() {
  echo "Failed test: $*" >&2
  exit 1
}

mkdir -p "${CASE_ROOT}/scripts" "${FAKE_BIN}"
cp "${RUNNER_SOURCE}" "${CASE_ROOT}/scripts/run-release-fuzz.sh"
chmod +x "${CASE_ROOT}/scripts/run-release-fuzz.sh"

{
  printf '%s\n' '#!/usr/bin/env bash' 'set -euo pipefail'
  printf '%s\n' 'printf "%s\n" "$*" >>"${CHIGIRI_FAKE_CARGO_LOG:?}"'
  printf '%s\n' 'if [[ "$*" == "+nightly --version" ]]; then'
  printf '%s\n' '  exit "${CHIGIRI_FAKE_NIGHTLY_STATUS:?}"' 'fi'
  printf '%s\n' 'if [[ "$*" == "+nightly fuzz --version" ]]; then'
  printf '%s\n' '  exit "${CHIGIRI_FAKE_FUZZ_STATUS:?}"' 'fi' 'exit 99'
} >"${FAKE_BIN}/cargo"
chmod +x "${FAKE_BIN}/cargo"

run_check() {
  local nightly_status="$1"
  local fuzz_status="$2"
  local command_status

  : >"${LOG_PATH}"
  if PATH="${FAKE_BIN}:${PATH}" \
    CHIGIRI_FAKE_CARGO_LOG="${LOG_PATH}" \
    CHIGIRI_FAKE_NIGHTLY_STATUS="${nightly_status}" \
    CHIGIRI_FAKE_FUZZ_STATUS="${fuzz_status}" \
    "${CASE_ROOT}/scripts/run-release-fuzz.sh" --check-toolchain \
    >"${OUTPUT_PATH}" 2>&1; then
    return 0
  else
    command_status=$?
  fi
  return "${command_status}"
}

if run_check 1 0; then
  fail "Missing nightly unexpectedly succeeded."
else
  status=$?
fi
[[ "${status}" == "2" ]] || fail "Missing nightly returned an exit status other than 2."
grep -Fq -- "rustup toolchain install nightly" "${OUTPUT_PATH}" || fail "Missing nightly recovery guidance."

if run_check 0 1; then
  fail "Missing cargo-fuzz unexpectedly succeeded."
else
  status=$?
fi
[[ "${status}" == "2" ]] || fail "Missing cargo-fuzz returned an exit status other than 2."
grep -Fq -- "cargo +nightly install cargo-fuzz --locked" "${OUTPUT_PATH}" || \
  fail "Missing cargo-fuzz installation guidance."
grep -Fq -- "+nightly fuzz --version" "${LOG_PATH}" || fail "cargo-fuzz was not checked with nightly."

run_check 0 0 || fail "Available fuzz toolchain was rejected."
grep -Fq -- "Release fuzz toolchain is available." "${OUTPUT_PATH}" || fail "Missing success confirmation."

echo "Release fuzz toolchain preflight tests passed."
