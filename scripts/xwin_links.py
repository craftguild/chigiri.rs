"""Discover and normalize confined cargo-xwin aliases without following swaps."""

from __future__ import annotations

import os
import secrets
import stat
from typing import Optional

from xwin_io import (CacheError, SnapshotEntry, MAX_BYTES, MAX_DEPTH, MAX_ENTRIES,
    MAX_LINK_EXPANSIONS, MAX_LINK_COMPONENTS, bounded_names, open_directory,
    open_relative_directory, same_identity, same_version)

def snapshot_links(xwin_fd: int) -> dict[tuple[str, ...], SnapshotEntry]:
    entries: dict[tuple[str, ...], SnapshotEntry] = {
        (): SnapshotEntry(os.fstat(xwin_fd), None)
    }
    budget = {"entries": 0}

    def walk(directory_fd: int, relative: tuple[str, ...]) -> None:
        if len(relative) > MAX_DEPTH:
            raise CacheError("cache directory depth exceeds its resource limit")
        before = os.fstat(directory_fd)
        for name in bounded_names(
            directory_fd, budget, "cache normalization exceeds its entry limit"
        ):
            found = os.stat(name, dir_fd=directory_fd, follow_symlinks=False)
            path = relative + (name,)
            if stat.S_ISDIR(found.st_mode):
                entries[path] = SnapshotEntry(found, None)
                child = open_directory(directory_fd, name)
                try:
                    walk(child, path)
                finally:
                    os.close(child)
            elif stat.S_ISREG(found.st_mode):
                entries[path] = SnapshotEntry(found, None)
            elif stat.S_ISLNK(found.st_mode):
                entries[path] = SnapshotEntry(found, os.readlink(name, dir_fd=directory_fd))
            else:
                raise CacheError("cache contains a special file")
        if not same_version(before, os.fstat(directory_fd)):
            raise CacheError("cache directory changed during normalization discovery")

    walk(xwin_fd, ())
    return entries


def lexical_target(parent: tuple[str, ...], target: str) -> list[str]:
    if target.startswith("/"):
        raise CacheError("cache link has an absolute target")
    parts = list(parent)
    for part in target.split("/"):
        if part in {"", "."}:
            continue
        if part == "..":
            if not parts:
                raise CacheError("cache link target escapes the xwin directory")
            parts.pop()
        else:
            parts.append(part)
        if len(parts) > MAX_LINK_COMPONENTS:
            raise CacheError("cache link target exceeds its component limit")
    return parts


def resolve_snapshot_link(
    entries: dict[tuple[str, ...], SnapshotEntry],
    link: tuple[str, ...],
    target: str,
) -> tuple[tuple[str, ...], SnapshotEntry]:
    remaining = lexical_target(link[:-1], target)
    current: list[str] = []
    visited: set[tuple[int, int]] = set()
    expansions = 0
    while remaining:
        current.append(remaining.pop(0))
        key = tuple(current)
        entry = entries.get(key)
        if entry is None:
            raise CacheError("cache link target is absent")
        if stat.S_ISLNK(entry.found.st_mode):
            identity = (entry.found.st_dev, entry.found.st_ino)
            if identity in visited:
                raise CacheError("cache link target is cyclic")
            visited.add(identity)
            expansions += 1
            if expansions > MAX_LINK_EXPANSIONS:
                raise CacheError("cache link chain exceeds its resource limit")
            assert entry.target is not None
            remaining = lexical_target(key[:-1], entry.target) + remaining
            if len(remaining) > MAX_LINK_COMPONENTS:
                raise CacheError("cache link chain exceeds its component limit")
            current = []
        elif remaining and not stat.S_ISDIR(entry.found.st_mode):
            raise CacheError("cache link traverses a non-directory entry")
    key = tuple(current)
    entry = entries.get(key)
    if entry is None:
        raise CacheError("cache link target is absent")
    if not (stat.S_ISREG(entry.found.st_mode) or stat.S_ISDIR(entry.found.st_mode)):
        raise CacheError("cache link target is not a regular file or directory")
    return key, entry


def same_snapshot(
    left: dict[tuple[str, ...], SnapshotEntry],
    right: dict[tuple[str, ...], SnapshotEntry],
) -> bool:
    if left.keys() != right.keys():
        return False
    return all(
        left[path].target == right[path].target
        and same_version(left[path].found, right[path].found)
        for path in left
    )


def unique_name(prefix: str) -> str:
    return f".{prefix}-{secrets.token_hex(16)}"


def consume_entry(budget: dict[str, int]) -> None:
    budget["entries"] += 1
    if budget["entries"] > MAX_ENTRIES:
        raise CacheError("cache copy exceeds its entry limit")


def copy_retained_file(
    source_root: int,
    source: tuple[str, ...],
    destination_root: int,
    destination: tuple[str, ...],
    observed: os.stat_result,
    budget: dict[str, int],
) -> None:
    source_parent = open_relative_directory(source_root, source[:-1])
    destination_parent = open_relative_directory(destination_root, destination[:-1])
    source_fd = None
    destination_fd = None
    try:
        source_fd = os.open(source[-1], os.O_RDONLY | os.O_NOFOLLOW, dir_fd=source_parent)
        opened = os.fstat(source_fd)
        if not same_version(opened, observed):
            raise CacheError("cache source file changed while copying")
        if opened.st_size > MAX_BYTES - budget["bytes"]:
            raise CacheError("cache copy exceeds its byte limit")
        destination_fd = os.open(
            destination[-1],
            os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
            0o400,
            dir_fd=destination_parent,
        )
        copied = 0
        while True:
            remaining = MAX_BYTES - budget["bytes"] - copied
            block = os.read(source_fd, min(1024 * 1024, max(1, remaining + 1)))
            if not block:
                break
            copied += len(block)
            if copied > MAX_BYTES - budget["bytes"]:
                raise CacheError("cache copy exceeds its byte limit")
            offset = 0
            while offset < len(block):
                offset += os.write(destination_fd, block[offset:])
        if not same_version(opened, os.fstat(source_fd)) or copied != opened.st_size:
            raise CacheError("cache source file changed while copying")
        budget["bytes"] += copied
        os.fchmod(destination_fd, 0o444)
        os.fsync(destination_fd)
    finally:
        if destination_fd is not None:
            os.close(destination_fd)
        if source_fd is not None:
            os.close(source_fd)
        os.close(destination_parent)
        os.close(source_parent)


def install_normalized_xwin(source_cache_fd: int, destination_cache_fd: int) -> None:
    source_xwin = open_directory(source_cache_fd, "xwin")
    destination_xwin = None
    try:
        entries = snapshot_links(source_xwin)
        resolutions: dict[tuple[str, ...], tuple[tuple[str, ...], SnapshotEntry]] = {}
        for path, entry in entries.items():
            if entry.target is not None:
                resolutions[path] = resolve_snapshot_link(entries, path, entry.target)
        os.mkdir("xwin", 0o700, dir_fd=destination_cache_fd)
        destination_xwin = open_directory(destination_cache_fd, "xwin")
        budget = {"entries": 1, "bytes": 0}
        directories: list[tuple[str, ...]] = [()]
        for path, entry in sorted(entries.items(), key=lambda item: (len(item[0]), item[0])):
            if path and stat.S_ISDIR(entry.found.st_mode):
                consume_entry(budget)
                parent = open_relative_directory(destination_xwin, path[:-1])
                try:
                    os.mkdir(path[-1], 0o700, dir_fd=parent)
                finally:
                    os.close(parent)
                directories.append(path)
        for path, entry in sorted(entries.items()):
            if path and stat.S_ISREG(entry.found.st_mode):
                consume_entry(budget)
                copy_retained_file(
                    source_xwin, path, destination_xwin, path, entry.found, budget
                )
        for path, entry in sorted(entries.items()):
            if path and entry.target is not None:
                resolved, resolved_entry = resolutions[path]
                if stat.S_ISREG(resolved_entry.found.st_mode):
                    consume_entry(budget)
                    if resolved_entry.found.st_size > MAX_BYTES - budget["bytes"]:
                        raise CacheError("cache copy exceeds its byte limit")
                    source_parent = open_relative_directory(destination_xwin, resolved[:-1])
                    destination_parent = open_relative_directory(destination_xwin, path[:-1])
                    try:
                        source_stat = os.stat(
                            resolved[-1], dir_fd=source_parent, follow_symlinks=False
                        )
                        if (
                            not stat.S_ISREG(source_stat.st_mode)
                            or source_stat.st_size != resolved_entry.found.st_size
                        ):
                            raise CacheError("normalized link target changed")
                        os.link(
                            resolved[-1], path[-1],
                            src_dir_fd=source_parent,
                            dst_dir_fd=destination_parent,
                            follow_symlinks=False,
                        )
                        budget["bytes"] += source_stat.st_size
                    finally:
                        os.close(destination_parent)
                        os.close(source_parent)
        if not same_snapshot(snapshot_links(source_xwin), entries):
            raise CacheError("cargo-xwin output changed while normalizing")
        for path in sorted(directories, key=len, reverse=True):
            directory = open_relative_directory(destination_xwin, path)
            try:
                os.fchmod(directory, 0o555)
                os.fsync(directory)
            finally:
                os.close(directory)
    finally:
        if destination_xwin is not None:
            os.close(destination_xwin)
        os.close(source_xwin)


def delete_tree(directory_fd: int, depth: int = 0, budget: Optional[dict[str, int]] = None) -> None:
    if depth > MAX_DEPTH:
        raise CacheError("cache cleanup depth exceeds its resource limit")
    if budget is None:
        budget = {"entries": 0}
    for name in bounded_names(
        directory_fd, budget, "cache cleanup exceeds its entry limit"
    ):
        found = os.stat(name, dir_fd=directory_fd, follow_symlinks=False)
        if stat.S_ISDIR(found.st_mode):
            child = open_directory(directory_fd, name)
            try:
                delete_tree(child, depth + 1, budget)
            finally:
                os.close(child)
            os.rmdir(name, dir_fd=directory_fd)
        else:
            os.unlink(name, dir_fd=directory_fd)
