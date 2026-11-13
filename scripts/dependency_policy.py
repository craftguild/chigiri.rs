#!/usr/bin/env python3
"""Separate ordinary third-party SDK dependencies from license-policy decisions."""

import json
import subprocess
import tempfile
import tomllib
from pathlib import Path

from generated_inventory import output_roots, owned_rust
from implementation_rule_io import read_file, require


def wildcard_dependencies(metadata):
    """Keep wildcard rejection except for verified local workspace edges.

    cargo-deny's private Path/Git and development-edge exceptions are retained.
    Its public-crate rule cannot express a source-only workspace release whose
    internal versions have one shared authority. Inspect the complete graph
    before cargo-deny; do not exclude any node from its other policies.
    """
    packages = metadata["packages"]
    workspace = set(metadata["workspace_members"])
    local = {str(Path(package["manifest_path"]).parent.resolve()): package
             for package in packages if package["id"] in workspace and package["source"] is None}
    approved = 0
    for package in packages:
        for dependency in package["dependencies"]:
            if dependency["req"] != "*":
                continue
            source = dependency.get("source")
            path = dependency.get("path")
            explicit = bool(path) or (isinstance(source, str) and source.startswith("git+"))
            if explicit and (package.get("publish") == [] or dependency["kind"] == "dev"):
                continue
            target = local.get(str(Path(path).resolve())) if path else None
            require(package["id"] in workspace and package["source"] is None
                    and source is None and target is not None
                    and dependency["name"] == target["name"])
            approved += 1
    return approved


def license_exclusions(repository, metadata, registered):
    """We refuse to abandon third party dependence by exclusion, confusion and exclusion."""
    packages = metadata["packages"]
    by_id = {package["id"]: package for package in packages}
    require(len(by_id) == len(packages))
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    workspace = set(metadata["workspace_members"])
    runtime_path = (repository / "crates/facade/Cargo.toml").resolve()
    exclusions = []
    for package in packages:
        manifest = Path(package["manifest_path"]).resolve()
        if manifest not in registered:
            continue
        expected = registered[manifest]
        require(package["source"] is None and package["publish"] == [])
        require((package["name"], package["version"]) == expected)
        require(sum((p["name"], p["version"]) == expected for p in packages) == 1)
        require(package["id"] not in workspace)
        dependencies = package["dependencies"]
        require(len(dependencies) == 1)
        dependency = dependencies[0]
        require(dependency["name"] == "chigiri" and dependency["kind"] is None)
        require(dependency["source"] is None)
        require(Path(dependency["path"]).resolve() == runtime_path.parent)
        resolved = nodes[package["id"]]["deps"]
        require(len(resolved) == 1 and resolved[0]["name"] == "chigiri")
        runtime_id = resolved[0]["pkg"]
        # Removing the SDK from the graph must leave only the workspace-root dependency and its closure.
        require(runtime_id in workspace)
        runtime = by_id[runtime_id]
        require(runtime["source"] is None and runtime["name"] == "chigiri")
        require(Path(runtime["manifest_path"]).resolve() == runtime_path)
        exclusions.append(f"{expected[0]}@{expected[1]}")
    return sorted(exclusions)


def main():
    repository = Path(__file__).resolve().parent.parent
    owned_rust(repository)
    registered = {}
    for relative in output_roots(repository):
        manifest = repository / relative / "Cargo.toml"
        package = tomllib.loads(read_file(manifest, 64 * 1024).decode())["package"]
        registered[manifest.resolve()] = (package["name"], package["version"])
    directory = repository / "target/sibling-generator"
    directory.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="dependency-policy-", dir=directory) as name:
        metadata_path = Path(name) / "metadata.json"
        with metadata_path.open("wb") as output:
            subprocess.run([
                "cargo", "metadata", "--locked", "--all-features", "--format-version", "1",
            ], cwd=repository, stdout=output, check=True, timeout=120)
        metadata = json.loads(read_file(metadata_path, 64 * 1024 * 1024))
        approved = wildcard_dependencies(metadata)
        print(f"Complete-graph wildcard policy passed: {approved} local workspace edges", flush=True)
        exclusions = license_exclusions(repository, metadata, registered)
        cargo = ["cargo", "deny", "--locked", "--workspace", "--all-features"]
        check = ["check", "--metadata-path", str(metadata_path)]
        subprocess.run([*cargo, *check, "advisories", "bans", "sources"],
                       cwd=repository, check=True, timeout=300)
        print("Excluding the normal license-only SDK:" + ", ".join(exclusions), flush=True)
        arguments = [value for package in exclusions for value in ("--exclude", package)]
        subprocess.run([*cargo, *arguments, *check, "licenses"],
                       cwd=repository, check=True, timeout=120)
    owned_rust(repository)


if __name__ == "__main__":
    main()
