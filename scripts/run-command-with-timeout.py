#!/usr/bin/env python3

import os
import json
import platform
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path


class CommandCancelled(BaseException):
    """Convert parent termination into failure after reaping the child process group."""

    def __init__(self, signum: int):
        self.signum = signum


def cancel(signum, _frame):
    raise CommandCancelled(signum)


def group_exists(process_group: int) -> bool:
    try:
        os.killpg(process_group, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        # A permission error does not prove that the group has exited, so check again after reaping.
        return True


def wait_for_group_exit(
    process: subprocess.Popen, process_group: int, timeout: float
) -> bool:
    deadline = time.monotonic() + timeout
    while True:
        process.poll()
        if not group_exists(process_group):
            return True
        if time.monotonic() >= deadline:
            return False
        time.sleep(0.01)


def stop_group(process: subprocess.Popen) -> None:
    process_group = process.pid
    try:
        os.killpg(process_group, signal.SIGTERM)
    except (ProcessLookupError, PermissionError):
        pass
    if wait_for_group_exit(process, process_group, 2):
        process.wait(timeout=2)
        return
    try:
        os.killpg(process_group, signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        pass
    if not wait_for_group_exit(process, process_group, 2):
        raise RuntimeError("Could not confirm termination of the child process group.")
    process.wait(timeout=2)


def write_result(argv: list[str], exit_code: int, elapsed: float) -> None:
    result_path = os.environ.get("CHIGIRI_COMMAND_RESULT")
    if result_path is None:
        return
    result = {
        "argv": argv,
        "elapsedSeconds": round(elapsed, 6),
        "exitCode": exit_code,
        "label": os.environ.get("CHIGIRI_COMMAND_LABEL", "command"),
        "timedOut": exit_code == 124,
        "toolchain": os.environ.get("CHIGIRI_COMMAND_TOOLCHAIN", "none"),
    }
    destination = Path(result_path)
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        mode="w", encoding="utf-8", dir=destination.parent, delete=False
    ) as temporary:
        json.dump(result, temporary, ensure_ascii=False, sort_keys=True)
        temporary.write("\n")
        temporary_path = temporary.name
    os.replace(temporary_path, destination)


def main() -> int:
    if len(sys.argv) < 4:
        raise SystemExit("usage: run-command-with-timeout.py SECONDS LOG COMMAND...")
    timeout = float(sys.argv[1])
    log_path = sys.argv[2]
    time_args = ["/usr/bin/time", "-l" if platform.system() == "Darwin" else "-v"]
    argv = sys.argv[3:]
    started = time.monotonic()
    exit_code = 124
    process = None
    previous = {signum: signal.signal(signum, cancel) for signum in (signal.SIGINT, signal.SIGTERM)}
    try:
        with open(log_path, "wb") as log:
            process = subprocess.Popen(
                [*time_args, *argv],
                stdout=log,
                stderr=subprocess.STDOUT,
                start_new_session=True,
            )
            try:
                exit_code = process.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                stop_group(process)
    except CommandCancelled as error:
        # A second cancellation must not interrupt child termination and reaping.
        for signum in previous:
            signal.signal(signum, signal.SIG_IGN)
        if process is not None:
            stop_group(process)
        exit_code = 128 + error.signum
    finally:
        for signum, handler in previous.items():
            signal.signal(signum, handler)
    write_result(argv, exit_code, time.monotonic() - started)
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
