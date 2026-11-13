#!/usr/bin/env python3
"""Verify canonical large-consumer shape, mutation, and diagnostic contracts.

Receiving trace: shape-minimums, shape-markers-and-five-command-compile,
missing-group-field-root-diagnostics,
wrong-output-diagnostic, generated-drift-and-abi-mismatch-diagnostics.
"""

import argparse
import json
import os
import re
import tempfile
from pathlib import Path
from typing import Optional


CASES = {
    "missing-group": ("E0599", "SF4018", "ProductObject001", "with_product_object001"),
    "missing-field": ("E0046", "SF4018", "ProductObject001.name", "name"),
    "wrong-output": ("E0271", "SF4019", "ProductObject001.id", "chigiri::Id"),
    "missing-root": ("E0046", "SF4018", "Query.node", "node"),
    "generated-drift": (None, "SF4016", "generated crate", "chigiri rust codegen"),
    "abi-mismatch": (None, "SF4016", "generated crate", "chigiri rust codegen"),
}


def replace_once(path: Path, old: str, new: str) -> None:
    source = path.read_text(encoding="utf-8")
    if source.count(old) != 1:
        raise SystemExit(f"Expected exactly one mutation target in {path}: {old!r}")
    path.write_text(source.replace(old, new, 1), encoding="utf-8")


def mutate(root: Path, case: str) -> None:
    assembly = root / "consumer/src/assembly.rs"
    query = root / "consumer/src/query.rs"
    generated = root / "generated/src/lib.rs"
    if case == "missing-group":
        replace_once(
            assembly,
            "        .with_product_object001(evaluated(evaluations, 3))\n",
            "",
        )
    elif case == "missing-field":
        path = root / "consumer/src/products_00.rs"
        source = path.read_text(encoding="utf-8")
        start = source.index("    async fn name<'a>(")
        end = source.index("    async fn related<'a>(", start)
        replace_once(path, source[start:end], "")
    elif case == "wrong-output":
        path = root / "consumer/src/products_00.rs"
        replace_once(path, "chigiri::FieldResult<chigiri::Id>", "chigiri::FieldResult<String>")
        replace_once(path, 'Ok("synthetic".into())', 'Ok("synthetic".to_owned())')
    elif case == "missing-root":
        source = query.read_text(encoding="utf-8")
        start = source.index("    async fn node<'a>(")
        end = source.index("    async fn search<'a>(", start)
        query.write_text(source[:start] + source[end:], encoding="utf-8")
    elif case == "generated-drift":
        generated.write_text(generated.read_text(encoding="utf-8") + " ", encoding="utf-8")
    elif case == "abi-mismatch":
        replace_once(
            generated,
            "chigiri::GENERATED_ABI_VERSION == 8,",
            "chigiri::GENERATED_ABI_VERSION == 6,",
        )
    else:
        raise SystemExit(f"unknown mutation: {case}")


def graphql_blocks(source: str, keyword: str) -> list[str]:
    pattern = rf"(?ms)^{keyword}\s+[A-Za-z_][A-Za-z0-9_]*(?:\s+implements\s+[^{{]+)?\s*\{{(.*?)^\}}"
    return re.findall(pattern, source)


def shape(schema_path: Path, consumer_root: Path, output: Optional[Path]) -> None:
    source = schema_path.read_text(encoding="utf-8")
    objects = graphql_blocks(source, "type")
    counts = {
        "objects": len(objects),
        "fields": sum(
            1
            for block in objects
            for line in block.splitlines()
            if re.match(r"^\s+[A-Za-z_][A-Za-z0-9_]*(?:\([^)]*\))?\s*:", line)
        ),
        "inputObjects": len(graphql_blocks(source, "input")),
        "enums": len(re.findall(r"(?m)^enum\s+", source)),
        "customScalars": len(re.findall(r"(?m)^scalar\s+", source)),
    }
    minimums = {
        "objects": 70,
        "fields": 750,
        "inputObjects": 19,
        "enums": 65,
        "customScalars": 5,
    }
    for name, minimum in minimums.items():
        if counts[name] < minimum:
            raise SystemExit(f"Large-consumer {name} count is below {minimum}: {counts[name]}.")
    required = {
        "schema": ["mutation: Mutation", "interface Node", "union SearchResult"],
        "assembly.rs": [".with_product_object000(product000)", "Resolvers::builder(std::convert::identity::<chigiri::FieldError>)", ".with_mutation("],
        "products_00.rs": ["ProductObject001Resolver", "ProductObject001RelatedArgs"],
        "query.rs": ["QueryResolver", "MutationResolver", "async fn", "FieldResult"],
    }
    for name, markers in required.items():
        text = source if name == "schema" else (consumer_root / "src" / name).read_text(encoding="utf-8")
        for marker in markers:
            if marker not in text:
                raise SystemExit(f"Missing large-consumer shape marker {marker!r} in {name}.")
    write_json(output, counts)


def closed_object(pairs: list[tuple[str, object]]) -> dict:
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError("duplicate diagnostic key")
        value[key] = item
    return value


def read_diagnostics(path: Path) -> list[dict]:
    records = []
    for line in path.read_text(encoding="utf-8").splitlines():
        try:
            value = json.loads(line, object_pairs_hook=closed_object)
        except json.JSONDecodeError:
            continue
        except ValueError as error:
            raise SystemExit("The diagnostic report contains duplicate keys.") from error
        records.append(value)
    if len(records) != 1 or not isinstance(records[0], dict) or set(records[0]) != {"diagnostics", "schema_version", "truncated"}:
        raise SystemExit("The diagnostic report must contain exactly one closed JSON object.")
    report = records[0]
    if report["schema_version"] != "chigiri/diagnostics-v2" or report["truncated"] is not False:
        raise SystemExit("Diagnostic report schema version or truncation flag is invalid.")
    if not isinstance(report["diagnostics"], list):
        raise SystemExit("`diagnostics` must be an array.")
    return report["diagnostics"]


def positive(log: Path) -> None:
    records = read_diagnostics(log)
    cargo = [item for item in records if item.get("code") == "SF4017"]
    if len(cargo) != 1 or cargo[0].get("origin") != "cargo" or cargo[0].get("severity") != "note":
        raise SystemExit("The successful doctor run emitted no real Cargo note.")
    if any(item.get("severity") in {"warning", "error"} for item in records):
        raise SystemExit("The successful doctor run emitted a warning or error.")
    serialized = json.dumps(cargo[0], ensure_ascii=False)
    for marker in (
        "Ran a real consumer cargo check.",
        "--locked",
        "--all-targets",
        "chigiri_vnext_large_consumer",
    ):
        if marker not in serialized:
            raise SystemExit(f"Positive doctor output is missing marker: {marker}")


def diagnostic(
    case: str, toolchain: str, log: Path, exit_code: int, output: Optional[Path]
) -> None:
    if exit_code != 1:
        raise SystemExit(f"{case}: doctor exit code must be 1, got {exit_code}")
    rustc, chigiri, coordinate, fix = CASES[case]
    records = read_diagnostics(log)
    if rustc and not any(item.get("origin") == "rustc" and item.get("code") == rustc for item in records):
        raise SystemExit(f"{case}: missing expected rustc diagnostic {rustc}.")
    supplements = [item for item in records if item.get("code") in {"SF4018", "SF4019"}]
    expected = [item for item in records if item.get("code") == chigiri]
    if len(expected) != 1 or len(supplements) > 1:
        raise SystemExit(f"{case}: expected exactly one Chigiri diagnostic.")
    serialized = json.dumps(expected[0], ensure_ascii=False, sort_keys=True)
    for marker in (coordinate, fix):
        if marker not in serialized:
            raise SystemExit(f"{case}: Chigiri diagnostic is missing marker {marker!r}.")
    if case == "wrong-output" and "ID!" not in serialized:
        raise SystemExit("wrong-output: diagnostic lacks the expected GraphQL type.")
    if rustc is None:
        if not any(item.get("code") == "SF4020" for item in records):
            raise SystemExit("The original inventory did not produce the expected SF4020 violation.")
        if any(item.get("origin") in {"cargo", "rustc"} for item in records):
            raise SystemExit(f"{case}: Running cargo after static failure.")
    write_json(
        output,
        {
            "case": case,
            "exitCode": exit_code,
            "chigiriCode": chigiri,
            "rustcCode": rustc,
            "toolchain": toolchain,
        },
    )


def write_json(path: Optional[Path], value: object) -> None:
    serialized = json.dumps(value, ensure_ascii=False, sort_keys=True) + "\n"
    if path is None:
        print(serialized, end="")
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=path.parent, delete=False) as temporary:
        temporary.write(serialized)
        temporary_path = temporary.name
    os.replace(temporary_path, path)


def main() -> None:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    shape_parser = subparsers.add_parser("shape")
    shape_parser.add_argument("schema", type=Path)
    shape_parser.add_argument("consumer", type=Path)
    shape_parser.add_argument("--output", type=Path)
    mutate_parser = subparsers.add_parser("mutate")
    mutate_parser.add_argument("case", choices=CASES)
    mutate_parser.add_argument("root", type=Path)
    positive_parser = subparsers.add_parser("positive")
    positive_parser.add_argument("log", type=Path)
    diagnostic_parser = subparsers.add_parser("diagnostic")
    diagnostic_parser.add_argument("case", choices=CASES)
    diagnostic_parser.add_argument("toolchain", choices=("1.95.0",))
    diagnostic_parser.add_argument("log", type=Path)
    diagnostic_parser.add_argument("exit_code", type=int)
    diagnostic_parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.command == "shape":
        shape(args.schema, args.consumer, args.output)
    elif args.command == "mutate":
        mutate(args.root, args.case)
    elif args.command == "positive":
        positive(args.log)
    elif args.command == "diagnostic":
        diagnostic(args.case, args.toolchain, args.log, args.exit_code, args.output)


if __name__ == "__main__":
    main()
