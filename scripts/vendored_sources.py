#!/usr/bin/env python3
"""Verify pinned upstream sources and offline manifest-only patches."""

import hashlib
import json
import re
import sys
import tomllib
from pathlib import Path

from implementation_rule_io import require

ROOT = Path(__file__).resolve().parent.parent

SOURCE_PATCHES = {}


def original_matches(content, expected):
    return len(content) == expected["bytes"] and hashlib.sha256(content).hexdigest() == expected["sha256"]


def checked_source(content, expected, replacements, refresh):
    """Verify the original digest and return the deterministically patched bytes."""
    original = content
    if not (refresh and original_matches(content, expected)):
        for before, after in reversed(replacements):
            require(original.count(after) == 1)
            original = original.replace(after, before)
    require(original_matches(original, expected))
    patched = original
    for before, after in replacements:
        require(patched.count(before) == 1)
        patched = patched.replace(before, after)
    require(refresh or content == patched)
    return patched


def patch_manifest(name, source):
    """Retain library dependencies while disabling upstream CLI and test targets."""
    sections = re.split(r"(?=^\[)", source, flags=re.MULTILINE)
    output = []
    for section in sections:
        header = section.splitlines()[0] if section else ""
        if header.startswith(("[[bin]]", "[[bench]]", "[[test]]", "[dev-dependencies.")):
            continue
        if name == "stylua" and header in {
            f"[dependencies.{dependency}]" for dependency in (
                "anyhow", "clap", "console", "crossbeam-channel", "env_logger",
                "globset", "ignore", "num_cpus", "serde_json", "similar", "threadpool", "toml",
            )
        }:
            continue
        if name == "full_moon" and header == "[dependencies.paste]":
            section = '[dependencies.paste]\npackage = "pastey"\nversion = "=0.2.3"\n\n'
        output.append(section)
    return "".join(output)


def check(refresh=False, root=ROOT):
    inventory = json.loads((root / "scripts/vendored-sources.json").read_text())
    require(inventory["format"] == 1)
    expected_roots = {package.get("directory", f"{package['name']}-{package['version']}")
                      for package in inventory["packages"]} | {"README.md"}
    require({path.name for path in (root / "vendor").iterdir()} == expected_roots)
    for package in inventory["packages"]:
        name, version = package["name"], package["version"]
        source_patches = SOURCE_PATCHES.get(f"{name}-{version}", {})
        require(set(source_patches) <= set(package["files"]))
        directory_name = package.get("directory", f"{name}-{version}")
        require(directory_name == f"{name}-{version}")
        directory = root / "vendor" / directory_name
        require(directory.is_dir() and not directory.is_symlink())
        original = (directory / "Cargo.toml.upstream").read_bytes()
        expected = package["files"]["Cargo.toml"]
        require(len(original) == expected["bytes"])
        require(hashlib.sha256(original).hexdigest() == expected["sha256"])
        patched = patch_manifest(name, original.decode()).encode()
        if refresh:
            (directory / "Cargo.toml").write_bytes(patched)
        require((directory / "Cargo.toml").read_bytes() == patched)
        manifest = tomllib.loads(patched.decode())
        require(manifest["package"]["name"] == name)
        require(manifest["package"]["version"] == version)
        require(manifest["package"]["license"] == package.get("license", "MPL-2.0"))
        actual = set()
        for path in directory.rglob("*"):
            if not (not path.is_symlink()):
                raise ValueError("vendor symlink rejected")
            if not path.is_file():
                continue
            relative = path.relative_to(directory).as_posix()
            actual.add(relative)
            if relative in {"Cargo.toml", "Cargo.toml.upstream"}:
                continue
            if relative in package.get("local_files", {}):
                require(original_matches(path.read_bytes(), package["local_files"][relative]))
                continue
            expected = package["files"][relative]
            content = path.read_bytes()
            patched_source = checked_source(content, expected, source_patches.get(relative, ()), refresh)
            if refresh and content != patched_source:
                path.write_bytes(patched_source)
        require(actual == set(package["files"]) | set(package.get("local_files", {})) | {"Cargo.toml.upstream"})
        count = sum(len(replacements) for replacements in source_patches.values())
        print(f"{name} {version}: {len(actual) - 2} upstream source files match; {count} manifest-only patch lifetime markers are consistent")


if __name__ == "__main__":
    require(sys.argv[1:] in ([], ["--refresh"]))
    check(refresh=sys.argv[1:] == ["--refresh"])
