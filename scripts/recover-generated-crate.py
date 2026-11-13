#!/usr/bin/env python3
"""Safely restore a Chigiri archive into a specified missing directory."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import struct
import sys
import unicodedata

MAGIC = b"chigiri/recovery-v1\n"
MAX_ARCHIVE = 68 * 1024 * 1024
MAX_BYTES = 64 * 1024 * 1024
MARKER = "chigiri-generated.json"
FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC


class Invalid(Exception):
    pass


def components(path):
    parts = path.split("/")
    if (not path or len(path.encode()) > 4096 or len(parts) > 64
            or "\\" in path or ":" in path
            or any(unicodedata.category(char) == "Cc" for char in path)
            or any(part in ("", ".", "..") or len(part.encode()) > 255 for part in parts)):
        raise Invalid()
    return parts


def identity(value):
    return value.st_dev, value.st_ino, stat.S_IFMT(value.st_mode)


def contents(value):
    return identity(value), value.st_size, value.st_mtime_ns, value.st_ctime_ns


class Directory:
    """Open each path component without retaining ancestor descriptors."""

    def __init__(self, path):
        absolute = Path(os.path.abspath(path))
        self.entries = [(os.open("/", FLAGS), None, None)]
        try:
            for name in absolute.parts[1:]:
                components(name)
                parent = self.entries[-1][0]
                descriptor = os.open(name, FLAGS, dir_fd=parent)
                self.entries.append((descriptor, parent, name))
            self.verify()
        except BaseException:
            self.close()
            raise

    @property
    def fd(self):
        return self.entries[-1][0]

    def verify(self):
        for descriptor, parent, name in self.entries:
            actual = os.fstat(descriptor)
            if parent is not None:
                named = os.stat(name, dir_fd=parent, follow_symlinks=False)
                if identity(actual) != identity(named):
                    raise Invalid()

    def close(self):
        for descriptor, _, _ in reversed(self.entries):
            os.close(descriptor)
        self.entries.clear()


def read_archive(path):
    path = Path(path)
    components(path.name)
    parent = Directory(path.parent)
    try:
        descriptor = os.open(path.name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC,
                             dir_fd=parent.fd)
        with os.fdopen(descriptor, "rb") as source:
            before = os.fstat(source.fileno())
            if not stat.S_ISREG(before.st_mode) or before.st_size > MAX_ARCHIVE:
                raise Invalid()
            data = source.read(MAX_ARCHIVE + 1)
            after = os.fstat(source.fileno())
            named = os.stat(path.name, dir_fd=parent.fd, follow_symlinks=False)
            parent.verify()
            if (len(data) > MAX_ARCHIVE or contents(before) != contents(after)
                    or contents(after) != contents(named) or len(data) != after.st_size):
                raise Invalid()
        return data
    finally:
        parent.close()


def decode(data):
    cursor = 0

    def take(length):
        nonlocal cursor
        if length > len(data) - cursor:
            raise Invalid()
        value = data[cursor:cursor + length]
        cursor += length
        return value

    def integer(format_code):
        return struct.unpack(format_code, take(struct.calcsize(format_code)))[0]

    if take(len(MAGIC)) != MAGIC:
        raise Invalid()
    output_length = integer(">I")
    if not 1 <= output_length <= 255:
        raise Invalid()
    output = take(output_length).decode("utf-8")
    if len(components(output)) != 1:
        raise Invalid()
    count = integer(">Q")
    if not 1 <= count <= 65536:
        raise Invalid()
    files = {}
    directories = set()
    previous = ""
    total = 0
    for _ in range(count):
        name_length, length = integer(">I"), integer(">Q")
        if not 1 <= name_length <= 4096:
            raise Invalid()
        name = take(name_length).decode("utf-8")
        parts = components(name)
        if name <= previous:
            raise Invalid()
        previous = name
        total += length
        if total > MAX_BYTES:
            raise Invalid()
        files[name] = take(length)
        directories.update("/".join(parts[:index]) for index in range(1, len(parts)))
    if cursor != len(data) or len(files) + len(directories) > 65536 or directories.intersection(files):
        raise Invalid()
    validate_inventory(files)
    return files, sorted(directories)


def closed_object(pairs):
    result = {}
    for name, value in pairs:
        if name in result:
            raise Invalid()
        result[name] = value
    return result


def validate_inventory(files):
    marker_bytes = files.get(MARKER, b"")
    if len(marker_bytes) > 1024 * 1024:
        raise Invalid()
    marker = json.loads(marker_bytes, object_pairs_hook=closed_object)
    if (not isinstance(marker, dict) or set(marker) != {
            "schema_version", "abi", "package", "runtime", "provenance", "generator", "files"}
            or marker["schema_version"] != "chigiri/generated-crate-v1"
            or type(marker["abi"]) is not int or marker["abi"] not in (7, 8)):
        raise Invalid()
    records = marker["files"]
    if not isinstance(records, list) or len(records) + 1 != len(files):
        raise Invalid()
    paths = []
    for record in records:
        if not isinstance(record, dict) or set(record) != {"path", "bytes", "sha256"}:
            raise Invalid()
        path = record["path"]
        if not isinstance(path, str) or path == MARKER or path not in files:
            raise Invalid()
        if (type(record["bytes"]) is not int or len(files[path]) != record["bytes"]
                or hashlib.sha256(files[path]).hexdigest() != record["sha256"]):
            raise Invalid()
        paths.append(path)
    if paths != sorted(set(files) - {MARKER}):
        raise Invalid()


def extract(path, files, directories):
    path = Path(path)
    components(path.name)
    parent = Directory(path.parent)
    descriptors = {}
    try:
        parent.verify()
        os.mkdir(path.name, mode=0o700, dir_fd=parent.fd)
        descriptors[""] = os.open(path.name, FLAGS, dir_fd=parent.fd)

        def verify():
            parent.verify()
            actual = os.fstat(descriptors[""])
            named = os.stat(path.name, dir_fd=parent.fd, follow_symlinks=False)
            if identity(actual) != identity(named):
                raise Invalid()
            for name, descriptor in descriptors.items():
                if name:
                    ancestor, _, leaf = name.rpartition("/")
                    named = os.stat(leaf, dir_fd=descriptors[ancestor], follow_symlinks=False)
                    if identity(os.fstat(descriptor)) != identity(named):
                        raise Invalid()

        for name in directories:
            verify()
            ancestor, _, leaf = name.rpartition("/")
            os.mkdir(leaf, mode=0o700, dir_fd=descriptors[ancestor])
            descriptors[name] = os.open(leaf, FLAGS, dir_fd=descriptors[ancestor])
        for name, data in files.items():
            verify()
            ancestor, _, leaf = name.rpartition("/")
            descriptor = os.open(leaf, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC,
                                 mode=0o600, dir_fd=descriptors[ancestor])
            with os.fdopen(descriptor, "wb") as output:
                output.write(data)
                output.flush()
                os.fsync(output.fileno())
                named = os.stat(leaf, dir_fd=descriptors[ancestor], follow_symlinks=False)
                if contents(os.fstat(output.fileno())) != contents(named):
                    raise Invalid()
        verify()
        for descriptor in descriptors.values():
            os.fsync(descriptor)
        os.fsync(parent.fd)
    finally:
        for descriptor in reversed(list(descriptors.values())):
            os.close(descriptor)
        parent.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive")
    parser.add_argument(
        "destination",
        help="Absent extraction destination in an existing parent directory",
    )
    args = parser.parse_args()
    try:
        files, directories = decode(read_archive(args.archive))
        extract(args.destination, files, directories)
    except (OSError, ValueError, TypeError, KeyError, RecursionError, Invalid):
        print("Could not restore the generated crate. Check archive integrity, path types, and destination absence.", file=sys.stderr)
        return 1
    print("Recovered the generated crate from its preserved copy.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
