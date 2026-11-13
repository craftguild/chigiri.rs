#!/usr/bin/env bash

# Prefer a Python interpreter with the standard-library TOML parser.
set -euo pipefail

for interpreter in python3 python3.14 python3.13 python3.12 python3.11; do
  if command -v "${interpreter}" >/dev/null 2>&1 &&
    "${interpreter}" -c 'import tomllib' >/dev/null 2>&1; then
    exec "${interpreter}" -B "$@"
  fi
done

echo "Manifest ownership checks require Python 3.11 or later." >&2
exit 1
