#!/usr/bin/env python3
"""Acquire pinned SDK inputs or dispatch descriptor-safe cache/view operations."""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys

import xwin_io as filesystem
import xwin_links as links
import xwin_records as records
import xwin_view as views
from xwin_io import (CacheError, SnapshotEntry, MANIFEST, MAX_BYTES, bounded_names,
    open_chain, acquire_lock, require_private_empty, validate_chain, open_directory,
    same_identity)
from xwin_links import (unique_name, install_normalized_xwin, delete_tree,
    snapshot_links, copy_retained_file)
from xwin_records import (cargo_environment, record_fd, pins, verify_path, with_cache,
    exclusive_write, verify_fd)
from xwin_view import stage_view, verify_view, unseal_view

def prepare(path: Path, toolchain: str, identity: dict[str, str]) -> None:
    chain = open_chain(path, create=True)
    lock = None
    try:
        require_private_empty(chain)
        lock = acquire_lock(chain.leaf, True, True)
        raw_name = unique_name("chigiri-xwin-acquisition")
        os.mkdir(raw_name, 0o700, dir_fd=chain.leaf)
        environment = cargo_environment(raw_name, identity, "3")

        def enter_cache() -> None:
            os.fchdir(chain.leaf)

        version = subprocess.run(
            ["cargo", f"+{toolchain}", "xwin", "--version"],
            env=environment,
            pass_fds=(chain.leaf, lock),
            preexec_fn=enter_cache,
            capture_output=True,
            text=True,
            check=False,
        )
        # Match the pinned release's Cargo subcommand banner, not its direct CLI banner.
        expected_version = f"cargo-xwin-xwin {identity['cargo-xwin']}"
        if version.returncode != 0 or version.stdout.strip() != expected_version:
            raise CacheError(
                f"cargo-xwin {identity['cargo-xwin']} is required; found: "
                f"{version.stdout.strip() or 'none'}"
            )
        validate_chain(chain)
        acquired = subprocess.run(
            ["cargo", f"+{toolchain}", "xwin", "cache", "xwin"],
            env=environment,
            pass_fds=(chain.leaf, lock),
            preexec_fn=enter_cache,
            check=False,
        )
        if acquired.returncode != 0:
            raise CacheError("cargo-xwin cache acquisition failed")
        validate_chain(chain)
        raw_fd = open_directory(chain.leaf, raw_name)
        try:
            raw_names = bounded_names(
                raw_fd,
                {"entries": 0},
                "cargo-xwin acquisition root has unexpected entries",
                1,
            )
            if raw_names != ["xwin"]:
                raise CacheError("cargo-xwin acquisition root has unexpected entries")
            install_normalized_xwin(raw_fd, chain.leaf)
            delete_tree(raw_fd)
        finally:
            os.close(raw_fd)
        os.rmdir(raw_name, dir_fd=chain.leaf)
        validate_chain(chain)
        record_fd(chain.leaf, identity)
        validate_chain(chain)
    finally:
        if lock is not None:
            os.close(lock)
        chain.close()


def usage() -> int:
    print(
        "usage: xwin-cache.py <verify-view|unseal-view> <cache-or-view>\n"
        "   or: xwin-cache.py verify <cache> <cargo-xwin> "
        "<visual-studio> <sdk> <crt> <arch> <variant>\n"
        "   or: xwin-cache.py stage <cache> <view> <cargo-xwin> "
        "<visual-studio> <sdk> <crt> <arch> <variant>\n"
        "   or: xwin-cache.py prepare <cache> <rust-toolchain> <cargo-xwin> "
        "<visual-studio> <sdk> <crt> <arch> <variant>",
        file=sys.stderr,
    )
    return 2


def main() -> int:
    action = sys.argv[1] if len(sys.argv) > 1 else ""
    try:
        if action in {"verify-view", "unseal-view"} and len(sys.argv) == 3:
            operation = {
                "verify-view": verify_view,
                "unseal-view": unseal_view,
            }[action]
            operation(Path(sys.argv[2]))
        elif action == "verify" and len(sys.argv) == 9:
            verify_path(Path(sys.argv[2]), pins(sys.argv[3:]))
        elif action == "stage" and len(sys.argv) == 10:
            stage_view(Path(sys.argv[2]), Path(sys.argv[3]), pins(sys.argv[4:]))
        elif action == "prepare" and len(sys.argv) == 10:
            prepare(Path(sys.argv[2]), sys.argv[3], pins(sys.argv[4:]))
        else:
            return usage()
    except (CacheError, OSError, ValueError, json.JSONDecodeError) as error:
        print(f"Windows SDK cache verification failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
