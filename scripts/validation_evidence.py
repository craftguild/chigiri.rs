"""Build a source snapshot and validate closed command-result evidence."""

import hashlib
import math
import re
from pathlib import Path

from implementation_rule_io import read_file
from validation_profiles import COMPLETION_SCHEMA, PROFILES
from validation_sources import source_paths


def source_snapshot(repository: Path):
    repository = repository.resolve()
    digest = hashlib.sha256()
    records = []

    # All authored inputs participate, including artwork and license files
    # without a conventional extension. Local state is excluded by source_paths.
    paths = source_paths(repository)
    for path in sorted(paths):
        name = path.relative_to(repository).as_posix()
        encoded = name.encode("utf-8")
        data = read_file(path, 64 * 1024 * 1024)
        digest.update(len(encoded).to_bytes(8, "big"))
        digest.update(encoded)
        digest.update(len(data).to_bytes(8, "big"))
        digest.update(data)
        records.append({"path": name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
    return {"sha256": digest.hexdigest(), "files": records}


def validate_result(step, result):
    expected = {"argv", "elapsedSeconds", "exitCode", "label", "timedOut", "toolchain"}
    if not isinstance(result, dict) or set(result) != expected:
        raise ValueError(f"The command evidence fields are invalid: {step.label}")
    if result["argv"] != list(step.argv) or result["label"] != step.label or result["toolchain"] != "product":
        raise ValueError(f"Command evidence does not match the required command: {step.label}")
    elapsed = result["elapsedSeconds"]
    if type(elapsed) not in (int, float) or not math.isfinite(elapsed) or not 0 <= elapsed <= step.seconds:
        raise ValueError(f"The command deadline evidence is invalid: {step.label}")
    if type(result["exitCode"]) is not int or result["exitCode"] != 0 or result["timedOut"] is not False:
        raise ValueError(f"The required command did not succeed: {step.label}")


def validate_completion(steps, results):
    if not steps or len({step.label for step in steps}) != len(steps) or len(results) != len(steps):
        raise ValueError("Command evidence is missing, duplicated, or out of order.")
    for step, result in zip(steps, results):
        validate_result(step, result)


def validate_report(profile, steps, report):
    expected = {"schema_version", "profile", "source_sha256", "commands"}
    if (profile not in PROFILES or not isinstance(report, dict)
            or set(report) != expected
            or report["schema_version"] != COMPLETION_SCHEMA
            or report["profile"] != profile
            or not isinstance(report["source_sha256"], str)
            or re.fullmatch(r"[0-9a-f]{64}", report["source_sha256"]) is None
            or not isinstance(report["commands"], list)):
        raise ValueError("The completion evidence does not match the requested profile.")
    validate_completion(steps, report["commands"])
