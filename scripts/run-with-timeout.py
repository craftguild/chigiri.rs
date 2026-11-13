#!/usr/bin/env python3
"""Run a command with a timeout applied to its entire process group."""

from __future__ import annotations

import argparse
import os
import signal
import subprocess
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--seconds", type=int, required=True)
    parser.add_argument("--log", required=True)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.seconds <= 0 or not args.command:
        parser.error("A positive timeout and a command are required.")
    log = Path(args.log)
    log.parent.mkdir(parents=True, exist_ok=True)
    with log.open("w", encoding="utf-8") as destination:
        process = subprocess.Popen(
            args.command,
            stdout=destination,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        try:
            return process.wait(timeout=args.seconds)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
            destination.write(f"command timeout: {args.seconds} seconds\n")
            return 124


if __name__ == "__main__":
    raise SystemExit(main())
