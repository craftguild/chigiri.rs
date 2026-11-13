#!/usr/bin/env bash

# Regression-test the narrow host-boundary source inventory.

set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
readonly TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/chigiri-host-boundary.XXXXXX")"

cleanup() {
  rm -rf -- "${TEST_ROOT}"
}
trap cleanup EXIT HUP INT TERM

mkdir -p "${TEST_ROOT}/scripts" "${TEST_ROOT}/crates/codegen/src" \
  "${TEST_ROOT}/crates/mock/src/tests" "${TEST_ROOT}/crates/host/src"
cp "${SCRIPT_DIR}/check_host_boundary.py" "${TEST_ROOT}/scripts/"

printf '%s\n' 'use rustix::fs;' >"${TEST_ROOT}/crates/host/src/unix.rs"
printf '%s\n' 'use std::os::unix::fs::PermissionsExt;' \
  >"${TEST_ROOT}/crates/mock/src/tests/backend.rs"
printf '%s\n' 'mod backend;' >"${TEST_ROOT}/crates/mock/src/tests/mod.rs"
printf '%s\n' '#![forbid(unsafe_code)]' 'fn product() {}' \
  >"${TEST_ROOT}/crates/codegen/src/lib.rs"
printf '%s\n' '#![forbid(unsafe_code)]' '#[cfg(test)] mod tests;' \
  >"${TEST_ROOT}/crates/mock/src/lib.rs"
mkdir -p "${TEST_ROOT}/crates/cli/src"
printf '%s\n' '#![forbid(unsafe_code)]' >"${TEST_ROOT}/crates/cli/src/lib.rs"
printf '%s\n' '#![forbid(unsafe_code)]' >"${TEST_ROOT}/crates/cli/src/main.rs"
printf '%s\n' '#![forbid(unsafe_code)]' >"${TEST_ROOT}/crates/host/src/lib.rs"
printf '%s\n' '#![forbid(unsafe_code)]' >"${TEST_ROOT}/crates/codegen/build.rs"
python3 "${TEST_ROOT}/scripts/check_host_boundary.py" >/dev/null

printf '%s\n' '#![forbid(unsafe_code)]' 'extern crate rustix as rx;' \
  '#[cfg(target_arch = "x86_64")] fn branch() {}' \
  >"${TEST_ROOT}/crates/codegen/src/lib.rs"
if python3 "${TEST_ROOT}/scripts/check_host_boundary.py" \
  >"${TEST_ROOT}/output" 2>&1; then
  echo "Host boundary checker accepted a direct product import." >&2
  exit 1
fi
grep -Fq 'direct rustix import' "${TEST_ROOT}/output"
grep -Fq 'target-platform selection' "${TEST_ROOT}/output"

printf '%s\n' '#![forbid(unsafe_code)]' 'use {' '  rustix as rx,' \
  '  signal_hook as hooks,' '  windows_sys as win,' '};' \
  'use std::os::{unix::fs::PermissionsExt};' \
  'use std::os as platform;' \
  'use platform::unix::fs::PermissionsExt as PlatformPermissionsExt;' \
  'use std as standard;' \
  'use standard::os::unix::ffi::OsStrExt;' \
  'use std::{self as standard_braced};' \
  'use standard_braced::os::unix::ffi::OsStringExt;' \
  'use {std as standard_grouped};' \
  'use standard_grouped::os::unix::raw::uid_t;' \
  'use tokio::signal::{unix};' \
  'use tokio::signal as signals;' \
  'use signals::unix::signal;' \
  'use tokio as runtime;' \
  'use runtime::signal::windows::ctrl_c;' \
  'use tokio::{self as runtime_braced};' \
  'use runtime_braced::signal::windows::ctrl_break;' \
  'use {tokio as runtime_grouped};' \
  'use runtime_grouped::signal::unix::Signal;' \
  'fn target_signal() { let _ = tokio::signal::windows::ctrl_c(); }' \
  '#[cfg_attr(' '  all(any(feature = "x"), target_os = "windows"),' \
  '  path = "windows.rs"' \
  ')] mod backend;' \
  'fn nested() -> bool { cfg!(all(any(feature = "x"), target_os = "linux")) }' \
  >"${TEST_ROOT}/crates/codegen/src/lib.rs"
if python3 "${TEST_ROOT}/scripts/check_host_boundary.py" \
  >"${TEST_ROOT}/output" 2>&1; then
  echo "Host boundary checker accepted alias and cfg_attr bypasses." >&2
  exit 1
fi
grep -Fq 'direct rustix import' "${TEST_ROOT}/output"
grep -Fq 'direct signal-hook import' "${TEST_ROOT}/output"
grep -Fq 'direct Windows binding import' "${TEST_ROOT}/output"
grep -Fq 'direct target-specific Tokio signal import' "${TEST_ROOT}/output"
grep -Fq 'target-platform selection' "${TEST_ROOT}/output"

printf '%s\n' '#![forbid(unsafe_code)]' '#[cfg(test)] mod shared;' \
  '#[path = "shared.rs"] mod production_alias;' \
  >"${TEST_ROOT}/crates/codegen/src/lib.rs"
printf '%s\n' 'use rustix::fs;' >"${TEST_ROOT}/crates/codegen/src/shared.rs"
if python3 "${TEST_ROOT}/scripts/check_host_boundary.py" \
  >"${TEST_ROOT}/output" 2>&1; then
  echo "Host boundary checker trusted one of multiple module declarations." >&2
  exit 1
fi
grep -Fq 'crates/codegen/src/shared.rs:1: direct rustix import' \
  "${TEST_ROOT}/output"
rm "${TEST_ROOT}/crates/codegen/src/shared.rs"

printf '%s\n' 'const MARKER: &str = r#"' '#![forbid(unsafe_code)]' '"#;' \
  '/* #![forbid(unsafe_code)] */' 'unsafe fn product() {}' \
  >"${TEST_ROOT}/crates/codegen/src/lib.rs"
if python3 "${TEST_ROOT}/scripts/check_host_boundary.py" \
  >"${TEST_ROOT}/output" 2>&1; then
  echo "Host boundary checker accepted a crate without forbid(unsafe_code)." >&2
  exit 1
fi
grep -Fq 'missing crate-wide unsafe prohibition' "${TEST_ROOT}/output"

printf '%s\n' '#![forbid(unsafe_code)]' 'fn product() {}' \
  >"${TEST_ROOT}/crates/codegen/src/lib.rs"
printf '%s\n' 'fn main() {}' >"${TEST_ROOT}/crates/cli/src/main.rs"
if python3 "${TEST_ROOT}/scripts/check_host_boundary.py" \
  >"${TEST_ROOT}/output" 2>&1; then
  echo "Host boundary checker accepted an unsafe-capable binary root." >&2
  exit 1
fi
grep -Fq 'crates/cli/src/main.rs:1: missing crate-wide unsafe prohibition' \
  "${TEST_ROOT}/output"
printf '%s\n' '#![forbid(unsafe_code)]' >"${TEST_ROOT}/crates/cli/src/main.rs"

mkdir -p "${TEST_ROOT}/crates/cli/src/bin"
printf '%s\n' 'use std::os::unix::ffi::OsStrExt;' \
  >"${TEST_ROOT}/crates/cli/src/bin/escape.rs"
printf '%s\n' 'use std::os::windows::ffi::OsStrExt;' \
  >"${TEST_ROOT}/crates/mock/build.rs"
if python3 "${TEST_ROOT}/scripts/check_host_boundary.py" \
  >"${TEST_ROOT}/output" 2>&1; then
  echo "Host boundary checker skipped auto-discovered Cargo roots." >&2
  exit 1
fi
grep -Fq 'crates/cli/src/bin/escape.rs:1: direct standard-library OS import' \
  "${TEST_ROOT}/output"
grep -Fq 'crates/cli/src/bin/escape.rs:1: missing crate-wide unsafe prohibition' \
  "${TEST_ROOT}/output"
grep -Fq 'crates/mock/build.rs:1: direct standard-library OS import' \
  "${TEST_ROOT}/output"
grep -Fq 'crates/mock/build.rs:1: missing crate-wide unsafe prohibition' \
  "${TEST_ROOT}/output"
rm -r "${TEST_ROOT}/crates/cli/src/bin" "${TEST_ROOT}/crates/mock/build.rs"

printf '%s\n' 'use std::path::PathBuf;' 'type NativeAuthority = PathBuf;' \
  'impl ::std::convert::AsRef<::std::path::Path> for crate::filesystem::Directory {' \
  '  fn as_ref(&self) -> &std::path::Path { todo!() }' '}' \
  'impl Directory {' \
  '  pub fn authority(&self) -> Result<NativeAuthority, Error> { todo!() }' '}' \
  'impl ResolvedPath {' \
  '  pub async fn generic<T>(&self) -> Option<std::path::PathBuf> { todo!() }' \
  '  pub const fn physical(&self) -> &PathBuf { todo!() }' \
  '  pub extern "C" fn ffi_path(&self) -> PathBuf { todo!() }' '}' \
  >"${TEST_ROOT}/crates/host/src/escape.rs"
if python3 "${TEST_ROOT}/scripts/check_host_boundary.py" \
  >"${TEST_ROOT}/output" 2>&1; then
  echo "Host boundary checker accepted public native path extraction." >&2
  exit 1
fi
grep -Fq 'public native path extraction' "${TEST_ROOT}/output"
rm "${TEST_ROOT}/crates/host/src/escape.rs"

printf '%s\n' 'use rustix::fs;' >"${TEST_ROOT}/crates/codegen/src/escape_tests.rs"
printf '%s\n' '#![forbid(unsafe_code)]' 'fn product() {}' \
  >"${TEST_ROOT}/crates/codegen/src/lib.rs"
if python3 "${TEST_ROOT}/scripts/check_host_boundary.py" \
  >"${TEST_ROOT}/output" 2>&1; then
  echo "Host boundary checker trusted an ungated test-like filename." >&2
  exit 1
fi
grep -Fq 'direct rustix import' "${TEST_ROOT}/output"

echo "Host boundary checker regression tests passed."
