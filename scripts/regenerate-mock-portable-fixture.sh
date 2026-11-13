#!/bin/sh
set -eu

CHIGIRI_REGENERATE_MOCK_PORTABLE_FIXTURE=1 \
    cargo +1.95.0 test -p chigiri_mock --lib \
    project::portable_fixture_tests::canonical_portable_fixture_is_producer_owned \
    --locked --offline -- --exact

cargo +1.95.0 test -p chigiri_mock --lib \
    project::portable_fixture_tests::canonical_portable_fixture_is_producer_owned \
    --locked --offline -- --exact
