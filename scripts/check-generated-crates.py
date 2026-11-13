#!/usr/bin/env python3
"""Check every registered SDK from outside its generated ownership tree."""

import argparse
import json
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

from generated_inventory import REGISTRY, output_roots, owned_rust
from implementation_rule_io import read_file
from product_cli import built_cli


def run(repository, label, command, seconds=600):
    log = repository / "target/sibling-generator" / f"{label}.log"
    subprocess.run(
        [sys.executable, str(repository / "scripts/run-command-with-timeout.py"),
         str(seconds), str(log), *map(str, command)],
        cwd=repository, check=True,
    )
    print(f"{label}: passed", flush=True)


def exact(repository, cli):
    manifests = json.loads(read_file(repository / REGISTRY, 64 * 1024))
    if not manifests:
        raise ValueError("No managed manifests were found.")
    command = [cli or built_cli(), "rust"]
    for index, manifest in enumerate(manifests):
        run(repository, f"generated-exact-{index}", [
            *command, "--manifest", manifest, "check", "--format", "json", "--deny-warnings",
        ], 300)


def quality(repository, toolchain):
    roots = output_roots(repository)
    if not roots:
        raise ValueError("No managed SDKs were found.")
    temporary_parent = repository / "target/sibling-generator"
    for index, relative in enumerate(roots):
        sdk = repository / relative
        package = tomllib.loads(read_file(sdk / "Cargo.toml", 64 * 1024).decode("utf-8"))["package"]["name"]
        label = f"generated-quality-{toolchain}-{index}"
        run(repository, label + "-fmt", [
            "cargo", "+1.95.0", "fmt", "--manifest-path", sdk / "Cargo.toml", "--", "--check",
        ])
        # The SDK omits its lockfile and handwritten sources; an external consumer
        # connects them through ordinary Cargo dependencies.
        with tempfile.TemporaryDirectory(prefix="sdk-quality-", dir=temporary_parent) as directory:
            consumer = Path(directory)
            (consumer / "src").mkdir()
            (consumer / "src/lib.rs").write_text(
                "//! External consumer used to verify the generated SDK's public entry point.\npub use contract as sdk;\n",
                encoding="utf-8",
            )
            (consumer / "Cargo.toml").write_text(
                f'[package]\nname="chigiri-sdk-quality-{index}"\nversion="0.0.0"\n'
                'edition="2024"\nrust-version="1.95"\npublish=false\n[workspace]\n'
                f'[dependencies]\ncontract={{package={json.dumps(package)},path={json.dumps(str(sdk))}}}\n',
                encoding="utf-8",
            )
            manifest = consumer / "Cargo.toml"
            run(repository, label + "-lock", [
                "cargo", f"+{toolchain}", "generate-lockfile", "--manifest-path", manifest,
            ])
            run(repository, label + "-consumer", [
                repository / "scripts/check-cargo-consumer.sh", toolchain, manifest,
                repository / "target/generated-quality", "-p", f"chigiri-sdk-quality-{index}",
                "-p", package,
            ])


def main():
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest="mode", required=True)
    commands.add_parser("exact").add_argument("cli", type=Path, nargs="?")
    commands.add_parser("quality").add_argument("toolchain", choices=("1.95.0",))
    args = parser.parse_args()
    repository = Path(__file__).resolve().parent.parent
    (repository / "target/sibling-generator").mkdir(parents=True, exist_ok=True)
    owned_rust(repository)
    if args.mode == "exact":
        exact(repository, args.cli.resolve() if args.cli else None)
    else:
        quality(repository, args.toolchain)
    owned_rust(repository)


if __name__ == "__main__":
    main()
