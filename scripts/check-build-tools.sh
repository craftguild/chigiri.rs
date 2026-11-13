#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
# shellcheck source=build-tool-versions.sh
. "$script_dir/build-tool-versions.sh"

scope=${1:-all}
host_os=$(uname -s)
host_arch=$(uname -m)

fail() {
    echo "build preflight: $*" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

require_python() {
    require_command python3
    python3 -c 'import sys; raise SystemExit(sys.version_info < (3, 9))' \
        || fail "Python 3.9 or newer is required"
}

require_supported_host() {
    case "$host_os:$host_arch" in
        Darwin:arm64|Darwin:aarch64|Darwin:x86_64|Linux:x86_64) ;;
        *) fail "unsupported build host: $host_os $host_arch" ;;
    esac
}

require_rust_target() {
    target=$1
    require_command cargo
    require_command rustc
    require_command rustup
    rust_version=$(rustc "+$CHIGIRI_RUST_TOOLCHAIN" --version 2>/dev/null || true)
    case "$rust_version" in
        "rustc $CHIGIRI_RUST_TOOLCHAIN "*) ;;
        *) fail "Rust $CHIGIRI_RUST_TOOLCHAIN is not installed" ;;
    esac
    rustup target list --installed --toolchain "$CHIGIRI_RUST_TOOLCHAIN" \
        | grep -Fx "$target" >/dev/null \
        || fail "Rust $CHIGIRI_RUST_TOOLCHAIN target is not installed: $target"
}

reject_native_overrides() {
    native_overrides=$(env | awk -F= '
        $1 == "CARGO_BUILD_TARGET" || $1 == "CARGO_TARGET_DIR" ||
        $1 == "RUSTFLAGS" || $1 == "CARGO_ENCODED_RUSTFLAGS" ||
        $1 == "RUSTDOCFLAGS" || $1 == "CARGO_ENCODED_RUSTDOCFLAGS" ||
        $1 == "CARGO_BUILD_RUSTFLAGS" || $1 == "CARGO_BUILD_RUSTDOCFLAGS" ||
        $1 ~ /^CARGO_TARGET_[A-Za-z0-9_]+$/ { print $1 }
    ' | LC_ALL=C sort)
    [ -z "$native_overrides" ] \
        || fail "native build overrides are not allowed: $(printf '%s' "$native_overrides" | tr '\n' ' ')"
}

reject_native_cargo_target() {
    key=build.target
    if output=$(CDPATH= cd -- "$repo_root" && \
        RUSTC_BOOTSTRAP=1 cargo "+$CHIGIRI_RUST_TOOLCHAIN" \
            -Z unstable-options config get "$key" --format=json 2>&1); then
        fail "native Cargo config override is not allowed: $key"
    fi
    case "$output" in
        *"config value \`$key\` is not set"*) ;;
        *) fail "native Cargo config inspection failed for $key" ;;
    esac
}

check_native() {
    require_supported_host
    reject_native_overrides
    reject_native_cargo_target
    case "$host_os:$host_arch" in
        Darwin:arm64|Darwin:aarch64) target=aarch64-apple-darwin ;;
        Darwin:x86_64) target=x86_64-apple-darwin ;;
        Linux:x86_64) target=x86_64-unknown-linux-gnu ;;
    esac
    require_rust_target "$target"
    echo "build preflight: native $host_os $host_arch; Rust $CHIGIRI_RUST_TOOLCHAIN"
}

check_macos() {
    target=$1
    require_supported_host
    [ "$host_os" = Darwin ] || fail "$target builds require a macOS host"
    apple_overrides=$(env | awk -F= '
        $1 == "DEVELOPER_DIR" || $1 == "SDKROOT" || $1 == "TOOLCHAINS" ||
        $1 == "MACOSX_DEPLOYMENT_TARGET" || $1 == "RUSTFLAGS" ||
        $1 == "CARGO_ENCODED_RUSTFLAGS" ||
        $1 == "CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER" ||
        $1 == "CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER" ||
        $1 == "CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS" ||
        $1 == "CARGO_TARGET_X86_64_APPLE_DARWIN_RUSTFLAGS" { print $1 }
    ')
    [ -z "$apple_overrides" ] \
        || fail "Apple build overrides are not allowed: $(printf '%s' "$apple_overrides" | tr '\n' ' ')"
    require_python
    require_rust_target "$target"
    require_command xcrun
    sdk_path=$(xcrun --sdk macosx --show-sdk-path 2>/dev/null || true)
    [ -n "$sdk_path" ] && [ -d "$sdk_path" ] \
        || fail "the selected macOS SDK is unavailable"
    clang_path=$(xcrun --sdk macosx --find clang 2>/dev/null || true)
    [ -n "$clang_path" ] && [ -x "$clang_path" ] \
        || fail "the selected macOS SDK has no usable clang"
    linker_path=$(xcrun --sdk macosx --find ld 2>/dev/null || true)
    [ -n "$linker_path" ] && [ -x "$linker_path" ] \
        || fail "the selected macOS SDK has no usable linker"
    sdk_version=$(xcrun --sdk macosx --show-sdk-version)
    echo "build preflight: $target; SDK $sdk_version at $sdk_path; clang $clang_path; linker $linker_path"
}

check_linux() {
    require_supported_host
    require_rust_target x86_64-unknown-linux-gnu
    require_command cross
    require_command docker
    require_python
    cross_overrides=$(env | sed -n 's/^\(CROSS_[A-Za-z0-9_]*\)=.*/\1/p')
    [ -z "$cross_overrides" ] \
        || fail "Cross overrides are not allowed: $(printf '%s' "$cross_overrides" | tr '\n' ' ')"
    [ -z "${DOCKER_HOST:-}" ] && [ -z "${DOCKER_CONTEXT:-}" ] \
        || fail "Docker endpoint overrides are not allowed"
    if cross_banner=$(CDPATH= cd / && cross --version 2>/dev/null); then
        cross_version=$(printf '%s\n' "$cross_banner" | sed -n '1p')
    else
        cross_version=none
    fi
    [ "$cross_version" = "cross $CHIGIRI_CROSS_VERSION" ] \
        || fail "cross $CHIGIRI_CROSS_VERSION is required; found: ${cross_version:-none}"
    image_line="image = \"$CHIGIRI_LINUX_IMAGE\""
    [ "$(grep -Fxc "$image_line" "$repo_root/Cross.toml")" -eq 1 ] \
        || fail "Cross.toml does not contain the exact pinned Linux image"
    docker info >/dev/null 2>&1 || fail "Docker daemon is unavailable"
    docker_context=$(docker context show 2>/dev/null || true)
    docker_endpoint=$(docker context inspect "$docker_context" \
        --format '{{.Endpoints.docker.Host}}' 2>/dev/null || true)
    case "$docker_endpoint" in
        unix://*) ;;
        *) fail "Docker must use a local Unix endpoint" ;;
    esac
    docker image inspect "$CHIGIRI_LINUX_IMAGE" >/dev/null 2>&1 \
        || fail "pinned Linux image is not cached: $CHIGIRI_LINUX_IMAGE"
    docker run --rm --pull=never "$CHIGIRI_LINUX_IMAGE" /bin/true >/dev/null 2>&1 \
        || fail "pinned Linux image cannot execute on this host"
    image_id=$(docker image inspect --format '{{.Id}}' "$CHIGIRI_LINUX_IMAGE")
    echo "build preflight: Linux image $CHIGIRI_LINUX_IMAGE ($image_id)"
}

xwin_cache_dir() {
    if [ -n "${CHIGIRI_XWIN_CACHE_DIR:-}" ]; then
        printf '%s\n' "$CHIGIRI_XWIN_CACHE_DIR"
    else
        printf '%s\n' "$repo_root/target/tool-cache/cargo-xwin-$CHIGIRI_CARGO_XWIN_VERSION-sdk-$CHIGIRI_WINDOWS_SDK_VERSION-crt-$CHIGIRI_WINDOWS_CRT_VERSION-vs-$CHIGIRI_XWIN_VERSION-$CHIGIRI_XWIN_VARIANT"
    fi
}

reject_xwin_overrides() {
    xwin_overrides=$(env | sed -n 's/^\(XWIN_[A-Za-z0-9_]*\)=.*/\1/p')
    [ -z "$xwin_overrides" ] \
        || fail "cargo-xwin overrides are not allowed: $(printf '%s' "$xwin_overrides" | tr '\n' ' ')"
}

check_windows() {
    require_supported_host
    reject_xwin_overrides
    require_rust_target x86_64-pc-windows-msvc
    require_python
    cargo_xwin_version=$(cargo "+$CHIGIRI_RUST_TOOLCHAIN" xwin --version 2>/dev/null) \
        || fail "cargo-xwin $CHIGIRI_CARGO_XWIN_VERSION version check failed"
    # The pinned release includes the xwin subcommand in its version banner.
    [ "$cargo_xwin_version" = "cargo-xwin-xwin $CHIGIRI_CARGO_XWIN_VERSION" ] \
        || fail "cargo-xwin $CHIGIRI_CARGO_XWIN_VERSION is required; found: ${cargo_xwin_version:-none}"
    rustup component list --installed --toolchain "$CHIGIRI_RUST_TOOLCHAIN" \
        | grep -E '^llvm-tools(-|$)' >/dev/null \
        || fail "Rust $CHIGIRI_RUST_TOOLCHAIN llvm-tools component is not installed"
    require_command clang
    require_command llvm-lib
    require_command lld-link
    clang --version >/dev/null 2>&1 || fail "clang is not usable"
    llvm-lib --version >/dev/null 2>&1 || fail "llvm-lib is not usable"
    lld-link --version >/dev/null 2>&1 || fail "lld-link is not usable"
    case "$host_os" in
        Darwin)
            require_command sandbox-exec
            "$script_dir/run-without-network.sh" sandbox-exec --deny-network /usr/bin/true \
                || fail "macOS network isolation is unavailable"
            ;;
        Linux)
            require_command unshare
            "$script_dir/run-without-network.sh" unshare --user-network /bin/true \
                || fail "Linux user/network namespace isolation is unavailable"
            ;;
    esac
    cache_dir=$(xwin_cache_dir)
    python3 "$script_dir/xwin-cache.py" verify "$cache_dir" \
        "$CHIGIRI_CARGO_XWIN_VERSION" "$CHIGIRI_XWIN_VERSION" \
        "$CHIGIRI_WINDOWS_SDK_VERSION" "$CHIGIRI_WINDOWS_CRT_VERSION" \
        x86_64 "$CHIGIRI_XWIN_VARIANT" \
        || fail "pinned Windows SDK cache is absent, incomplete, or changed"
    echo "build preflight: cargo-xwin $CHIGIRI_CARGO_XWIN_VERSION; SDK $CHIGIRI_WINDOWS_SDK_VERSION; CRT $CHIGIRI_WINDOWS_CRT_VERSION; cache $cache_dir"
}

case "$scope" in
    native) check_native ;;
    aarch64-apple-darwin|x86_64-apple-darwin) check_macos "$scope" ;;
    x86_64-unknown-linux-gnu) check_linux ;;
    x86_64-pc-windows-msvc) check_windows ;;
    all)
        require_supported_host
        [ "$host_os" = Darwin ] \
            || fail "the four-target preflight is supported only on macOS"
        check_macos aarch64-apple-darwin
        check_macos x86_64-apple-darwin
        check_linux
        check_windows
        ;;
    *) fail "unknown preflight scope: $scope" ;;
esac
