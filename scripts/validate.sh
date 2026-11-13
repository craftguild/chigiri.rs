#!/usr/bin/env bash
set -euo pipefail

# Run ordinary fixed-toolchain validation, or request the full product profile.
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
exec bash "$repo_root/scripts/python-with-toml.sh" "$repo_root/scripts/validate.py" "$@"
