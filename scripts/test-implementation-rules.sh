#!/usr/bin/env bash
# Retain product ownership and repository-language regressions.
set -euo pipefail
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
bash "$script_dir/python-with-toml.sh" "$script_dir/test-generated-ownership.py"
python3 "$script_dir/test_repository_language.py"
