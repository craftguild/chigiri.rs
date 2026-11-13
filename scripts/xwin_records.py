"""Record and verify immutable, identity-bound Windows SDK cache receipts."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path

from xwin_io import (CacheError, MANIFEST, MARKER, acquire_lock, inventory_tree,
    open_chain, read_regular, validate_chain, validate_layout)

def pins(arguments: list[str]) -> dict[str, str]:
    if len(arguments) != 6:
        raise CacheError("six pinned cache identity values are required")
    names = ("cargo-xwin", "visual-studio", "sdk", "crt", "arch", "variant")
    return dict(zip(names, arguments))


def canonical(document: dict[str, object]) -> bytes:
    return (json.dumps(document, sort_keys=True, separators=(",", ":")) + "\n").encode()


def marker(document: dict[str, object]) -> bytes:
    identity = document["identity"]
    assert isinstance(identity, dict)
    names = ("cargo-xwin", "visual-studio", "sdk", "crt", "arch", "variant")
    lines = [f"{name}={identity[name]}" for name in names]
    lines.append(f"inventory-sha256={hashlib.sha256(canonical(document)).hexdigest()}")
    return ("\n".join(lines) + "\n").encode()


def exclusive_write(cache_fd: int, name: str, data: bytes) -> None:
    descriptor = os.open(
        name,
        os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
        0o600,
        dir_fd=cache_fd,
    )
    try:
        offset = 0
        while offset < len(data):
            offset += os.write(descriptor, data[offset:])
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def record_fd(cache_fd: int, identity: dict[str, str]) -> dict[str, object]:
    files = inventory_tree(cache_fd, exclusions=frozenset({MANIFEST, MARKER}))
    validate_layout(cache_fd, files)
    document: dict[str, object] = {"schema": 2, "identity": identity, "files": files}
    exclusive_write(cache_fd, MANIFEST, canonical(document))
    repeated = inventory_tree(cache_fd, exclusions=frozenset({MANIFEST, MARKER}))
    if repeated != files:
        raise CacheError("cache changed while provenance was recorded")
    os.fsync(cache_fd)
    exclusive_write(cache_fd, MARKER, marker(document))
    return document


def read_document(cache_fd: int, name: str) -> bytes:
    data, _ = read_regular(cache_fd, name, 64 * 1024 * 1024)
    return data


def verify_fd(cache_fd: int, identity: dict[str, str]) -> dict[str, object]:
    files = inventory_tree(cache_fd, exclusions=frozenset({MANIFEST, MARKER}))
    validate_layout(cache_fd, files)
    document = json.loads(read_document(cache_fd, MANIFEST).decode("utf-8"))
    expected: dict[str, object] = {"schema": 2, "identity": identity, "files": files}
    if document != expected:
        raise CacheError("cache contents or pinned identity differ from the recorded inventory")
    if read_document(cache_fd, MARKER) != marker(expected):
        raise CacheError("cache marker differs from the recorded inventory identity")
    if inventory_tree(cache_fd, exclusions=frozenset({MANIFEST, MARKER})) != files:
        raise CacheError("cache changed during verification")
    return expected


def with_cache(path: Path, exclusive: bool, create_lock: bool):
    chain = open_chain(path)
    try:
        lock = acquire_lock(chain.leaf, exclusive, create_lock)
    except Exception:
        chain.close()
        raise
    return chain, lock


def verify_path(path: Path, identity: dict[str, str]) -> None:
    chain, lock = with_cache(path, False, False)
    try:
        verify_fd(chain.leaf, identity)
        validate_chain(chain)
    finally:
        os.close(lock)
        chain.close()


def cargo_environment(cache: str, identity: dict[str, str], retries: str) -> dict[str, str]:
    if any(name.startswith("XWIN_") for name in os.environ):
        raise CacheError("cargo-xwin overrides are not allowed")
    environment = os.environ.copy()
    environment.update(
        XWIN_CACHE_DIR=cache,
        XWIN_ARCH=identity["arch"],
        XWIN_VERSION=identity["visual-studio"],
        XWIN_SDK_VERSION=identity["sdk"],
        XWIN_CRT_VERSION=identity["crt"],
        XWIN_VARIANT=identity["variant"],
        XWIN_CROSS_COMPILER="clang-cl",
        XWIN_INCLUDE_ATL="false",
        XWIN_INCLUDE_DEBUG_LIBS="false",
        XWIN_INCLUDE_DEBUG_SYMBOLS="false",
        XWIN_HTTP_RETRIES=retries,
    )
    return environment
