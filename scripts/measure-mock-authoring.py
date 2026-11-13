#!/usr/bin/env python3
"""Measure current Mock authoring commands with one release binary and fixed SDL."""

import argparse
import hashlib
import json
import platform
import re
import subprocess
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
RUNS = 3


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def invoke(binary, arguments, directory, label):
    started = time.monotonic()
    result = subprocess.run(
        [str(binary), "mock", *map(str, arguments)],
        cwd=ROOT,
        capture_output=True,
        timeout=300,
    )
    elapsed = time.monotonic() - started
    (directory / f"{label}.stdout").write_bytes(result.stdout)
    (directory / f"{label}.stderr").write_bytes(result.stderr)
    if result.returncode:
        raise ValueError(f"{label} failed with exit {result.returncode}")
    return {
        "argv": ["<binary>", "mock", *map(str, arguments)],
        "elapsed_seconds": elapsed,
        "exit_code": result.returncode,
    }


def ready_resolvers():
    source = (ROOT / "crates/mock/fixtures/mock-v11-ready.toml").read_text()
    pattern = re.compile(
        r'^\[resolvers\."([^"]+)"\]\nluau = \'\'\'\n(.*?)\n\'\'\'$',
        re.MULTILINE | re.DOTALL,
    )
    entries = pattern.findall(source)
    if len(entries) != 6:
        raise ValueError("The fixed ready fixture must contain six resolvers.")
    return dict(entries)


def install_ready_resolvers(project):
    for coordinate, source in ready_resolvers().items():
        parent, field = coordinate.split(".", 1)
        path = project / "resolvers" / parent.lower() / f"{field}.luau"
        if not path.is_file():
            raise ValueError(f"Generated resolver is missing: {coordinate}")
        path.write_text(source + "\n")


def series(binary, arguments, directory, prefix):
    return [invoke(binary, arguments, directory, f"{prefix}-{index + 1}") for index in range(RUNS)]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--source-sha256", required=True)
    parser.add_argument("--phase", choices=("baseline", "final"), required=True)
    args = parser.parse_args()
    binary = args.binary.resolve()
    output = args.output.resolve()
    if not binary.is_file() or output.suffix != ".json":
        raise ValueError("The binary and JSON output paths are required.")
    directory = output.parent
    directory.mkdir(parents=True, exist_ok=True)
    fixture = directory / f"{args.phase}-fixture"
    prior = [output, fixture, *directory.glob(f"{args.phase}-*.stdout"), *directory.glob(f"{args.phase}-*.stderr")]
    if any(path.exists() for path in prior):
        raise ValueError("The measurement destination must be fresh.")
    fixture.mkdir()
    schema = ROOT / "crates/mock/fixtures/blog.graphql"
    init = []
    projects = []
    for index in range(RUNS):
        project = fixture / f"project-{index + 1}"
        init.append(
            invoke(
                binary,
                ["init", "--schema", schema, "--config-dir", project],
                directory,
                f"{args.phase}-init-{index + 1}",
            )
        )
        install_ready_resolvers(project)
        projects.append(project)
    project = projects[0]
    full = series(binary, ["check", "--config-dir", project], directory, f"{args.phase}-full")
    resolver = "resolvers/query/getUsers.luau"
    target = series(
        binary,
        ["check", "--config-dir", project, "--target", resolver],
        directory,
        f"{args.phase}-resolver",
    )
    result = {
        "format": "chigiri-mock-authoring-measurement/v3",
        "phase": args.phase,
        "platform": platform.platform(),
        "binary_sha256": digest(binary),
        "binary_bytes": binary.stat().st_size,
        "source_sha256": args.source_sha256,
        "fixture": "crates/mock/fixtures/blog.graphql",
        "fixture_sha256": digest(schema),
        "runs": RUNS,
        "series_semantics": "first invocation is cold; later invocations repeat the same command and captured project",
        "init": init,
        "full_check": full,
        "resolver_target": target,
        "retired": ["library_target"],
    }
    output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
