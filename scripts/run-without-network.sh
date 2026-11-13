#!/bin/sh
set -eu

guard=$1
mode=$2
shift 2

case "$guard:$mode" in
    sandbox-exec:--deny-network)
        exec sandbox-exec -p '(version 1) (allow default) (deny network*)' "$@"
        ;;
    unshare:--user-network)
        exec unshare --user --map-root-user --net "$@"
        ;;
    *)
        echo "unsupported network-isolation guard" >&2
        exit 1
        ;;
esac
