"""Copy a verified SDK into a bounded disposable build view and verify its seal."""

from __future__ import annotations

import os
import json
from pathlib import Path
import stat

from xwin_io import (CacheError, MAX_BYTES, MAX_DEPTH, CASEFOLD_LIBRARY_DIRECTORIES,
    VIEW_MANIFEST, bounded_names, inventory_tree, open_chain, open_directory,
    open_relative_directory, require_private_empty, same_identity, same_version,
    validate_chain)
from xwin_links import consume_entry
from xwin_records import canonical, exclusive_write, read_document, verify_fd, with_cache

def copy_file(
    source_fd: int, destination_fd: int, name: str, budget: dict[str, int]
) -> None:
    observed = os.stat(name, dir_fd=source_fd, follow_symlinks=False)
    if (
        not stat.S_ISREG(observed.st_mode)
        or observed.st_size > MAX_BYTES - budget["bytes"]
    ):
        raise CacheError("cache file cannot be staged safely")
    source = os.open(name, os.O_RDONLY | os.O_NOFOLLOW, dir_fd=source_fd)
    destination = os.open(
        name,
        os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
        0o400,
        dir_fd=destination_fd,
    )
    try:
        opened = os.fstat(source)
        if not same_version(observed, opened):
            raise CacheError("cache file changed while staging")
        copied = 0
        while True:
            remaining = MAX_BYTES - budget["bytes"] - copied
            block = os.read(source, min(1024 * 1024, max(1, remaining + 1)))
            if not block:
                break
            copied += len(block)
            if copied > MAX_BYTES - budget["bytes"]:
                raise CacheError("cache file exceeds its staging limit")
            offset = 0
            while offset < len(block):
                offset += os.write(destination, block[offset:])
        if not same_version(opened, os.fstat(source)) or copied != opened.st_size:
            raise CacheError("cache file changed while staging")
        budget["bytes"] += copied
        os.fchmod(destination, 0o444)
        os.fsync(destination)
    finally:
        os.close(destination)
        os.close(source)


def copy_tree(
    source_fd: int,
    destination_fd: int,
    budget: dict[str, int],
    depth: int = 0,
) -> None:
    if depth > MAX_DEPTH:
        raise CacheError("cache directory depth exceeds its resource limit")
    before = os.fstat(source_fd)
    for name in bounded_names(
        source_fd, budget, "cache copy exceeds its entry limit"
    ):
        found = os.stat(name, dir_fd=source_fd, follow_symlinks=False)
        if stat.S_ISDIR(found.st_mode):
            os.mkdir(name, 0o700, dir_fd=destination_fd)
            source_child = open_directory(source_fd, name)
            destination_child = open_directory(destination_fd, name)
            try:
                copy_tree(source_child, destination_child, budget, depth + 1)
                os.fchmod(destination_child, 0o555)
                os.fsync(destination_child)
            finally:
                os.close(destination_child)
                os.close(source_child)
        elif stat.S_ISREG(found.st_mode):
            copy_file(source_fd, destination_fd, name, budget)
        else:
            raise CacheError("sealed cache contains a non-regular entry")
    if not same_version(before, os.fstat(source_fd)):
        raise CacheError("cache changed while staging")


def add_casefold_library_aliases(xwin_fd: int, budget: dict[str, int]) -> None:
    scan_budget = {"entries": 0}
    for parts in CASEFOLD_LIBRARY_DIRECTORIES:
        directory_fd = open_relative_directory(xwin_fd, parts)
        try:
            os.fchmod(directory_fd, 0o700)
            for name in bounded_names(
                directory_fd, scan_budget,
                "library alias scan exceeds its entry limit",
            ):
                alias = name.lower()
                if alias == name or not alias.endswith(".lib"):
                    continue
                found = os.stat(name, dir_fd=directory_fd, follow_symlinks=False)
                if not stat.S_ISREG(found.st_mode):
                    raise CacheError("library alias source is not a regular file")
                try:
                    existing = os.stat(alias, dir_fd=directory_fd, follow_symlinks=False)
                except FileNotFoundError:
                    consume_entry(budget)
                    os.link(
                        name, alias, src_dir_fd=directory_fd,
                        dst_dir_fd=directory_fd, follow_symlinks=False,
                    )
                    existing = os.stat(alias, dir_fd=directory_fd, follow_symlinks=False)
                    if not same_identity(found, existing):
                        raise CacheError("library alias identity changed")
                if not stat.S_ISREG(existing.st_mode):
                    raise CacheError("library alias is not a regular file")
            os.fsync(directory_fd)
        finally:
            os.fchmod(directory_fd, 0o555)
            os.close(directory_fd)


def xwin_inventory(cache_fd: int, read_only: bool) -> list[dict[str, object]]:
    xwin_fd = open_directory(cache_fd, "xwin")
    try:
        root = os.fstat(xwin_fd)
        if read_only and root.st_mode & 0o222:
            raise CacheError("staged xwin directory is writable")
        result = [{"kind": "directory", "path": "xwin"}]
        result.extend(inventory_tree(xwin_fd, prefix=("xwin",), require_read_only=read_only))
        return result
    finally:
        os.close(xwin_fd)


def stage_view(source: Path, destination: Path, identity: dict[str, str]) -> None:
    source_chain, lock = with_cache(source, False, False)
    destination_chain = None
    try:
        document = verify_fd(source_chain.leaf, identity)
        destination_chain = open_chain(destination, create=True)
        require_private_empty(destination_chain)
        source_xwin = open_directory(source_chain.leaf, "xwin")
        os.mkdir("xwin", 0o700, dir_fd=destination_chain.leaf)
        destination_xwin = open_directory(destination_chain.leaf, "xwin")
        try:
            budget = {"entries": 1, "bytes": 0}
            copy_tree(source_xwin, destination_xwin, budget)
            os.fchmod(destination_xwin, 0o555)
            os.fsync(destination_xwin)
            copied = xwin_inventory(destination_chain.leaf, True)
            expected = [
                entry for entry in document["files"]
                if str(entry["path"]) == "xwin"
                or str(entry["path"]).startswith("xwin/")
            ]
            if copied != expected:
                raise CacheError("staged xwin view differs from the sealed cache")
            add_casefold_library_aliases(destination_xwin, budget)
        finally:
            os.close(destination_xwin)
            os.close(source_xwin)
        actual = xwin_inventory(destination_chain.leaf, True)
        exclusive_write(
            destination_chain.leaf,
            VIEW_MANIFEST,
            canonical({"schema": 1, "files": actual}),
        )
        if verify_fd(source_chain.leaf, identity) != document:
            raise CacheError("sealed cache changed while staging")
        validate_chain(source_chain)
        validate_chain(destination_chain)
    finally:
        if destination_chain is not None:
            destination_chain.close()
        os.close(lock)
        source_chain.close()


def verify_view(path: Path) -> None:
    chain = open_chain(path)
    try:
        document = json.loads(read_document(chain.leaf, VIEW_MANIFEST).decode("utf-8"))
        actual = xwin_inventory(chain.leaf, True)
        if document != {"schema": 1, "files": actual}:
            raise CacheError("staged xwin view changed")
        validate_chain(chain)
    finally:
        chain.close()


def unseal_view(path: Path) -> None:
    chain = open_chain(path)
    try:
        xwin_fd = open_directory(chain.leaf, "xwin")
        try:
            budget = {"entries": 0}

            def make_writable(directory_fd: int, depth: int = 0) -> None:
                if depth > MAX_DEPTH:
                    raise CacheError("staged view depth exceeds its resource limit")
                for name in bounded_names(
                    directory_fd, budget, "staged view exceeds its entry limit"
                ):
                    found = os.stat(name, dir_fd=directory_fd, follow_symlinks=False)
                    if stat.S_ISDIR(found.st_mode):
                        child = open_directory(directory_fd, name)
                        try:
                            make_writable(child, depth + 1)
                        finally:
                            os.close(child)
                os.fchmod(directory_fd, 0o700)

            make_writable(xwin_fd)
        finally:
            os.close(xwin_fd)
    finally:
        chain.close()
