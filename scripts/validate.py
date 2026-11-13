#!/usr/bin/env python3
"""Run product validation sequentially and reject source changes during the run."""

import argparse
import json
import os
import signal
import subprocess
import sys
import tempfile
from pathlib import Path

from validation_evidence import source_snapshot, validate_report, validate_result
from validation_plan import plan
from validation_profiles import COMPLETION_SCHEMA, DEFAULT_PROFILE, PROFILES


class ValidationCancelled(BaseException):
    def __init__(self, signum):
        self.signum = signum


def cancel(signum, _frame):
    raise ValidationCancelled(signum)


def run_step(repository, evidence, step):
    result_path = evidence / f"{step.label}.json"
    env = dict(os.environ, CHIGIRI_COMMAND_RESULT=str(result_path),
               CHIGIRI_COMMAND_LABEL=step.label, CHIGIRI_COMMAND_TOOLCHAIN="product")
    process = None
    try:
        process = subprocess.Popen([
            sys.executable, str(repository / "scripts/run-command-with-timeout.py"),
            str(step.seconds), str(evidence / f"{step.label}.log"), *step.argv,
        ], cwd=repository, env=env)
        status = process.wait()
    except ValidationCancelled:
        for signum in (signal.SIGINT, signal.SIGTERM):
            signal.signal(signum, signal.SIG_IGN)
        if process is not None:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        raise
    if status != 0:
        raise ValueError(f"{step.label} ended with nonzero: {status}; log: {evidence / (step.label + '.log')}")
    result = json.loads(result_path.read_text(encoding="utf-8"))
    validate_result(step, result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", choices=PROFILES, default=DEFAULT_PROFILE)
    args = parser.parse_args()
    repository = Path(__file__).resolve().parent.parent
    control = repository / ".craftguild"
    control.mkdir(exist_ok=True)
    current = control / f"current-{args.profile}.json"
    current.unlink(missing_ok=True)
    parent = repository / "target/product-validation"
    parent.mkdir(parents=True, exist_ok=True)
    evidence = Path(tempfile.mkdtemp(prefix="run-", dir=parent))
    before = source_snapshot(repository)
    (evidence / "source.json").write_text(json.dumps(before, ensure_ascii=False, indent=2) + "\n")
    steps = plan(args.profile)
    results = []
    previous = {signum: signal.signal(signum, cancel) for signum in (signal.SIGINT, signal.SIGTERM)}
    try:
        for step in steps:
            print(f"Verification: {step.label}", flush=True)
            results.append(run_step(repository, evidence, step))
    finally:
        for signum, handler in previous.items():
            signal.signal(signum, handler)
    if source_snapshot(repository) != before:
        raise ValueError("source tree changed during validation")
    report = {
        "schema_version": COMPLETION_SCHEMA, "profile": args.profile,
        "source_sha256": before["sha256"], "commands": results,
    }
    validate_report(args.profile, steps, report)
    (evidence / "completed.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    current.write_text(json.dumps({"schema_version": 1, "profile": args.profile,
                                  "source_sha256": before["sha256"],
                                  "completion": (evidence / "completed.json").relative_to(repository).as_posix()}) + "\n")
    print(f"Product verification succeeded ({args.profile}): {len(results)} commands; {evidence / 'completed.json'}")


if __name__ == "__main__":
    try:
        main()
    except ValidationCancelled as error:
        raise SystemExit(128 + error.signum)
    except (OSError, ValueError) as error:
        raise SystemExit(str(error)) from error
