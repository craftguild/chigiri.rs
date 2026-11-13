#!/usr/bin/env python3
"""Convert authoritative product ownership into canonical shared validation fixtures."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib

from generated_inventory import MARKER, REGISTRY, output_roots, owned_rust
from graphiql_inventory import BASE, check as check_assets
from implementation_rule_io import read_file, require, visit_tree

ROOT = Path(__file__).resolve().parent.parent


def groups():
    result = []
    for output, label in output_roots(ROOT).items():
        marker = json.loads(read_file(ROOT / output / MARKER, 1024 * 1024))
        owner = marker["package"]["name"]
        manifest = next(name for name in json.loads((ROOT / REGISTRY).read_text())
                        if Path(name).name == label and
                        (Path(name).parent / tomllib.loads((ROOT / name).read_text())["generation"]["output"]).as_posix() == output)
        result.append({
            "owner": owner, "manifest": f"tests/fixtures/generated-ownership/{owner}.json",
            "output_root": output,
            "reproduce": ["cargo", "+1.95.0", "run", "--locked", "--quiet", "-p",
                          "chigiri_codegen", "--example", "reproduce-owned", "--", manifest, "{output}"],
            "rust_size_exception": True,
        })
    result.append({
        "owner": "chigiri-graphiql-assets", "manifest": "tests/fixtures/generated-ownership/graphiql-assets.json",
        "output_root": f"{BASE}/assets",
        "reproduce": ["python3", "scripts/graphiql_build.py", "--reproduce", "{output}"],
        "rust_size_exception": False,
    })
    return result


def digest(path):
    return hashlib.sha256(read_file(path, 64 * 1024 * 1024)).hexdigest()


def files(root):
    result = {}
    visit_tree(root, lambda name, data: result.update({name: hashlib.sha256(data).hexdigest()}))
    return result


def inputs(group):
    # Bind the authoritative product provenance, not a second mutable Cargo
    # input inventory. Canonical replay still validates the real generator.
    if group["owner"] == "chigiri-graphiql-assets":
        names = {f"{BASE}/asset-inventory.json", f"{BASE}/dependency-licenses.json"}
    else:
        marker_path = f"{group['output_root']}/{MARKER}"
        manifest = group["reproduce"][-2]
        names = {marker_path, manifest}
        config = tomllib.loads((ROOT / manifest).read_text())
        marker = json.loads((ROOT / marker_path).read_text())
        for source in marker["provenance"]["sources"]:
            names.add((Path(manifest).parent / config["schema"]["directory"] / source["path"]).as_posix())
    return {name: digest(ROOT / name) for name in sorted(names)}


def refresh(verify, check=False):
    owned_rust(ROOT)
    check_assets(ROOT)
    selected = groups()
    configured = tomllib.loads((ROOT / "craftguild.toml").read_text()).get("generated", [])
    require(configured == selected)
    parent = ROOT / "target/standards-ownership"
    parent.mkdir(parents=True, exist_ok=True)
    for group in selected:
        expected = files(ROOT / group["output_root"])
        if verify:
            with tempfile.TemporaryDirectory(prefix="reproduce-", dir=parent) as name:
                output = Path(name)
                argv = [str(output) if item == "{output}" else item for item in group["reproduce"]]
                subprocess.run(argv, cwd=ROOT, check=True, timeout=600)
                require(files(output) == expected)
        source_inputs = inputs(group)
        if group["owner"] == "chigiri-graphiql-assets":
            revision = digest(ROOT / BASE / "asset-inventory.json")
        else:
            marker = json.loads((ROOT / group["output_root"] / MARKER).read_text())
            revision = marker["generator"]["git_commit"]
        record = {"schema_version": 1, "owner": group["owner"], "generator_revision": revision,
                  "inputs": source_inputs,
                  "files": {f"{group['output_root']}/{name}": value for name, value in sorted(expected.items())}}
        data = (json.dumps(record, indent=2) + "\n").encode()
        destination = ROOT / group["manifest"]
        require(not (ROOT / "tests/fixtures/generated-ownership").is_symlink())
        require(not destination.parent.is_symlink() and not destination.is_symlink())
        destination.parent.mkdir(parents=True, exist_ok=True)
        if check:
            require(destination.read_bytes() == data)
        else:
            destination.write_bytes(data)
        # Every ordinary assessment subsequently requires the shared verifier's
        # isolated reproduction. Conversion alone never grants an exemption.
        print(f"Prepared shared ownership: {group['owner']} ({len(expected)} files)", file=sys.stderr, flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verify", action="store_true")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    refresh(args.verify, args.check)
