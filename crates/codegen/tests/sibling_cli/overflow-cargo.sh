#!/bin/sh
# The current cargo stream has a fixture that exceeds the 64 MiB limit.
sleep 30 &
descendant=$!
printf '%s\n' "$descendant" > "$CHIGIRI_TEST_OVERFLOW_PID"
cat "$CHIGIRI_TEST_OVERFLOW_INPUT"
wait "$descendant"
