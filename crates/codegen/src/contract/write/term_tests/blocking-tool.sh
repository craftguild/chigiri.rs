#!/bin/sh
# Let the real tool handle preflight and preparation. Block only the selected
# tool invocation after the transaction has started.
set -eu
block=false
if [ "$CHIGIRI_TEST_TERM_TOOL" = cargo ] && [ "$1" = check ]; then
    block=true
fi
if [ "$CHIGIRI_TEST_TERM_TOOL" = rustup ] && [ -f "$CHIGIRI_TEST_TERM_STAGE/state.json" ]; then
    block=true
fi
if [ "$block" = true ]; then
    sleep 60 &
    descendant=$!
    printf '%s %s %s' "$PPID" "$$" "$descendant" > "$CHIGIRI_TEST_TERM_READY"
    exec 1>&- 2>&-
    wait
else
    exec "$CHIGIRI_TEST_TERM_REAL_TOOL" "$@"
fi
