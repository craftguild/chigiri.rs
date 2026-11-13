#!/usr/bin/env sh
# Shared compilation followed by retained product distribution packaging.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
exec bash "$root/scripts/python-with-toml.sh" "$root/scripts/package-distribution.py" "$@"
