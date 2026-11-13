#!/usr/bin/env bash
# Retain the native alias; the shared engine resolves and builds its context.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
profile="${1:-release}"
case "$profile" in debug|release) ;; *) echo 'Expected debug or release' >&2; exit 2;; esac
exec sh "$root/tools/standards.sh" build --artifact chigiri-cli --profile "$profile"
