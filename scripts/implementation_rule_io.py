"""Provide bounded, no-follow file reads for repository checkers."""

from contextlib import contextmanager
import os
from pathlib import Path
import stat
import unicodedata

MAX_BYTES = 64 * 1024 * 1024
MAX_ENTRIES = 65536
DIRECTORY_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC


class Invalid(ValueError):
    pass


def require(condition):
    if not condition:
        raise Invalid()


def components(path):
    require(isinstance(path, str) and 0 < len(path.encode()) <= 4096)
    parts = path.split("/")
    require(len(parts) <= 64 and "\\" not in path and ":" not in path)
    require(not any(unicodedata.category(char) == "Cc" for char in path))
    require(all(part not in ("", ".", "..") and len(part.encode()) <= 255 for part in parts))
    return parts


def identity(value):
    return value.st_dev, value.st_ino, stat.S_IFMT(value.st_mode)


def snapshot(value):
    return identity(value), value.st_size, value.st_mtime_ns, value.st_ctime_ns


@contextmanager
def directory(path):
    """Retain ancestor descriptors and reject aliases to the same inode."""
    entries = [(os.open("/", DIRECTORY_FLAGS), None, None)]
    try:
        for name in Path(path).absolute().parts[1:]:
            components(name)
            parent = entries[-1][0]
            fd = os.open(name, DIRECTORY_FLAGS, dir_fd=parent)
            entries.append((fd, parent, name))
        yield entries[-1][0]
        for fd, parent, name in entries:
            if parent is not None:
                require(identity(os.fstat(fd)) == identity(os.stat(name, dir_fd=parent, follow_symlinks=False)))
    finally:
        for fd, _, _ in reversed(entries):
            os.close(fd)


def read_at(parent, name, maximum):
    components(name)
    require("/" not in name and maximum >= 0)
    fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC, dir_fd=parent)
    with os.fdopen(fd, "rb") as source:
        before = os.fstat(source.fileno())
        require(stat.S_ISREG(before.st_mode) and before.st_size <= maximum)
        data = source.read(maximum + 1)
        after = os.fstat(source.fileno())
        named = os.stat(name, dir_fd=parent, follow_symlinks=False)
        require(len(data) <= maximum and len(data) == after.st_size)
        require(snapshot(before) == snapshot(after) == snapshot(named))
        return data


def read_file(path, maximum):
    with directory(path.parent) as parent:
        return read_at(parent, path.name, maximum)


def visit_tree(root, inspect):
    """Inventory paths without reading file bytes; retain empty directories and reject unknown entries."""
    state = {"entries": 0, "bytes": 0, "case_paths": set()}
    files, directories = set(), set()

    def visit(fd, prefix, depth):
        require(depth <= 64)
        before = snapshot(os.fstat(fd))
        with os.scandir(fd) as entries:
            for entry in entries:
                state["entries"] += 1
                require(state["entries"] <= MAX_ENTRIES)
                path = prefix + entry.name
                components(path)
                require(path.casefold() not in state["case_paths"])
                state["case_paths"].add(path.casefold())
                original = entry.stat(follow_symlinks=False)
                if stat.S_ISDIR(original.st_mode):
                    child = os.open(entry.name, DIRECTORY_FLAGS, dir_fd=fd)
                    try:
                        require(identity(original) == identity(os.fstat(child)))
                        directories.add(path)
                        visit(child, path + "/", depth + 1)
                        require(identity(os.fstat(child)) == identity(os.stat(entry.name, dir_fd=fd, follow_symlinks=False)))
                    finally:
                        os.close(child)
                else:
                    require(stat.S_ISREG(original.st_mode))
                    data = read_at(fd, entry.name, MAX_BYTES - state["bytes"])
                    state["bytes"] += len(data)
                    inspect(path, data)
                    files.add(path)
        require(snapshot(os.fstat(fd)) == before)

    with directory(root) as fd:
        visit(fd, "", 0)
    return files, directories
