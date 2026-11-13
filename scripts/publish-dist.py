#!/usr/bin/env python3
"""Publish verified bytes through retained no-follow directory descriptors."""

from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import secrets
import stat
import subprocess
import sys


def load_verifier():
    specification = importlib.util.spec_from_file_location(
        "chigiri_publish_verifier", Path(__file__).with_name("verify-dist.py")
    )
    assert specification and specification.loader
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    return module


VERIFIER = load_verifier()
DIRECTORY_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW


def same_identity(left: os.stat_result, right: os.stat_result) -> bool:
    return (
        left.st_dev == right.st_dev
        and left.st_ino == right.st_ino
        and stat.S_IFMT(left.st_mode) == stat.S_IFMT(right.st_mode)
    )


def verify_chain(descriptors: list[int], names: list[str]) -> None:
    for parent, child, name in zip(descriptors, descriptors[1:], names):
        retained = os.fstat(child)
        current = os.stat(name, dir_fd=parent, follow_symlinks=False)
        if not stat.S_ISDIR(current.st_mode) or not same_identity(retained, current):
            raise ValueError("publication directory ancestry changed")


def close_chain(descriptors: list[int]) -> None:
    for descriptor in reversed(descriptors):
        os.close(descriptor)


def open_chain(path: Path) -> tuple[list[int], list[str]]:
    if not path.is_absolute():
        raise ValueError("publication paths must be absolute")
    descriptors = [os.open("/", DIRECTORY_FLAGS)]
    names: list[str] = []
    try:
        for part in path.parts[1:]:
            if part in {"", ".", ".."}:
                raise ValueError("publication path contains an invalid component")
            descriptors.append(os.open(part, DIRECTORY_FLAGS, dir_fd=descriptors[-1]))
            names.append(part)
        verify_chain(descriptors, names)
        return descriptors, names
    except Exception:
        close_chain(descriptors)
        raise


def read_source(parent: int, name: str) -> tuple[bytes, os.stat_result]:
    descriptor = os.open(name, os.O_RDONLY | os.O_NOFOLLOW, dir_fd=parent)
    try:
        identity = os.fstat(descriptor)
        if not stat.S_ISREG(identity.st_mode):
            raise ValueError("publication source is not a regular file")
        with os.fdopen(os.dup(descriptor), "rb") as source:
            return source.read(), identity
    finally:
        os.close(descriptor)


def write_all(descriptor: int, data: bytes) -> None:
    offset = 0
    while offset < len(data):
        offset += os.write(descriptor, data[offset:])


def remove_source(parent: int, name: str, expected: os.stat_result) -> None:
    claim_name = f".source-claim-{secrets.token_hex(16)}"
    os.mkdir(claim_name, 0o700, dir_fd=parent)
    claim = os.open(claim_name, DIRECTORY_FLAGS, dir_fd=parent)
    moved = False
    try:
        os.rename(name, "entry", src_dir_fd=parent, dst_dir_fd=claim)
        moved = True
        found = os.stat("entry", dir_fd=claim, follow_symlinks=False)
        if not same_identity(expected, found):
            raise ValueError("publication source identity changed")
        os.unlink("entry", dir_fd=claim)
        moved = False
    finally:
        os.close(claim)
        if not moved:
            os.rmdir(claim_name, dir_fd=parent)


def run_exchange_helper(
    helper: Path,
    action: str,
    private: Path,
    slot: Path,
    name: str,
    candidate: os.stat_result,
    previous: os.stat_result | None,
) -> None:
    arguments = [
        str(helper),
        action,
        str(private),
        str(slot),
        name,
        str(candidate.st_dev),
        str(candidate.st_ino),
    ]
    if previous is not None:
        arguments.extend((str(previous.st_dev), str(previous.st_ino)))
    result = subprocess.run(
        arguments,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        detail = result.stdout.strip() or f"helper exited {result.returncode}"
        raise ValueError(detail)


def publish(
    source: Path,
    slot: Path,
    name: str,
    target: str,
    helper: Path,
    exchange_runner=run_exchange_helper,
) -> str:
    if not name or name in {".", ".."} or "/" in name:
        raise ValueError("artifact name must be one portable path component")
    source_chain, source_names = open_chain(source.parent)
    slot_chain, slot_names = open_chain(slot)
    private_name = f".publish-{secrets.token_hex(16)}"
    private_descriptor: int | None = None
    staged_descriptor: int | None = None
    staged_identity: os.stat_result | None = None
    stage_at_private = False
    private_created = False
    try:
        data, source_identity = read_source(source_chain[-1], source.name)
        executable = os.name == "nt" or source_identity.st_mode & 0o111 != 0
        description = VERIFIER.verify_artifact_bytes(target, name, data, executable)

        os.mkdir(private_name, 0o700, dir_fd=slot_chain[-1])
        private_created = True
        private_descriptor = os.open(private_name, DIRECTORY_FLAGS, dir_fd=slot_chain[-1])
        staged_descriptor = os.open(
            "candidate",
            os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
            0o700,
            dir_fd=private_descriptor,
        )
        stage_at_private = True
        staged_identity = os.fstat(staged_descriptor)
        write_all(staged_descriptor, data)
        os.fchmod(staged_descriptor, 0o755)
        os.fsync(staged_descriptor)
        staged_identity = os.fstat(staged_descriptor)
        verify_chain(source_chain, source_names)
        verify_chain(slot_chain, slot_names)
        remove_source(source_chain[-1], source.name, source_identity)
        try:
            current = os.stat(name, dir_fd=slot_chain[-1], follow_symlinks=False)
        except FileNotFoundError:
            current = None
        if current is not None and not stat.S_ISREG(current.st_mode):
            raise ValueError("existing distribution artifact is not a regular file")
        verify_chain(slot_chain, slot_names)
        assert private_descriptor is not None
        private_path = slot / private_name
        if current is None:
            exchange_runner(
                helper, "install", private_path, slot, name, staged_identity, None
            )
            stage_at_private = False
        else:
            exchange_runner(
                helper, "exchange", private_path, slot, name, staged_identity, current
            )
            stage_at_private = False
            try:
                os.unlink("candidate", dir_fd=private_descriptor)
            except OSError:
                pass
        try:
            os.rmdir(private_name, dir_fd=slot_chain[-1])
            private_created = False
        except OSError:
            pass
        return description
    finally:
        if staged_descriptor is not None:
            if stage_at_private and staged_identity is not None and private_descriptor is not None:
                try:
                    current = os.stat(
                        "candidate", dir_fd=private_descriptor, follow_symlinks=False
                    )
                    if same_identity(staged_identity, current):
                        os.unlink("candidate", dir_fd=private_descriptor)
                except OSError:
                    pass
            os.close(staged_descriptor)
        if private_descriptor is not None:
            os.close(private_descriptor)
        if private_created:
            try:
                os.rmdir(private_name, dir_fd=slot_chain[-1])
            except OSError:
                pass
        close_chain(slot_chain)
        close_chain(source_chain)


def main() -> int:
    if len(sys.argv) != 6:
        print(
            "usage: publish-dist.py <source> <slot-directory> <name> <target> "
            "<atomic-helper>",
            file=sys.stderr,
        )
        return 2
    try:
        description = publish(
            Path(sys.argv[1]),
            Path(sys.argv[2]),
            sys.argv[3],
            sys.argv[4],
            Path(sys.argv[5]),
        )
    except (OSError, ValueError, VERIFIER.VerificationError) as error:
        print(f"distribution publication failed: {error}", file=sys.stderr)
        return 1
    print(f"publish-dist: Passed: {sys.argv[4]}: {description}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
