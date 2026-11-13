#!/usr/bin/env python3
"""Compare the performance test to current candidate and fail closed."""

import argparse
from pathlib import Path

from performance_benchmark_evidence import require_benchmark
from performance_evidence import (
    EvidenceError,
    require_accepted_baseline,
    require_baseline_toolchain,
)
from performance_workload_evidence import require_workload
from performance_evidence_metadata import strict_json_loads


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "kind",
        choices=(
            "baseline-toolchain", "baseline", "benchmark", "load", "soak", "short-soak"
        ),
    )
    parser.add_argument("evidence", type=Path)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--toolchain", required=True)
    parser.add_argument(
        "--comparison-mode", choices=("initial-candidate", "previous-release")
    )
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--baseline-binary", type=Path)
    args = parser.parse_args()
    evidence = read_object(args.evidence, "performance evidence")
    if args.kind == "baseline-toolchain":
        require_baseline_toolchain(evidence, args.toolchain)
    elif args.kind == "baseline":
        if args.binary is None:
            raise SystemExit("Baseline evidence requires a benchmark binary.")
        require_accepted_baseline(
            evidence,
            args.binary,
            args.toolchain,
            args.comparison_mode or "",
            args.baseline_binary,
        )
    elif args.kind == "benchmark":
        if args.baseline is None or args.binary is None:
            raise SystemExit("Benchmark evidence verification requires a baseline and binary.")
        baseline = read_object(args.baseline, "benchmark baseline")
        require_benchmark(
            evidence,
            baseline,
            args.binary,
            args.toolchain,
            args.comparison_mode or "",
            args.baseline_binary,
            args.baseline,
        )
    else:
        if args.binary is None:
            raise SystemExit("Workload evidence verification requires a binary.")
        require_workload(evidence, args.kind, args.binary, args.toolchain)


def read_object(path: Path, label: str) -> dict[str, object]:
    value = strict_json_loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise EvidenceError(f"{label} root must be a JSON object")
    return value


if __name__ == "__main__":
    main()
