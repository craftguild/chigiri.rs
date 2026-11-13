#!/bin/sh
# Add one rustc warning to the real cargo check and leave the completion record
# to the real tool.
set -eu
if [ "$1" = check ]; then
    printf '%s\n' '{"reason":"compiler-message","message":{"code":{"code":"fixture_warning"},"level":"warning","message":"Check the candidate warning.","spans":[],"children":[]}}'
fi
exec "$CHIGIRI_TEST_REAL_CARGO" "$@"
