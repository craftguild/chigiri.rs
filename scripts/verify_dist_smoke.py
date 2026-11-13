#!/usr/bin/env python3
"""Run bounded identity-aware smoke commands without leaving descendants."""

from __future__ import annotations

import os
import re
from pathlib import Path
import selectors
import signal
import subprocess
import time


OUTPUT_LIMIT = 1024 * 1024


class SmokeError(Exception):
    """A smoke command exceeded its contract."""


def group_exists(pid: int) -> bool:
    try:
        os.killpg(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def terminate_group(process: subprocess.Popen[bytes]) -> None:
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    try:
        process.kill()
    except ProcessLookupError:
        pass
    try:
        process.wait(timeout=1)
    except subprocess.TimeoutExpired:
        pass
    deadline = time.monotonic() + 1
    while group_exists(process.pid) and time.monotonic() < deadline:
        time.sleep(0.002)


def run_bounded(path: Path, arguments: list[str], timeout: float = 10) -> tuple[int, bytes, bytes]:
    process = subprocess.Popen(
        [str(path), *arguments],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
    )
    assert process.stdout is not None and process.stderr is not None
    selector = selectors.DefaultSelector()
    streams = {process.stdout.fileno(): bytearray(), process.stderr.fileno(): bytearray()}
    for stream in (process.stdout, process.stderr):
        os.set_blocking(stream.fileno(), False)
        selector.register(stream, selectors.EVENT_READ)
    deadline = time.monotonic() + timeout
    try:
        while selector.get_map():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise SmokeError(f"smoke check exceeded {timeout:g} seconds")
            for key, _ in selector.select(min(remaining, 0.05)):
                chunk = os.read(key.fd, 8192)
                if not chunk:
                    selector.unregister(key.fileobj)
                    continue
                streams[key.fd].extend(chunk)
                if sum(map(len, streams.values())) > OUTPUT_LIMIT:
                    raise SmokeError(f"smoke output exceeded {OUTPUT_LIMIT} bytes")
            process.poll()
        try:
            status = process.wait(timeout=max(0.001, deadline - time.monotonic()))
        except subprocess.TimeoutExpired as error:
            raise SmokeError(f"smoke check exceeded {timeout:g} seconds") from error
        if group_exists(process.pid):
            raise SmokeError("smoke command left a descendant process")
        return status, bytes(streams[process.stdout.fileno()]), bytes(streams[process.stderr.fileno()])
    except BaseException:
        terminate_group(process)
        raise
    finally:
        selector.close()
        process.stdout.close()
        process.stderr.close()


def smoke(path: Path) -> None:
    commands = [
        ("root help", ["--help"], rb"Usage: chigiri \[COMMAND\]"),
        ("root version", ["--version"], rb"\nchigiri [0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.+-]+)?\n"),
        ("Rust help", ["rust", "--help"], b"Usage: chigiri rust"),
        ("Rust version", ["rust", "--version"], rb"\nchigiri rust [0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.+-]+)?\n"),
        ("Mock help", ["mock", "--help"], b"Usage: chigiri mock"),
        ("Mock version", ["mock", "--version"], rb"\nchigiri mock [0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.+-]+)?\n"),
    ]
    for label, arguments, marker in commands:
        status, stdout, stderr = run_bounded(path, arguments)
        if status != 0 or stderr or re.search(marker, stdout) is None:
            raise SmokeError(f"{label} smoke check did not identify Chigiri")
        print(f"verify-dist: Passed: {label} ({path.parent.name})")
