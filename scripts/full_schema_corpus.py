"""Verify the repo-owned full schema inventory without external product knowledge."""

import json
import re
import tomllib
from pathlib import Path

from generated_inventory import closed_object, digest, fields, marker
from implementation_rule_io import components, read_file, require


FIXTURE = "tests/fixtures/full-schema"


def verify(fixture):
    source = json.loads(read_file(fixture / "source.json", 64 * 1024), object_pairs_hook=closed_object)
    fields(source, "schema_version schemaSources boundEnums representativeEnums shape")
    require(source["schema_version"] == "chigiri/owned-corpus-v1")
    require(len(source["schemaSources"]) == 11)
    recorded = {}
    texts = []
    for entry in source["schemaSources"]:
        fields(entry, "fixture bytes sha256")
        name = entry["fixture"]
        components(name)
        require(name.startswith("graphql/") and name.endswith(".graphql") and name not in recorded)
        data = read_file(fixture / name, 256 * 1024)
        require(type(entry["bytes"]) is int and len(data) == entry["bytes"] and digest(data) == entry["sha256"])
        recorded[name] = {"path": name.removeprefix("graphql/"), "bytes": len(data), "sha256": digest(data)}
        texts.append(data.decode("utf-8"))
    actual = {path.relative_to(fixture).as_posix() for path in (fixture / "graphql").rglob("*") if path.is_file()}
    total = sum(item["bytes"] for item in recorded.values())
    require(actual == set(recorded) and total >= 30_735)
    text = re.sub(r'"""[\s\S]*?"""|"(?:\\.|[^"\\])*"|#[^\n]*', "", "\n".join(texts))
    shape = {name: len(re.findall(r"\b" + keyword + r"\s+\w+", text))
             for name, keyword in [("objects", "type"), ("inputs", "input"), ("enums", "enum"), ("scalars", "scalar")]}
    shape["objectFields"] = sum(len(re.findall(r"^\s*\w+(?:\([^\n]*\))?\s*:", body, re.M))
                                for body in re.findall(r"\btype\s+\w+\s*\{([^}]+)\}", text))
    require(shape == source["shape"] == {"objects": 67, "objectFields": 709, "inputs": 19, "enums": 65, "scalars": 5})
    manifest = tomllib.loads(read_file(fixture / "chigiri.toml", 64 * 1024).decode("utf-8"))
    enums = sorted(re.findall(r"\benum\s+(\w+)", text))
    require(sorted(manifest["bindings"]["domain_enums"]) == source["boundEnums"] == enums)
    require(manifest["runtime"] == {"source": "path", "root": "../../.."})
    generated, inventory = marker(read_file(fixture / "generated/chigiri-generated.json", 1024 * 1024), "chigiri.toml")
    provenance = generated["provenance"]
    require(provenance["sources"] == [recorded[name] for name in sorted(recorded)])
    domain = read_file(fixture / "domain/src/lib.rs", 64 * 1024).decode("utf-8")
    require({item["wire"] for item in source["representativeEnums"]} == {"FormatKind", "TransportKind", "UnitKind"})
    for item in source["representativeEnums"]:
        fields(item, "wire domain variants")
        declaration = re.search(r"pub enum " + re.escape(item["domain"]) + r"\s*\{([^}]+)\}", domain)
        require(declaration is not None)
        require([name.strip() for name in declaration[1].split(",") if name.strip()] == item["variants"])
    return {"schemaFiles": len(recorded), "schemaBytes": total, "boundEnums": len(enums),
            "representativeEnums": 3, "generatedRustFiles": sum(name.endswith(".rs") for name in inventory),
            "schemaDigest": provenance["schema_sha256"], "bindingFingerprint": provenance["binding_sha256"]}


if __name__ == "__main__":
    print(json.dumps(verify(Path(__file__).resolve().parent.parent / FIXTURE), ensure_ascii=False))
