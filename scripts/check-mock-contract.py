#!/usr/bin/env python3
"""Verify Mock workspace membership, MSRV, and dependency isolation."""

import json
import subprocess
from pathlib import Path

from validation_plan import CLI_TEST_TARGETS, MOCK_TEST_TARGETS, NORMAL_PACKAGES


def validate(metadata, sdk_metadata):
    packages = {package["id"]: package for package in metadata["packages"]}
    names = {package["name"]: package for package in metadata["packages"]}
    normal = set(NORMAL_PACKAGES)
    members = {packages[key]["name"] for key in metadata["workspace_members"]}
    defaults = {packages[key]["name"] for key in metadata["workspace_default_members"]}
    if members != normal | {"chigiri_mock", "chigiri_cli"} or defaults != normal:
        raise ValueError("Workspace members or default members differ from the CLI and Mock contract.")
    mock = names["chigiri_mock"]
    cli = names["chigiri_cli"]
    tests = {target["name"] for target in mock["targets"] if target["kind"] == ["test"]}
    if tests != set(MOCK_TEST_TARGETS):
        raise ValueError("The shared validation set has missing or extra Mock integration targets.")
    cli_tests = {target["name"] for target in cli["targets"] if target["kind"] == ["test"]}
    # The artifact target runs explicitly through the common acceptance gate;
    # its ignored default-suite result is not product acceptance evidence.
    if cli_tests != set(CLI_TEST_TARGETS) | {"artifact_acceptance"}:
        raise ValueError("The shared validation set has missing or extra CLI integration targets.")
    cli_bins = {target["name"] for target in cli["targets"] if target["kind"] == ["bin"]}
    if cli_bins != {"chigiri"}:
        raise ValueError("The CLI must own exactly the chigiri product binary.")
    for package in (names["chigiri_codegen"], mock):
        if any(target["kind"] == ["bin"] for target in package["targets"]):
            raise ValueError("The codegen and Mock libraries must not own product binaries.")
    if any(package["rust_version"] != "1.95" or package["publish"] != []
           for package in (mock, cli)):
        raise ValueError("CLI or Mock MSRV and publish settings differ from the contract.")
    for name in normal:
        if names[name]["rust_version"] != "1.95":
            raise ValueError("The normal-package MSRV does not match the contract.")
    sdk = next(package for package in sdk_metadata["packages"]
               if package["name"] == "chigiri-mock-blog-contract")
    if sdk["rust_version"] != "1.95":
        raise ValueError("The comparison SDK MSRV does not match the contract.")
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    pending = [mock["id"]]
    visited = set()
    while pending:
        key = pending.pop()
        if key in visited:
            continue
        visited.add(key)
        if packages[key]["name"] == "chigiri-mock-blog-contract":
            raise ValueError("The Mock crate has a forbidden dependency on the generated blog contract.")
        pending.extend(node["pkg"] for node in nodes[key]["deps"]
                       if any(kind["kind"] is None for kind in node["dep_kinds"]))
    pending = [names[name]["id"] for name in normal]
    visited = set()
    while pending:
        key = pending.pop()
        if key in visited:
            continue
        visited.add(key)
        if packages[key]["name"].startswith("luaur") or packages[key]["name"] in {
            "stylua", "full_moon", "full_moon_derive", "ratatui", "crossterm",
        }:
            raise ValueError("A standard package has a forbidden Mock UI, formatter, or Luau dependency.")
        pending.extend(node["pkg"] for node in nodes[key]["deps"])
    vm = names["luaur"]
    if vm["version"] != "0.1.8" or set(nodes[vm["id"]]["features"]) != {"typecheck"}:
        raise ValueError("The VM version or features do not match the contract.")
    formatter = names["stylua"]
    if formatter["version"] != "2.5.2" or set(nodes[formatter["id"]]["features"]) != {"luau"}:
        raise ValueError("The formatter version or features do not match the contract.")
    print(
        f"Mock contract passed: {len(normal)} standard packages use Rust 1.95 without VM "
        "dependencies; CLI and Mock use Rust 1.95; the generated SDK uses Rust 1.95."
    )


def main():
    root = Path(__file__).resolve().parent.parent
    metadata = subprocess.run([
        "cargo", "+1.95.0", "metadata", "--locked", "--all-features", "--format-version", "1",
    ], cwd=root, check=True, capture_output=True, timeout=120)
    sdk_metadata = subprocess.run([
        "cargo", "+1.95.0", "metadata", "--no-deps", "--locked", "--format-version", "1",
        "--manifest-path", "generated/mock-blog/Cargo.toml",
    ], cwd=root, check=True, capture_output=True, timeout=120)
    validate(json.loads(metadata.stdout), json.loads(sdk_metadata.stdout))


if __name__ == "__main__":
    main()
