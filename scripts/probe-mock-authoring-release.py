#!/usr/bin/env python3
"""Record the ADR-0049 public-release feasibility gate without source patches."""

import argparse
import hashlib
import json
import platform
import re
import shutil
import subprocess
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LUAUR_VERSION = "0.1.8"
REGISTRY_SOURCE = "registry+https://github.com/rust-lang/crates.io-index"
LOCK_FIELD = re.compile(r'^(name|version|source|checksum) = "([^"]*)"$')


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def crates_io(name, version):
    request = urllib.request.Request(
        f"https://crates.io/api/v1/crates/{name}",
        headers={"User-Agent": "chigiri-mock-release-gate/1"},
    )
    with urllib.request.urlopen(request, timeout=20) as response:
        body = json.load(response)
    selected = next(item for item in body["versions"] if item["num"] == version)
    return {
        "name": name,
        "max_version": body["crate"]["max_version"],
        "selected_version": selected["num"],
        "selected_yanked": selected["yanked"],
        "selected_license": selected["license"],
        "selected_checksum": selected["checksum"],
        "repository": body["crate"]["repository"],
    }


def command(directory, label, argv, expected):
    started = time.monotonic()
    result = subprocess.run(argv, cwd=directory, capture_output=True, timeout=300)
    elapsed = time.monotonic() - started
    stdout = result.stdout
    stderr = result.stderr
    (directory / f"{label}.stdout").write_bytes(stdout)
    (directory / f"{label}.stderr").write_bytes(stderr)
    if result.returncode != expected:
        raise ValueError(f"{label} exited {result.returncode}; expected {expected}")
    return {
        "argv": list(argv),
        "exit_code": result.returncode,
        "elapsed_seconds": elapsed,
        "stdout_sha256": sha256(stdout),
        "stderr_sha256": sha256(stderr),
    }


def write_probe(directory):
    (directory / "src/bin").mkdir(parents=True)
    (directory / "Cargo.toml").write_text(
        """[package]
name = "chigiri-mock-release-probe"
version = "0.0.0"
edition = "2024"
publish = false

[workspace]

[dependencies]
luaur-rt = { version = "=0.1.8", default-features = false, features = ["typecheck"] }
"""
    )
    (directory / "src/bin/one_shot.rs").write_text(
        """fn main() {
    luaur_rt::check(
        "--!strict\\nlocal shared = require(\\\"@lib/shared\\\")\\nlocal value: number = shared.missing(\\\"wrong\\\")\\nreturn value\\n",
    )
    .expect("the published one-source helper currently accepts an unresolved require boundary");
    println!("unresolved_require_accepted=true typed_consumer_checked=false");
}
"""
    )
    (directory / "src/bin/multi_source.rs").write_text(
        """use luaur_rt::{ModuleSource, check_modules_with_definitions};

fn main() {
    let sources = [ModuleSource { name: "main", source: "return 1" }];
    let _ = check_modules_with_definitions(&sources, &["main"], &[], "");
}
"""
    )
    (directory / "src/bin/diagnostic_fields.rs").write_text(
        """fn main() {
    let diagnostic = luaur_rt::check("local value: number = 'x'").unwrap_err().remove(0);
    let _ = (diagnostic.module, diagnostic.kind);
}
"""
    )


def selected_lock_packages():
    selected = {}
    for block in (ROOT / "Cargo.lock").read_text().split("[[package]]")[1:]:
        package = {}
        for line in block.splitlines():
            match = LOCK_FIELD.fullmatch(line)
            if match:
                package[match.group(1)] = match.group(2)
        name = package.get("name", "")
        if name.startswith("luaur"):
            missing = {"version", "source", "checksum"} - package.keys()
            if missing:
                raise ValueError(f"The {name} lock entry is missing {sorted(missing)}.")
            selected[name] = {
                key: package[key] for key in ("version", "source", "checksum")
            }
    missing = {"luaur", "luaur-rt"} - selected.keys()
    if missing:
        raise ValueError(f"The lockfile is missing required packages: {sorted(missing)}.")
    return selected


def verify_releases(lock_packages, releases):
    for release in releases:
        locked = lock_packages[release["name"]]
        if release["selected_yanked"]:
            raise ValueError(f"The selected {release['name']} release is yanked.")
        if release["selected_checksum"] != locked["checksum"]:
            raise ValueError(f"The selected {release['name']} checksum does not match Cargo.lock.")


def repository_selection(lock_packages):
    completed = subprocess.run(
        [
            "cargo",
            "+1.95.0",
            "metadata",
            "--locked",
            "--offline",
            "--no-deps",
            "--format-version",
            "1",
        ],
        cwd=ROOT,
        check=True,
        capture_output=True,
        timeout=120,
    )
    packages = json.loads(completed.stdout)["packages"]
    mock = next(
        package
        for package in packages
        if Path(package["manifest_path"]).resolve() == ROOT / "crates/mock/Cargo.toml"
    )
    dependency = next(item for item in mock["dependencies"] if item["name"] == "luaur")
    selected = {
        "version": dependency["req"],
        "default-features": dependency["uses_default_features"],
        "features": dependency["features"],
    }
    overrides = sorted(
        name
        for name, package in lock_packages.items()
        if package["source"] != REGISTRY_SOURCE
    )
    if dependency["source"] != REGISTRY_SOURCE:
        overrides = sorted(set(overrides) | {"luaur"})
    return selected, overrides


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    artifact_root = (ROOT / "target").resolve()
    if output.suffix != ".json" or not output.is_relative_to(artifact_root):
        raise ValueError("The output must be a JSON file below target.")
    output.parent.mkdir(parents=True, exist_ok=True)
    directory = output.parent / "release-gate-probe"
    if directory.exists():
        shutil.rmtree(directory)
    directory.mkdir()
    write_probe(directory)

    lock_packages = selected_lock_packages()
    release_names = sorted(lock_packages)
    releases = [crates_io(name, lock_packages[name]["version"]) for name in release_names]
    verify_releases(lock_packages, releases)
    selected, patched = repository_selection(lock_packages)
    if selected["version"] != f"={LUAUR_VERSION}":
        raise ValueError("The Mock dependency is not pinned to the probed release.")
    if patched:
        raise ValueError("The public-release gate cannot run with a luaur source override.")

    commands = {}
    commands["generate_lock"] = command(
        directory,
        "generate-lock",
        ["cargo", "+1.95.0", "generate-lockfile", "--offline"],
        0,
    )
    commands["one_shot"] = command(
        directory,
        "one-shot",
        ["cargo", "+1.95.0", "run", "--locked", "--offline", "--bin", "one_shot"],
        0,
    )
    commands["multi_source_api"] = command(
        directory,
        "multi-source-api",
        ["cargo", "+1.95.0", "check", "--locked", "--offline", "--bin", "multi_source"],
        101,
    )
    commands["diagnostic_fields"] = command(
        directory,
        "diagnostic-fields",
        ["cargo", "+1.95.0", "check", "--locked", "--offline", "--bin", "diagnostic_fields"],
        101,
    )
    metadata = subprocess.run(
        ["cargo", "+1.95.0", "metadata", "--locked", "--offline", "--format-version", "1"],
        cwd=directory,
        check=True,
        capture_output=True,
        timeout=120,
    )
    packages = json.loads(metadata.stdout)["packages"]
    runtime = next(package for package in packages if package["name"] == "luaur-rt")
    source_root = Path(runtime["manifest_path"]).parent
    result = {
        "format": "chigiri-mock-public-release-gate/v1",
        "gate_passed": False,
        "recorded_at_unix": int(time.time()),
        "platform": platform.platform(),
        "rustc": subprocess.check_output(["rustc", "+1.95.0", "--version"], text=True).strip(),
        "mock_dependency": selected,
        "root_luaur_patches": patched,
        "lock_packages": lock_packages,
        "crates_io_releases": releases,
        "resolved_luaur_rt_source": runtime["source"],
        "resolved_luaur_rt_manifest_sha256": sha256(Path(runtime["manifest_path"]).read_bytes()),
        "resolved_luaur_rt_lib_sha256": sha256((source_root / "src/lib.rs").read_bytes()),
        "resolved_luaur_rt_typecheck_sha256": sha256((source_root / "src/typecheck.rs").read_bytes()),
        "commands": commands,
        "missing_capabilities": [
            "safe native multi-source checker input",
            "safe native module resolution",
            "module identity on diagnostics",
            "structured native diagnostic kind",
        ],
        "conclusion": "blocked_public_release_dependency",
    }
    output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
