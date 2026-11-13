#!/usr/bin/env bash
# Preserve product ownership/language/host gates and use shared Code measurement.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
case "$#:${1:-}" in
  0:|1:--product-only|1:--boundaries-only) ;;
  *) echo "usage: $0 [--product-only|--boundaries-only]" >&2; exit 2 ;;
esac
if [[ "${MAX_IMPLEMENTATION_FILE_LINES+set}" == set ]]; then
  echo "MAX_IMPLEMENTATION_FILE_LINES is retired; use the pinned Code contract." >&2
  exit 2
fi
bash "$root/scripts/python-with-toml.sh" "$root/scripts/implementation_rules.py"
python3 "$root/scripts/check_repository_language.py"
python3 "$root/scripts/check_host_boundary.py"
# Recheck boundaries after the catalog without repeating product conversion.
if [[ "${1:-}" == "--boundaries-only" ]]; then
  exit 0
fi
# Shared verification cannot discover product groups or bind their revisions.
bash "$root/scripts/python-with-toml.sh" "$root/scripts/refresh-standards-ownership.py" --check
if [[ "${1:-}" == "--product-only" ]]; then
  exit 0
fi
mkdir -p "$root/.craftguild"
sh "$root/tools/standards.sh" assess > "$root/.craftguild/implementation-size.json"
echo "Shared implementation Code assessment passed; .craftguild/implementation-size.json"
