"""Verify GraphiQL asset ownership, provenance, and embedding inventories."""

import hashlib
import json
from pathlib import Path
import sys

from implementation_rule_io import components, read_file, require, visit_tree

ROOT = Path(__file__).resolve().parent.parent
BASE = "crates/graphiql"
SOURCES = (
    "frontend/package.json", "frontend/package-lock.json", "frontend/entry.mjs",
    "frontend/layout.css", "frontend/build.mjs", "frontend/license-supplements.json",
)
TOOLS = ("scripts/graphiql_build.py", "scripts/graphiql_inventory.py", "scripts/graphiql_evidence.py")
BASE_LICENSES = {"Apache-2.0", "BSD-3-Clause", "MIT", "Unicode-3.0"}
CODE_SUFFIXES = {".js", ".mjs", ".cjs", ".jsx", ".ts", ".tsx", ".css", ".html"}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def source_inventory(root):
    names = [f"{BASE}/{name}" for name in SOURCES] + list(TOOLS)
    files, directories = visit_tree(root / BASE / "frontend", lambda _path, _data: None)
    require(files == {name.removeprefix("frontend/") for name in SOURCES})
    require(not directories)
    return {name: digest(read_file(root / name, 2 * 1024 * 1024)) for name in names}


def check(root=ROOT, verify_sources=True):
    base = root / BASE
    if not base.exists():
        # The entire asset crate in the cargo repository is not exempt. Only the old checker single fixture is allowed.
        require(not (root / "Cargo.toml").exists())
        return set()
    inventory = json.loads(read_file(base / "asset-inventory.json", 1024 * 1024))
    require(set(inventory) == {"format", "sources", "files", "embedding", "licenses"})
    require(inventory["format"] == "chigiri-graphiql-assets/v1")
    if verify_sources:
        require(inventory["sources"] == source_inventory(root))
    records = inventory["files"]
    require(isinstance(records, list) and 1 <= len(records) <= 128)
    expected = {record["path"]: record for record in records}
    require(len(expected) == len(records))
    total = 0

    def inspect(path, data):
        nonlocal total
        require(path in expected)
        record = expected[path]
        require(set(record) == {"path", "bytes", "sha256", "content_type"})
        components(path)
        require(record["bytes"] == len(data) <= 16 * 1024 * 1024)
        require(record["sha256"] == digest(data))
        require(record["content_type"] in {
            "text/javascript; charset=utf-8", "text/css; charset=utf-8",
            "font/ttf", "text/plain; charset=utf-8",
        })
        total += len(data)

    files, directories = visit_tree(base / "assets", inspect)
    require(files == set(expected) and total <= 32 * 1024 * 1024)
    expected_directories = {str(parent) for name in files for parent in Path(name).parents if str(parent) != "."}
    require(directories == expected_directories)
    require(inventory["embedding"] == digest(read_file(base / "src/embedded.rs", 128 * 1024)))
    require(inventory["licenses"] == digest(read_file(base / "dependency-licenses.json", 1024 * 1024)))
    return {f"{BASE}/assets/{path}" for path in files if Path(path).suffix in CODE_SUFFIXES}


def licenses(root=ROOT):
    check(root)
    base = root / BASE
    policy = json.loads(read_file(base / "license-policy.json", 65536))
    require(set(policy) == {"allow", "exceptions"})
    require(set(policy["allow"]) == BASE_LICENSES)
    allowed = {}
    for record in policy["exceptions"]:
        require(set(record) == {"name", "version", "license", "approval"})
        require(isinstance(record["approval"], str) and record["approval"].strip())
        key = record["name"], record["version"], record["license"]
        require(key not in allowed)
        allowed[key] = False
    rejected = set()
    for record in json.loads(read_file(base / "dependency-licenses.json", 1024 * 1024)):
        key = record["name"], record["version"], record["license"]
        if record["license"] in BASE_LICENSES:
            continue
        if key in allowed:
            allowed[key] = True
        else:
            rejected.add(key)
    for name, version, license_name in sorted(rejected):
        print(f"unapproved GraphiQL dependency license: {name} {version}: {license_name}", file=sys.stderr)
    require(not rejected and all(allowed.values()))


if __name__ == "__main__":
    if sys.argv[1:] == ["licenses"]:
        licenses()
        print("GraphiQL dependency license check passed")
    else:
        require(not sys.argv[1:])
        print(f"GraphiQL fixed asset check passed: {len(check())} generated files")
