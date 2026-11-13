#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
# shellcheck source=build-tool-versions.sh
. "$script_dir/build-tool-versions.sh"

if [ "${CHIGIRI_ACCEPT_MICROSOFT_LICENSE:-}" != "yes" ]; then
    echo "This downloads Microsoft SDK/CRT material." >&2
    echo "Review https://go.microsoft.com/fwlink/?LinkId=2086102 first." >&2
    echo "Re-run with CHIGIRI_ACCEPT_MICROSOFT_LICENSE=yes only if you accept it." >&2
    exit 1
fi

xwin_overrides=$(env | sed -n 's/^\(XWIN_[A-Za-z0-9_]*\)=.*/\1/p')
if [ -n "$xwin_overrides" ]; then
    echo "cargo-xwin overrides are not allowed: $(printf '%s' "$xwin_overrides" | tr '\n' ' ')" >&2
    exit 1
fi

cache_dir=${CHIGIRI_XWIN_CACHE_DIR:-"$repo_root/target/tool-cache/cargo-xwin-$CHIGIRI_CARGO_XWIN_VERSION-sdk-$CHIGIRI_WINDOWS_SDK_VERSION-crt-$CHIGIRI_WINDOWS_CRT_VERSION-vs-$CHIGIRI_XWIN_VERSION-$CHIGIRI_XWIN_VARIANT"}
python3 "$script_dir/xwin-cache.py" prepare "$cache_dir" \
    "$CHIGIRI_RUST_TOOLCHAIN" \
    "$CHIGIRI_CARGO_XWIN_VERSION" "$CHIGIRI_XWIN_VERSION" \
    "$CHIGIRI_WINDOWS_SDK_VERSION" "$CHIGIRI_WINDOWS_CRT_VERSION" \
    x86_64 "$CHIGIRI_XWIN_VARIANT"
echo "Prepared pinned Windows SDK cache: $cache_dir"
