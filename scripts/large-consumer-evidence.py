#!/usr/bin/env python3
"""Create and validate closed JSON evidence for large-consumer conformance.

Receiving trace: generated-size-file-line-metrics.
"""

import argparse
import json
import os
import tempfile
from pathlib import Path


IDS = [f"LCON-{number:02}" for number in range(1, 17) if number != 4]
TOP_KEYS = {
    "acceptanceIds",
    "commands",
    "diagnostics",
    "formatVersion",
    "metrics",
    "providers",
    "scope",
    "shape",
    "source",
}
COMMAND_KEYS = {"argv", "elapsedSeconds", "exitCode", "label", "timedOut", "toolchain"}
SHAPE_KEYS = {"customScalars", "enums", "fields", "inputObjects", "objects"}


def load(path: Path) -> object:
    with path.open(encoding="utf-8") as source:
        return json.load(source)


def source_files(repository: Path) -> list[Path]:
    def failure(error):
        raise error

    paths = []
    for directory, children, files in os.walk(repository, followlinks=False, onerror=failure):
        children[:] = [name for name in children if name not in {"target", ".git", "vendor"}]
        paths.extend(Path(directory) / name for name in files if name.endswith((".rs", ".sh")))
    return paths


def line_count(path: Path) -> int:
    from implementation_rule_io import read_file
    data = read_file(path, 64 * 1024 * 1024)
    return data.count(b"\n") + int(bool(data) and not data.endswith(b"\n"))


def metrics(repository: Path, generated: Path, application: Path) -> dict:
    generated_files = sorted(generated.rglob("*.rs"))
    application_files = list(application.rglob("*.rs"))
    # Unlike the checker, include the generated directory when collecting evidence owners.
    from generated_inventory import owned_rust
    owned = owned_rust(repository)
    repository_files = [path for path in source_files(repository)
                        if path.relative_to(repository).as_posix() not in owned
                        and not {"target", ".git", "vendor"}.intersection(path.relative_to(repository).parts)]
    return {
        "applicationMaxLines": max(map(line_count, application_files), default=0),
        "generatedBytes": sum(path.stat().st_size for path in generated_files),
        "generatedFiles": len(generated_files),
        "generatedMaxLines": max(map(line_count, generated_files), default=0),
        "handwrittenMaxLines": max(map(line_count, repository_files), default=0),
        "handwrittenFiles": len(repository_files),
        "generatedExcludedFiles": len(owned),
    }


def write_atomic(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=path.parent, delete=False) as temporary:
        json.dump(value, temporary, ensure_ascii=False, indent=2, sort_keys=True)
        temporary.write("\n")
        temporary_path = temporary.name
    os.replace(temporary_path, path)


def create(args: argparse.Namespace) -> None:
    commands = [load(path) for path in args.command]
    diagnostics = [load(path) for path in args.diagnostic]
    document = {
        "acceptanceIds": IDS,
        "commands": commands,
        "diagnostics": {
            "negative": diagnostics,
            "positiveExitCodes": {"1.95.0": args.positive},
        },
        "formatVersion": 5,
        "metrics": metrics(args.repository, args.generated, args.application),
        "providers": {
            "negativeSuite": args.provider_negative,
            "officialSuite": args.provider_official,
            "thirdPartyMinimal": args.provider_third_party,
        },
        "scope": {"formalPerformanceGate": "Not Passed", "externalProductDependency": False},
        "shape": load(args.shape),
        "source": {"dirty": args.dirty == "true", "revision": args.revision},
    }
    validate_document(document)
    write_atomic(args.output, document)


def exact_keys(value: dict, expected: set[str], location: str) -> None:
    if set(value) != expected:
        raise ValueError(f"{location} keys are not a closed set: {sorted(value)}")


def validate_document(document: object) -> None:
    if not isinstance(document, dict):
        raise ValueError("The evidence root must be an object.")
    exact_keys(document, TOP_KEYS, "root")
    if document["formatVersion"] != 5 or document["acceptanceIds"] != IDS:
        raise ValueError("The formatVersion or acceptance ID set is incorrect.")
    exact_keys(document["source"], {"dirty", "revision"}, "source")
    revision = document["source"]["revision"]
    if not isinstance(revision, str) or len(revision) != 40:
        raise ValueError("The source revision must be a 40-character commit hash.")
    exact_keys(document["shape"], SHAPE_KEYS, "shape")
    minimums = {"objects": 70, "fields": 750, "inputObjects": 19, "enums": 65, "customScalars": 5}
    for name, minimum in minimums.items():
        if document["shape"].get(name, -1) < minimum:
            raise ValueError(f"Shape count is below the required minimum: {name}")
    exact_keys(
        document["metrics"],
        {"applicationMaxLines", "generatedBytes", "generatedFiles", "generatedMaxLines", "handwrittenMaxLines", "handwrittenFiles", "generatedExcludedFiles"},
        "metrics",
    )
    measured = document["metrics"]
    if any(type(value) is not int or value < 0 for value in measured.values()):
        raise ValueError("Metrics need nonnegative integers.")
    if (measured["generatedBytes"] > 64 * 1024 * 1024
            or not 0 < measured["generatedFiles"] <= 65536
            or measured["handwrittenFiles"] <= 0 or measured["generatedExcludedFiles"] <= 0):
        raise ValueError("Evidence exceeds the size or file-count budget.")
    for name in ("applicationMaxLines", "handwrittenMaxLines"):
        if measured[name] > 400:
            raise ValueError(f"Line budget violation: {name}")
    validate_diagnostics(document["diagnostics"])
    validate_providers(document["providers"])
    if document["scope"] != {"formalPerformanceGate": "Not Passed", "externalProductDependency": False}:
        raise ValueError("Scope boundary does not match the required contract.")
    validate_commands(document["commands"])


def validate_diagnostics(value: dict) -> None:
    exact_keys(value, {"negative", "positiveExitCodes"}, "diagnostics")
    if value["positiveExitCodes"] != {"1.95.0": 0} or len(value["negative"]) != 6:
        raise ValueError("Doctor positive/negative evidence is incomplete.")
    expected_cases = {
        "missing-group",
        "missing-field",
        "wrong-output",
        "missing-root",
        "generated-drift",
        "abi-mismatch",
    }
    actual_cases = set()
    for item in value["negative"]:
        exact_keys(
            item,
            {"case", "exitCode", "chigiriCode", "rustcCode", "toolchain"},
            "diagnostic case",
        )
        if item["exitCode"] != 1:
            raise ValueError("Negative doctor evidence must report exit status 1.")
        actual_cases.add((item["case"], item["toolchain"]))
    expected_pairs = {
        (case, toolchain)
        for case in expected_cases
        for toolchain in ("1.95.0",)
    }
    if actual_cases != expected_pairs:
        raise ValueError("Negative doctor case set does not match the contract.")


def validate_providers(value: dict) -> None:
    exact_keys(value, {"negativeSuite", "officialSuite", "thirdPartyMinimal"}, "providers")
    if set(value.values()) != {True}:
        raise ValueError("Provider suite did not pass completely.")


def validate_commands(commands: object) -> None:
    if not isinstance(commands, list) or not commands:
        raise ValueError("Command evidence is missing.")
    labels = []
    for command in commands:
        exact_keys(command, COMMAND_KEYS, "command")
        if not isinstance(command["argv"], list) or not command["argv"]:
            raise ValueError("Command evidence must include a nonempty argv list.")
        if command["elapsedSeconds"] < 0 or command["elapsedSeconds"] > 180:
            raise ValueError("Command exceeded the 180-second elapsed-time threshold.")
        expected_exit = 1 if command["label"].startswith("doctor-negative-") else 0
        if command["exitCode"] != expected_exit or command["timedOut"]:
            raise ValueError(f"The command result is incorrect: {command['label']}")
        labels.append(command["label"])
    required = {
        "codegen",
        "drift",
        "sdk-fmt",
        "doctor-positive",
        "provider-negative",
        "provider-official",
        "provider-third-party",
    }
    required.update(f"doctor-negative-{name}" for name in (
        "missing-group", "missing-field", "wrong-output",
        "missing-root", "generated-drift", "abi-mismatch",
    ))
    if not required.issubset(labels):
        raise ValueError("Required large-consumer command evidence is missing.")
    for toolchain in ("1.95.0",):
        if not any(
            command["toolchain"] == toolchain and command["label"] == "doctor-positive"
            for command in commands
        ):
            raise ValueError(f"Missing positive doctor evidence for toolchain: {toolchain}")
        for case in (
            "missing-group",
            "missing-field",
                "wrong-output",
            "missing-root",
            "generated-drift",
            "abi-mismatch",
        ):
            if not any(
                command["toolchain"] == toolchain
                and command["label"] == f"doctor-negative-{case}"
                for command in commands
            ):
                raise ValueError(f"Negative doctor command failure: {toolchain} {case}")
        for step in ("fmt", "check", "test", "clippy", "rustdoc"):
            if not any(
                command["toolchain"] == toolchain and command["label"] == step
                for command in commands
            ):
                raise ValueError(f"The toolchain command is missing: {toolchain} {step}")


def main() -> None:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command_name", required=True)
    create_parser = subparsers.add_parser("create")
    create_parser.add_argument("--output", type=Path, required=True)
    create_parser.add_argument("--repository", type=Path, required=True)
    create_parser.add_argument("--generated", type=Path, required=True)
    create_parser.add_argument("--application", type=Path, required=True)
    create_parser.add_argument("--revision", required=True)
    create_parser.add_argument("--dirty", choices=("true", "false"), required=True)
    create_parser.add_argument("--shape", type=Path, required=True)
    create_parser.add_argument("--positive", type=int, required=True)
    create_parser.add_argument("--diagnostic", type=Path, action="append", default=[])
    create_parser.add_argument("--command", type=Path, action="append", default=[])
    create_parser.add_argument("--provider-third-party", action="store_true")
    create_parser.add_argument("--provider-negative", action="store_true")
    create_parser.add_argument("--provider-official", action="store_true")
    validate_parser = subparsers.add_parser("validate")
    validate_parser.add_argument("evidence", type=Path)
    args = parser.parse_args()
    if args.command_name == "create":
        create(args)
    else:
        validate_document(load(args.evidence))


if __name__ == "__main__":
    main()
