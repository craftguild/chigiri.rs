#!/usr/bin/env bash

# Separate the performance proof compiler from moving alias or ambient override.
require_performance_toolchain() {
  local version="$1"
  local repository="$2"
  if [[ ! "$version" =~ ^[1-9][0-9]*\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
    echo "Performance comparison requires a fixed x.y.z toolchain version." >&2
    return 2
  fi
  local name
  while IFS= read -r name; do
    case "$name" in
      RUSTC|RUSTDOC|CARGO|RUSTUP_TOOLCHAIN|RUSTFLAGS|RUSTDOCFLAGS|\
      CARGO_HOME|CARGO_CONFIG|CARGO_INCREMENTAL|CARGO_ENCODED_RUSTFLAGS|\
      RUSTC_BOOTSTRAP|RUSTC_WRAPPER|RUSTC_WORKSPACE_WRAPPER|\
      CARGO_BUILD_*|CARGO_PROFILE_*|CARGO_TARGET_*|\
      CC|CXX|CPP|AR|CFLAGS|CXXFLAGS|CPPFLAGS|LDFLAGS|SDKROOT|MACOSX_DEPLOYMENT_TARGET)
        if [[ -n "${!name:-}" ]]; then
          echo "Performance builds reject the ambient compiler override $name." >&2
          return 2
        fi
        ;;
    esac
  done < <(compgen -e)
  require_no_external_cargo_config "$repository" || return $?
  CHIGIRI_PERFORMANCE_RUSTC_PATH="$(rustup which --toolchain "$version" rustc)" || return $?
  CHIGIRI_PERFORMANCE_CARGO_PATH="$(rustup which --toolchain "$version" cargo)" || return $?
  if [[ ! -x "$CHIGIRI_PERFORMANCE_RUSTC_PATH" || ! -x "$CHIGIRI_PERFORMANCE_CARGO_PATH" ]]; then
    echo "Could not resolve Cargo and rustc for the specified toolchain." >&2
    return 2
  fi
}

require_no_external_cargo_config() {
  local repository="$1"
  local candidate
  for candidate in "$HOME/.cargo/config" "$HOME/.cargo/config.toml"; do
    if [[ -e "$candidate" ]]; then
      echo "Cargo config is not allowed for a controlled performance build: $candidate." >&2
      return 2
    fi
  done
  local directory
  directory="$(dirname "$repository")"
  while [[ "$directory" != "." ]]; do
    for candidate in "$directory/.cargo/config" "$directory/.cargo/config.toml"; do
      if [[ -e "$candidate" ]]; then
        echo "Cargo config outside the repository is not allowed for a controlled performance build: $candidate." >&2
        return 2
      fi
    done
    if [[ "$directory" == "/" ]]; then
      break
    fi
    directory="$(dirname "$directory")"
  done
}

performance_cargo() {
  RUSTC="$CHIGIRI_PERFORMANCE_RUSTC_PATH" \
    CARGO_BUILD_RUSTC="$CHIGIRI_PERFORMANCE_RUSTC_PATH" \
    "$CHIGIRI_PERFORMANCE_CARGO_PATH" "$@"
}

# Do not write comparative evidence from a different compiler generation.
require_distinct_performance_files() {
  python3 -c '
import os
import sys

left = os.path.realpath(sys.argv[1])
right = os.path.realpath(sys.argv[2])
same = left == right
if not same and os.path.exists(left) and os.path.exists(right):
    same = os.path.samefile(left, right)
if same:
    raise SystemExit("Same file. The previous release artifact and next baseline output must differ.")
' "$1" "$2"
}
