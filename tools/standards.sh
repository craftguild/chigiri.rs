#!/bin/sh
# Dispatch to the immutable shared engine; product gates are configured adapters.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
cd "$root"
if [ -f .craftguild/gpg-failure ]; then
    echo 'OPENPGP_FAILURE: pause the goal before further work' >&2
    exit 71
fi
if [ "$#" -eq 2 ] && { [ "$2" = --help ] || [ "$2" = -h ]; }; then
    exec bash scripts/python-with-toml.sh standards/launcher.py "$@"
fi
case "${1:-}" in
    build|prepare)
        if [ -n "${CHIGIRI_PREPARE_TOOLCHAIN:-}" ]; then
            TOOLCHAIN=$CHIGIRI_PREPARE_TOOLCHAIN
            export TOOLCHAIN
        fi
        ;;
esac
exec bash scripts/python-with-toml.sh standards/launcher.py "$@"
