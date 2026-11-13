#!/usr/bin/env python3
"""Inspect Chigiri distribution binaries without rebuilding or mutating them."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import platform
import re
import struct
import sys

from verify_dist_smoke import SmokeError, run_bounded, smoke


TARGET_FILES = {
    "aarch64-apple-darwin": "chigiri",
    "x86_64-apple-darwin": "chigiri",
    "x86_64-unknown-linux-gnu": "chigiri",
    "x86_64-pc-windows-msvc": "chigiri.exe",
}
TIER_ONE = {"aarch64-apple-darwin", "x86_64-unknown-linux-gnu"}
GLIBC_CEILING = (2, 17, 0)


class VerificationError(Exception):
    """A distribution artifact violates the closed build contract."""


def need(data: bytes, offset: int, size: int, description: str) -> None:
    if offset < 0 or size < 0 or offset + size > len(data):
        raise VerificationError(f"truncated {description}")


def verify_macho(data: bytes, cpu: int) -> str:
    need(data, 0, 32, "Mach-O header")
    if data[:4] == b"\xcf\xfa\xed\xfe":
        endian = "<"
    elif data[:4] == b"\xfe\xed\xfa\xcf":
        endian = ">"
    else:
        raise VerificationError("expected a thin 64-bit Mach-O executable")
    actual_cpu = struct.unpack_from(f"{endian}I", data, 4)[0]
    if actual_cpu != cpu:
        raise VerificationError(
            f"unexpected Mach-O CPU 0x{actual_cpu:08x}; expected 0x{cpu:08x}"
        )
    file_type, commands, command_bytes = struct.unpack_from(f"{endian}III", data, 12)
    if file_type != 2 or commands == 0 or command_bytes == 0:
        raise VerificationError("Mach-O image is not an executable")
    need(data, 32, command_bytes, "Mach-O load commands")
    return "Mach-O 64-bit executable"


def elf_sections(data: bytes) -> tuple[str, list[tuple[int, int, int, int]]]:
    need(data, 0, 64, "ELF header")
    if data[:4] != b"\x7fELF" or data[4] != 2:
        raise VerificationError("expected a 64-bit ELF executable")
    if data[5] == 1:
        endian = "<"
    elif data[5] == 2:
        endian = ">"
    else:
        raise VerificationError("ELF file has an unknown byte order")
    machine = struct.unpack_from(f"{endian}H", data, 18)[0]
    if machine != 62:
        raise VerificationError(f"unexpected ELF machine {machine}; expected x86_64")
    file_type = struct.unpack_from(f"{endian}H", data, 16)[0]
    version = struct.unpack_from(f"{endian}I", data, 20)[0]
    entry_point = struct.unpack_from(f"{endian}Q", data, 24)[0]
    program_offset = struct.unpack_from(f"{endian}Q", data, 32)[0]
    program_size = struct.unpack_from(f"{endian}H", data, 54)[0]
    program_count = struct.unpack_from(f"{endian}H", data, 56)[0]
    if file_type not in {2, 3} or version != 1 or entry_point == 0:
        raise VerificationError("ELF image is not an executable")
    if program_size < 56 or program_count == 0:
        raise VerificationError("ELF program table is absent or malformed")
    need(data, program_offset, program_size * program_count, "ELF program table")
    executable_entry = False
    for index in range(program_count):
        offset = program_offset + index * program_size
        kind, flags = struct.unpack_from(f"{endian}II", data, offset)
        virtual_address = struct.unpack_from(f"{endian}Q", data, offset + 16)[0]
        memory_size = struct.unpack_from(f"{endian}Q", data, offset + 40)[0]
        executable_entry |= (
            kind == 1 and flags & 1 != 0
            and virtual_address <= entry_point < virtual_address + memory_size
        )
    if not executable_entry:
        raise VerificationError("ELF entry point is not in an executable load segment")
    section_offset = struct.unpack_from(f"{endian}Q", data, 40)[0]
    entry_size = struct.unpack_from(f"{endian}H", data, 58)[0]
    count = struct.unpack_from(f"{endian}H", data, 60)[0]
    if entry_size < 64 or count == 0:
        raise VerificationError("ELF section table is absent or malformed")
    need(data, section_offset, entry_size * count, "ELF section table")
    sections = []
    for index in range(count):
        offset = section_offset + index * entry_size
        section_type = struct.unpack_from(f"{endian}I", data, offset + 4)[0]
        file_offset = struct.unpack_from(f"{endian}Q", data, offset + 24)[0]
        size = struct.unpack_from(f"{endian}Q", data, offset + 32)[0]
        link = struct.unpack_from(f"{endian}I", data, offset + 40)[0]
        need(data, file_offset, size, "ELF section")
        sections.append((section_type, file_offset, size, link))
    return endian, sections


def dynamic_string(data: bytes, base: int, size: int, offset: int) -> str:
    if offset >= size:
        raise VerificationError("ELF version string offset is out of range")
    end = data.find(b"\0", base + offset, base + size)
    if end < 0:
        raise VerificationError("ELF version string is unterminated")
    try:
        return data[base + offset : end].decode("ascii")
    except UnicodeDecodeError as error:
        raise VerificationError("ELF version string is not ASCII") from error


def glibc_requirements(data: bytes) -> list[tuple[int, ...]]:
    endian, sections = elf_sections(data)
    versions: list[tuple[int, ...]] = []
    for section_type, start, size, link in sections:
        if section_type != 0x6FFFFFFE:
            continue
        if link >= len(sections):
            raise VerificationError("ELF version section has an invalid string-table link")
        _, strings_start, strings_size, _ = sections[link]
        cursor = start
        end = start + size
        visited = 0
        while cursor < end:
            need(data, cursor, 16, "ELF version-need entry")
            _, auxiliary_count, _, auxiliary_offset, next_offset = struct.unpack_from(
                f"{endian}HHIII", data, cursor
            )
            auxiliary = cursor + auxiliary_offset
            for _ in range(auxiliary_count):
                need(data, auxiliary, 16, "ELF version auxiliary entry")
                _, _, _, name_offset, auxiliary_next = struct.unpack_from(
                    f"{endian}IHHII", data, auxiliary
                )
                name = dynamic_string(data, strings_start, strings_size, name_offset)
                match = re.fullmatch(r"GLIBC_(\d+)\.(\d+)(?:\.(\d+))?", name)
                if match:
                    versions.append(tuple(int(part or 0) for part in match.groups()))
                if auxiliary_next == 0:
                    break
                auxiliary += auxiliary_next
            visited += 1
            if visited > 4096:
                raise VerificationError("ELF version section has too many entries")
            if next_offset == 0:
                break
            cursor += next_offset
    return versions


def verify_elf(data: bytes) -> str:
    versions = glibc_requirements(data)
    newest = max(versions, default=None)
    if newest is not None and newest > GLIBC_CEILING:
        rendered = ".".join(str(part) for part in newest)
        raise VerificationError(f"GLIBC_{rendered} exceeds the GLIBC_2.17 ceiling")
    ceiling = "none" if newest is None else ".".join(str(part) for part in newest)
    return f"ELF 64-bit x86_64; newest required GLIBC_{ceiling}"


def verify_pe(data: bytes) -> str:
    need(data, 0, 64, "DOS header")
    if data[:2] != b"MZ":
        raise VerificationError("expected a PE executable with a DOS header")
    pe_offset = struct.unpack_from("<I", data, 60)[0]
    need(data, pe_offset, 136, "PE header")
    if data[pe_offset : pe_offset + 4] != b"PE\0\0":
        raise VerificationError("PE signature is absent")
    machine = struct.unpack_from("<H", data, pe_offset + 4)[0]
    sections = struct.unpack_from("<H", data, pe_offset + 6)[0]
    optional_size, characteristics = struct.unpack_from("<HH", data, pe_offset + 20)
    optional_magic = struct.unpack_from("<H", data, pe_offset + 24)[0]
    entry_point = struct.unpack_from("<I", data, pe_offset + 40)[0]
    image_size = struct.unpack_from("<I", data, pe_offset + 80)[0]
    executable = characteristics & 0x0002 != 0
    dynamic_library = characteristics & 0x2000 != 0
    if (
        machine != 0x8664 or optional_magic != 0x20B or sections == 0
        or optional_size < 112 or not executable or dynamic_library
        or entry_point == 0 or entry_point >= image_size
    ):
        raise VerificationError("expected an x86_64 PE32+ executable")
    return "PE32+ x86_64"


def verify_artifact_bytes(target: str, name: str, data: bytes, executable: bool) -> str:
    if target not in TARGET_FILES:
        raise VerificationError(f"unknown distribution target: {target}")
    if name != TARGET_FILES[target]:
        raise VerificationError(f"unexpected filename for {target}: {name}")
    if not executable:
        raise VerificationError(f"artifact is not executable: {name}")
    if target == "aarch64-apple-darwin":
        return verify_macho(data, 0x0100000C)
    if target == "x86_64-apple-darwin":
        return verify_macho(data, 0x01000007)
    if target == "x86_64-unknown-linux-gnu":
        return verify_elf(data)
    return verify_pe(data)


def verify_artifact(target: str, path: Path) -> str:
    if path.is_symlink() or not path.is_file():
        raise VerificationError(f"artifact is not a regular file: {path}")
    executable = os.name == "nt" or os.access(path, os.X_OK)
    data = path.read_bytes()
    return verify_artifact_bytes(target, path.name, data, executable)


def exact_inventory(dist: Path) -> dict[str, Path]:
    expected = {f"{target}/{name}" for target, name in TARGET_FILES.items()}
    expected_dirs = set(TARGET_FILES)
    if not dist.is_dir():
        raise VerificationError(f"distribution directory is absent: {dist}")
    actual_files: set[str] = set()
    actual_dirs: set[str] = set()
    for path in dist.rglob("*"):
        relative = path.relative_to(dist).as_posix()
        if path.is_symlink():
            raise VerificationError(f"distribution inventory contains a symlink: {relative}")
        if path.is_dir():
            actual_dirs.add(relative)
        elif path.is_file():
            actual_files.add(relative)
        else:
            raise VerificationError(f"distribution inventory contains a special file: {relative}")
    if actual_files != expected or actual_dirs != expected_dirs:
        missing = sorted(expected - actual_files)
        extra = sorted((actual_files - expected) | (actual_dirs - expected_dirs))
        raise VerificationError(f"distribution inventory mismatch; missing={missing}; extra={extra}")
    return {target: dist / target / name for target, name in TARGET_FILES.items()}


def native_tier_one_target() -> str | None:
    system = platform.system()
    machine = platform.machine().lower()
    if system == "Darwin" and machine in {"arm64", "aarch64"}:
        return "aarch64-apple-darwin"
    if system == "Linux" and machine == "x86_64":
        return "x86_64-unknown-linux-gnu"
    if system == "Darwin" and machine == "x86_64":
        return None
    raise VerificationError(f"unsupported verification host: {system} {machine}")


def verify_all(repo_root: Path) -> None:
    native = native_tier_one_target()
    artifacts = exact_inventory(repo_root / "target" / "dist")
    for target, path in artifacts.items():
        description = verify_artifact(target, path)
        print(f"verify-dist: Passed: {target}: {description}")
    for target in sorted(TIER_ONE):
        if target == native:
            smoke(artifacts[target])
        else:
            print(
                f"verify-dist: Not executed: {target} runtime checks; "
                "foreign Tier 1 target for this host"
            )
    for target in sorted(set(TARGET_FILES) - TIER_ONE):
        print(
            f"verify-dist: Not executed: {target} runtime checks; "
            "Tier 2 is build-only"
        )
    print("verify-dist: local verification is not native Tier 1 qualification")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact", nargs=2, metavar=("TARGET", "PATH"))
    arguments = parser.parse_args()
    try:
        if arguments.artifact:
            target, raw_path = arguments.artifact
            description = verify_artifact(target, Path(raw_path))
            print(f"verify-dist: Passed: {target}: {description}")
        else:
            repo_root = Path(__file__).resolve().parent.parent
            verify_all(repo_root)
    except (OSError, SmokeError, VerificationError) as error:
        print(f"verify-dist: Failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
