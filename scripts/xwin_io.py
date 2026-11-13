"""Bounded descriptor-relative filesystem identities and cache inventories."""

from __future__ import annotations

from dataclasses import dataclass
import fcntl
import hashlib
import os
from pathlib import Path
import stat
from typing import Optional

MANIFEST = "chigiri-cache-manifest.json"
MARKER = "chigiri-toolchain.env"
LOCK = ".chigiri-cache.lock"
VIEW_MANIFEST = ".chigiri-view.json"
MAX_ENTRIES = 100_000
MAX_BYTES = 8 * 1024 * 1024 * 1024
MAX_DEPTH = 64
MAX_LINK_EXPANSIONS = 256
MAX_LINK_COMPONENTS = 4_096
REQUIRED = {
    "xwin/crt/include": ".h",
    "xwin/crt/lib/x86_64": ".lib",
    "xwin/sdk/include/ucrt": ".h",
    "xwin/sdk/include/um": ".h",
    "xwin/sdk/include/shared": ".h",
    "xwin/sdk/include/winrt": ".h",
    "xwin/sdk/lib/ucrt/x86_64": ".lib",
    "xwin/sdk/lib/um/x86_64": ".lib",
}
CASEFOLD_LIBRARY_DIRECTORIES = (
    ("crt", "lib", "x86_64"),
    ("sdk", "lib", "ucrt", "x86_64"),
    ("sdk", "lib", "um", "x86_64"),
)


class CacheError(Exception):
    """The cache is incomplete, mutable, unsafe, or not the pinned cache."""


@dataclass
class OpenChain:
    path: Path
    descriptors: list[int]
    names: list[str]

    @property
    def leaf(self) -> int:
        return self.descriptors[-1]

    def close(self) -> None:
        for descriptor in reversed(self.descriptors):
            os.close(descriptor)


@dataclass(frozen=True)
class SnapshotEntry:
    found: os.stat_result
    target: Optional[str]


def same_identity(left: os.stat_result, right: os.stat_result) -> bool:
    return (
        left.st_dev == right.st_dev
        and left.st_ino == right.st_ino
        and stat.S_IFMT(left.st_mode) == stat.S_IFMT(right.st_mode)
    )


def same_version(left: os.stat_result, right: os.stat_result) -> bool:
    return (
        same_identity(left, right)
        and left.st_size == right.st_size
        and left.st_mtime_ns == right.st_mtime_ns
        and left.st_ctime_ns == right.st_ctime_ns
    )


def open_chain(path: Path, create: bool = False) -> OpenChain:
    if not path.is_absolute():
        raise CacheError("cache path must be absolute")
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    descriptors = [os.open("/", flags)]
    names: list[str] = []
    try:
        for part in path.parts[1:]:
            if part in {"", ".", ".."}:
                raise CacheError("cache path has an invalid component")
            parent = descriptors[-1]
            try:
                child = os.open(part, flags, dir_fd=parent)
            except FileNotFoundError:
                if not create:
                    raise
                os.mkdir(part, 0o700, dir_fd=parent)
                child = os.open(part, flags, dir_fd=parent)
            except OSError as error:
                raise CacheError("cache ancestry is not a retained directory") from error
            descriptors.append(child)
            names.append(part)
        chain = OpenChain(path, descriptors, names)
        validate_chain(chain)
        return chain
    except Exception:
        for descriptor in reversed(descriptors):
            os.close(descriptor)
        raise


def validate_chain(chain: OpenChain) -> None:
    for parent, child, name in zip(
        chain.descriptors, chain.descriptors[1:], chain.names
    ):
        retained = os.fstat(child)
        current = os.stat(name, dir_fd=parent, follow_symlinks=False)
        if not stat.S_ISDIR(current.st_mode) or not same_identity(retained, current):
            raise CacheError("cache ancestry changed or is not a retained directory")


def directory_is_empty(directory_fd: int) -> bool:
    with os.scandir(directory_fd) as entries:
        return next(entries, None) is None


def bounded_names(
    directory_fd: int,
    budget: dict[str, int],
    message: str,
    limit: Optional[int] = None,
) -> list[str]:
    maximum = MAX_ENTRIES if limit is None else limit
    names: list[str] = []
    with os.scandir(directory_fd) as entries:
        for entry in entries:
            budget["entries"] += 1
            if budget["entries"] > maximum:
                raise CacheError(message)
            names.append(entry.name)
    return sorted(names)


def require_private_empty(chain: OpenChain) -> None:
    found = os.fstat(chain.leaf)
    if found.st_uid != os.getuid():
        raise CacheError("cache root is not owned by the invoking user")
    if not directory_is_empty(chain.leaf):
        raise CacheError("Windows SDK preparation requires an empty cache directory")
    os.fchmod(chain.leaf, 0o700)
    if not directory_is_empty(chain.leaf):
        raise CacheError("Windows SDK preparation requires an empty cache directory")


def acquire_lock(cache_fd: int, exclusive: bool, create: bool) -> int:
    flags = os.O_RDWR | os.O_NOFOLLOW
    if create:
        flags |= os.O_CREAT
    try:
        descriptor = os.open(LOCK, flags, 0o600, dir_fd=cache_fd)
    except OSError as error:
        raise CacheError("cache preparation lock is absent or unsafe") from error
    try:
        found = os.fstat(descriptor)
        if not stat.S_ISREG(found.st_mode):
            raise CacheError("cache preparation lock is not a regular file")
        operation = fcntl.LOCK_EX if exclusive else fcntl.LOCK_SH
        try:
            fcntl.flock(descriptor, operation | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise CacheError("another Windows SDK cache operation is active") from error
        return descriptor
    except Exception:
        os.close(descriptor)
        raise


def open_directory(parent_fd: int, name: str) -> int:
    observed = os.stat(name, dir_fd=parent_fd, follow_symlinks=False)
    if not stat.S_ISDIR(observed.st_mode):
        raise CacheError(f"cache directory is not retained: {name}")
    descriptor = os.open(
        name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent_fd
    )
    if not same_identity(observed, os.fstat(descriptor)):
        os.close(descriptor)
        raise CacheError(f"cache directory changed while opening: {name}")
    return descriptor


def open_relative_directory(root_fd: int, parts: tuple[str, ...]) -> int:
    descriptor = os.dup(root_fd)
    try:
        for part in parts:
            child = open_directory(descriptor, part)
            os.close(descriptor)
            descriptor = child
        return descriptor
    except Exception:
        os.close(descriptor)
        raise


def read_regular(parent_fd: int, name: str, maximum: int) -> tuple[bytes, os.stat_result]:
    observed = os.stat(name, dir_fd=parent_fd, follow_symlinks=False)
    if not stat.S_ISREG(observed.st_mode):
        raise CacheError(f"cache entry is not a regular file: {name}")
    descriptor = os.open(name, os.O_RDONLY | os.O_NOFOLLOW, dir_fd=parent_fd)
    try:
        opened = os.fstat(descriptor)
        if not same_version(observed, opened):
            raise CacheError(f"cache file changed while opening: {name}")
        if opened.st_size > maximum:
            raise CacheError("cache read exceeds its resource limit")
        data = bytearray()
        while True:
            allowance = max(1, maximum + 1 - len(data))
            block = os.read(descriptor, min(1024 * 1024, allowance))
            if not block:
                break
            data.extend(block)
            if len(data) > maximum:
                raise CacheError("cache read exceeds its resource limit")
        after = os.fstat(descriptor)
        if not same_version(opened, after) or len(data) != opened.st_size:
            raise CacheError(f"cache file changed while reading: {name}")
        return bytes(data), opened
    finally:
        os.close(descriptor)


def digest_regular(
    parent_fd: int, name: str, maximum: int
) -> tuple[str, os.stat_result, int]:
    observed = os.stat(name, dir_fd=parent_fd, follow_symlinks=False)
    if not stat.S_ISREG(observed.st_mode):
        raise CacheError(f"cache entry is not a regular file: {name}")
    descriptor = os.open(name, os.O_RDONLY | os.O_NOFOLLOW, dir_fd=parent_fd)
    try:
        opened = os.fstat(descriptor)
        if not same_version(observed, opened):
            raise CacheError(f"cache file changed while opening: {name}")
        if opened.st_size > maximum:
            raise CacheError("cache inventory exceeds its byte limit")
        digest = hashlib.sha256()
        count = 0
        while True:
            block = os.read(descriptor, min(1024 * 1024, max(1, maximum + 1 - count)))
            if not block:
                break
            count += len(block)
            if count > maximum:
                raise CacheError("cache inventory exceeds its byte limit")
            digest.update(block)
        after = os.fstat(descriptor)
        if not same_version(opened, after) or count != opened.st_size:
            raise CacheError(f"cache file changed while reading: {name}")
        return digest.hexdigest(), opened, count
    finally:
        os.close(descriptor)


def inventory_tree(
    root_fd: int,
    prefix: tuple[str, ...] = (),
    exclusions: frozenset[str] = frozenset(),
    require_read_only: bool = False,
) -> list[dict[str, object]]:
    entries: list[dict[str, object]] = []
    budget = {"entries": 0}
    total_bytes = 0

    def walk(directory_fd: int, relative: tuple[str, ...]) -> None:
        nonlocal total_bytes
        if len(relative) > MAX_DEPTH:
            raise CacheError("cache directory depth exceeds its resource limit")
        before = os.fstat(directory_fd)
        if require_read_only and before.st_mode & 0o222:
            raise CacheError("staged xwin directory is writable")
        for name in bounded_names(
            directory_fd, budget, "cache inventory exceeds its entry limit"
        ):
            if not relative and name in exclusions:
                continue
            if name in {"", ".", ".."} or "/" in name:
                raise CacheError("cache entry has an invalid name")
            found = os.stat(name, dir_fd=directory_fd, follow_symlinks=False)
            path = relative + (name,)
            rendered = "/".join(prefix + path)
            if stat.S_ISDIR(found.st_mode):
                child = open_directory(directory_fd, name)
                try:
                    entries.append({"kind": "directory", "path": rendered})
                    walk(child, path)
                finally:
                    os.close(child)
            elif stat.S_ISREG(found.st_mode):
                if require_read_only and found.st_mode & 0o222:
                    raise CacheError("staged xwin file is writable")
                remaining = MAX_BYTES - total_bytes
                digest, opened, read_bytes = digest_regular(directory_fd, name, remaining)
                total_bytes += read_bytes
                entries.append(
                    {
                        "kind": "file",
                        "path": rendered,
                        "size": opened.st_size,
                        "sha256": digest,
                    }
                )
            elif stat.S_ISLNK(found.st_mode):
                raise CacheError(f"cache entry is a symlink: {rendered}")
            else:
                raise CacheError(f"cache entry is not regular: {rendered}")
        after = os.fstat(directory_fd)
        if not same_version(before, after):
            raise CacheError("cache directory changed while inventorying")

    walk(root_fd, ())
    return entries


def validate_layout(cache_fd: int, entries: list[dict[str, object]]) -> None:
    xwin_fd = open_directory(cache_fd, "xwin")
    try:
        done, _ = read_regular(xwin_fd, "DONE", 1024 * 1024)
    finally:
        os.close(xwin_fd)
    first = done.decode("utf-8").splitlines()[:1]
    if not first or "x86_64" not in first[0].split():
        raise CacheError("cargo-xwin DONE record has no x86_64 libraries")
    file_paths = {
        str(item["path"])
        for item in entries
        if item.get("kind") == "file"
    }
    for relative, suffix in REQUIRED.items():
        prefix = relative + "/"
        if not any(path.startswith(prefix) and path.lower().endswith(suffix) for path in file_paths):
            raise CacheError(f"required cache content is absent: {relative}/*{suffix}")
