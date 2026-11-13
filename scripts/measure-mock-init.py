#!/usr/bin/env python3
"""Measure fresh-process initialization and record source identity and results as JSON."""

import argparse
import hashlib
import json
from pathlib import Path
import platform
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--schema", required=True, type=Path)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--max-seconds", type=float)
    args = parser.parse_args()
    if args.runs < 1:
        parser.error("--runs must be positive")
    binary = args.binary.resolve(strict=True)
    source = args.schema.read_bytes()
    report = {
        "platform": platform.platform(),
        "machine": platform.machine(),
        "binary": str(binary),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "schema_sha256": hashlib.sha256(source).hexdigest(),
        "runs": [],
    }
    for _ in range(args.runs):
        with tempfile.TemporaryDirectory(prefix="chigiri-init-measure-") as directory:
            root = Path(directory).resolve()
            (root / "schema.graphql").write_bytes(source)
            start = time.perf_counter()
            result = subprocess.run(
                [str(binary), "init", "--schema", "schema.graphql", "--config-dir", "mock"],
                cwd=root, capture_output=True, text=True, timeout=600,
            )
            elapsed = time.perf_counter() - start
            files = {}
            if result.returncode == 0:
                for path in sorted((root / "mock").rglob("*")):
                    if path.is_file():
                        files[str(path.relative_to(root / "mock"))] = hashlib.sha256(path.read_bytes()).hexdigest()
            report["runs"].append({
                "seconds": elapsed, "exit_code": result.returncode,
                "stdout": result.stdout, "stderr": result.stderr, "files": files,
            })
            print(f"init: {elapsed:.3f}s, exit={result.returncode}", flush=True)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    if any(run["exit_code"] for run in report["runs"]):
        raise SystemExit(1)
    if args.max_seconds is not None and any(run["seconds"] > args.max_seconds for run in report["runs"]):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
