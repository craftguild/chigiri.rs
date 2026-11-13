"""Verify performance environment, source, and timestamps as a closed contract."""

from __future__ import annotations

import hashlib
import json
import platform
import re
import subprocess
import time
from pathlib import Path
from typing import Any

MIN_RECORDED_UNIX_SECONDS = 1_577_836_800
MAX_FUTURE_SECONDS = 300
SOURCE_KEYS = {
    "revision", "revisionUnixSeconds", "dirty", "binarySha256", "toolchain"
}
TOOLCHAIN_KEYS = {"version", "cargo", "rustc"}
HARDWARE_KEYS = {"os", "osVersion", "arch", "logicalCpus", "descriptor"}


class EvidenceError(RuntimeError):
    """Raised when evidence collection or semantic validation fails."""


def strict_json_loads(document: str | bytes) -> Any:
    """Decode JSON while rejecting duplicate keys and non-standard numbers."""

    def object_pairs(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        value: dict[str, Any] = {}
        for name, item in pairs:
            if name in value:
                raise EvidenceError(f"duplicate JSON field: {name}")
            value[name] = item
        return value

    def invalid_constant(value: str) -> None:
        raise EvidenceError(f"non-standard JSON number: {value}")

    return json.loads(
        document, object_pairs_hook=object_pairs, parse_constant=invalid_constant
    )


def command_output(arguments: list[str]) -> str:
    result = subprocess.run(arguments, check=False, capture_output=True, text=True)
    value = result.stdout.strip()
    if result.returncode != 0 or not value:
        detail = result.stderr.strip() or "empty output"
        raise EvidenceError(f"command failed: {' '.join(arguments)}: {detail}")
    return value


def optional_command_output(arguments: list[str]) -> str | None:
    try:
        result = subprocess.run(arguments, check=False, capture_output=True, text=True)
    except FileNotFoundError:
        return None
    value = result.stdout.strip()
    return value if result.returncode == 0 and value else None


def command_output_allow_empty(arguments: list[str]) -> str:
    result = subprocess.run(arguments, check=False, capture_output=True, text=True)
    if result.returncode != 0:
        detail = result.stderr.strip() or "empty error"
        raise EvidenceError(f"command failed: {' '.join(arguments)}: {detail}")
    return result.stdout.strip()


def require_immutable_toolchain(version: object) -> str:
    pattern = r"[1-9][0-9]*\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
    if not isinstance(version, str) or re.fullmatch(pattern, version) is None:
        raise EvidenceError("performance toolchain must be an immutable Rust version")
    return version


def toolchain_identity(version: str) -> dict[str, str]:
    version = require_immutable_toolchain(version)
    cargo_path = command_output(["rustup", "which", "--toolchain", version, "cargo"])
    rustc_path = command_output(["rustup", "which", "--toolchain", version, "rustc"])
    cargo = command_output([cargo_path, "--version", "--verbose"])
    rustc = command_output([rustc_path, "--version", "--verbose"])
    require_toolchain({"version": version, "cargo": cargo, "rustc": rustc})
    return {"version": version, "cargo": cargo, "rustc": rustc}


def hardware_identity() -> dict[str, object]:
    system = platform.system().lower()
    os_name = "macos" if system == "darwin" else system
    os_version = optional_command_output(["sw_vers", "-productVersion"])
    if os_version is None:
        os_version = command_output(["uname", "-r"])
    machine = platform.machine().lower()
    arch = "aarch64" if machine == "arm64" else machine
    descriptor = optional_command_output(["sysctl", "-n", "machdep.cpu.brand_string"])
    if descriptor is None:
        descriptor = stable_lscpu_descriptor(
            command_output(["env", "LC_ALL=C", "lscpu"])
        )
    if descriptor.lower() == arch:
        raise EvidenceError("hardware descriptor could not distinguish the CPU model")
    logical_cpus = _logical_cpus()
    return {
        "os": os_name,
        "osVersion": os_version,
        "arch": arch,
        "logicalCpus": logical_cpus,
        "descriptor": descriptor,
    }


def stable_lscpu_descriptor(output: str) -> str:
    labels = ("Vendor ID", "Model name", "CPU family", "Model", "Stepping")
    values: dict[str, str] = {}
    for line in output.splitlines():
        name, separator, value = line.partition(":")
        if separator and name.strip() in labels and value.strip():
            values.setdefault(name.strip(), value.strip())
    if "Model name" not in values:
        raise EvidenceError("lscpu did not report a stable CPU model name")
    return ";".join(f"{label}={values[label]}" for label in labels if label in values)


def _logical_cpus() -> int:
    value = optional_command_output(["sysctl", "-n", "hw.logicalcpu"])
    if value is None:
        value = command_output(["getconf", "_NPROCESSORS_ONLN"])
    try:
        logical_cpus = int(value)
    except ValueError as error:
        raise EvidenceError("logical CPU command returned a non-integer") from error
    if logical_cpus <= 0 or str(logical_cpus) != value:
        raise EvidenceError("logical CPU command returned an invalid count")
    return logical_cpus


def binary_sha256(binary: Path) -> str:
    digest = hashlib.sha256()
    with binary.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def source_identity(binary: Path, version: str) -> dict[str, Any]:
    revision = command_output(["git", "rev-parse", "HEAD"])
    return {
        "revision": revision,
        "revisionUnixSeconds": revision_unix_seconds(revision),
        "dirty": bool(command_output_allow_empty(["git", "status", "--porcelain"])),
        "binarySha256": binary_sha256(binary.resolve()),
        "toolchain": toolchain_identity(version),
    }


def require_identity(evidence: dict[str, Any], binary: Path, version: str, *, require_clean: bool = True) -> None:
    expected = source_identity(binary, version)
    actual = evidence.get("source")
    if actual != expected:
        raise EvidenceError("evidence revision/toolchain/binary digest does not match candidate")
    require_source_identity(actual, require_clean=require_clean)


def require_source_identity(source: object, *, require_clean: bool = True) -> None:
    require_exact_keys(source, SOURCE_KEYS, "source identity")
    assert isinstance(source, dict)
    if type(source.get("dirty")) is not bool:
        raise EvidenceError("source dirty state must be boolean")
    if require_clean and source["dirty"]:
        raise EvidenceError("clean source identity is required")
    if not _hex_string(source.get("revision"), 40) or not _hex_string(
        source.get("binarySha256"), 64
    ):
        raise EvidenceError("source revision or binary digest is malformed")
    recorded_revision = positive_integer(
        source.get("revisionUnixSeconds"), "source revision timestamp"
    )
    if recorded_revision != revision_unix_seconds(source["revision"]):
        raise EvidenceError("source revision timestamp does not match git history")
    require_toolchain(source.get("toolchain"))


def require_toolchain(value: object) -> None:
    require_exact_keys(value, TOOLCHAIN_KEYS, "toolchain identity")
    assert isinstance(value, dict)
    version = require_immutable_toolchain(value.get("version"))
    cargo = require_nonempty_string(value.get("cargo"), "Cargo identity")
    rustc = require_nonempty_string(value.get("rustc"), "Rustc identity")
    if f"release: {version}" not in cargo.splitlines():
        raise EvidenceError("Cargo identity does not match immutable toolchain version")
    if f"release: {version}" not in rustc.splitlines():
        raise EvidenceError("Rustc identity does not match immutable toolchain version")


def require_hardware(value: object, *, current: bool) -> None:
    require_exact_keys(value, HARDWARE_KEYS, "hardware identity")
    assert isinstance(value, dict)
    for name in ("os", "osVersion", "arch", "descriptor"):
        require_nonempty_string(value.get(name), f"hardware {name}")
    logical_cpus = value.get("logicalCpus")
    if isinstance(logical_cpus, bool) or not isinstance(logical_cpus, int) or logical_cpus <= 0:
        raise EvidenceError("hardware logical CPU count must be positive")
    if current and value != hardware_identity():
        raise EvidenceError("evidence hardware does not match the current measurement host")


def require_recorded_time(value: object, source: object | None = None) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise EvidenceError("recordedAtUnixSeconds must be an integer")
    if value < MIN_RECORDED_UNIX_SECONDS or value > int(time.time()) + MAX_FUTURE_SECONDS:
        raise EvidenceError("recordedAtUnixSeconds is outside the auditable range")
    if source is not None:
        require_exact_keys(source, SOURCE_KEYS, "source identity")
        assert isinstance(source, dict)
        revision_time = positive_integer(
            source.get("revisionUnixSeconds"), "source revision timestamp"
        )
        if value < revision_time:
            raise EvidenceError("recordedAtUnixSeconds predates the source revision")
    return value


def revision_unix_seconds(revision: str) -> int:
    if not _hex_string(revision, 40):
        raise EvidenceError("source revision is not a 40-character hexadecimal commit")
    value = command_output(["git", "show", "-s", "--format=%ct", revision])
    try:
        parsed = int(value)
    except ValueError as error:
        raise EvidenceError("source revision timestamp is not an integer") from error
    if parsed <= 0 or str(parsed) != value:
        raise EvidenceError("source revision timestamp is not canonical")
    return parsed


def require_exact_keys(value: object, expected: set[str], label: str) -> None:
    if not isinstance(value, dict) or set(value) != expected:
        raise EvidenceError(f"{label} fields do not match the closed contract")


def require_nonempty_string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise EvidenceError(f"{label} must be a non-empty string")
    return value


def positive_integer(value: object, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
        raise EvidenceError(f"{label} must be a positive integer")
    return value


def require_command(
    value: object, program: str, mode: str, ordered_flags: tuple[str, ...]
) -> dict[str, str]:
    if not isinstance(value, list) or not all(isinstance(item, str) for item in value):
        raise EvidenceError("evidence command must be a string list")
    if len(value) != 2 + len(ordered_flags) * 2:
        raise EvidenceError("evidence command argument count does not match")
    if Path(value[0]).name != program or value[1] != mode:
        raise EvidenceError("evidence command program or mode does not match")
    arguments: dict[str, str] = {}
    for index, flag in enumerate(ordered_flags):
        position = 2 + index * 2
        if value[position] != flag or not value[position + 1]:
            raise EvidenceError("evidence command flags do not match the closed order")
        arguments[flag] = value[position + 1]
    return arguments


def _hex_string(value: object, length: int) -> bool:
    return (
        isinstance(value, str)
        and len(value) == length
        and all(character in "0123456789abcdef" for character in value)
    )
