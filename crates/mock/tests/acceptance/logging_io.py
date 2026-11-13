"""Saturate real binary log pipes and verify HTTP continuity and writer-process recovery."""

import http.client
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import time


BINARY, CONFIG, ROOT = sys.argv[1:]
ROOT = Path(ROOT)


def absent(pid):
    for probe in (os.kill, os.killpg):
        try:
            probe(pid, 0)
        except ProcessLookupError:
            continue
        raise AssertionError(f"writer/process group remains: {pid}")


def children(parent):
    result = subprocess.run(
        ["ps", "-axo", "pid=,ppid=,comm="], capture_output=True, check=True, timeout=2
    )
    found = []
    for line in result.stdout.decode().splitlines():
        values = line.split(maxsplit=2)
        if len(values) == 3 and int(values[1]) == parent:
            assert "chigiri" in values[2] or values[2] == "<defunct>", values
            found.append(int(values[0]))
    return found


def pipe_output(closed=False, full=False):
    read, write = os.pipe()
    if closed:
        os.close(read)
        read = None
    if full:
        os.set_blocking(write, False)
        try:
            while True:
                os.write(write, b"stderr test filler\n" * 256)
        except BlockingIOError:
            pass
        try:
            while True:
                os.write(write, b"x")
        except BlockingIOError:
            pass
        os.set_blocking(write, True)
    return read, write


def request(port):
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=2)
    try:
        body = json.dumps({"query": "{ getUsers { id } }"})
        start = time.monotonic()
        connection.request("POST", "/graphql", body, {"Content-Type": "application/json"})
        response = connection.getresponse()
        content = json.loads(response.read())
        assert response.status == 200 and "errors" not in content, content
        assert content["data"]["getUsers"] == [{"id": "User-1"}, {"id": "User-2"}]
        assert time.monotonic() - start < 2
    finally:
        connection.close()


def drain(fd, output):
    if fd is None:
        return
    os.set_blocking(fd, False)
    # Bound how much output a producer can drain into memory.
    for _ in range(64):
        try:
            value = os.read(fd, 65536)
        except BlockingIOError:
            return
        if not value:
            return
        output.extend(value)
        assert len(output) <= 4 * 1024 * 1024


def records(output):
    return [json.loads(line) for line in output.split(b"\n")[:-1]]


def assert_loss_shape(losses):
    for value in losses:
        assert set(value["dropped"]) == {"hook", "access", "subscription", "notice"}, value
        # This retained finite-request scenario creates no subscriptions.
        assert value["dropped"]["subscription"] == 0, value


def scenario(mode):
    closed = mode in ("closed_stdout", "both_closed")
    both = mode.startswith("both_")
    read_out, write_out = pipe_output(closed=closed)
    read_err = None
    if both:
        read_err, error = pipe_output(closed=closed, full=not closed)
    else:
        error = os.open(ROOT / f"{mode}.stderr", os.O_CREAT | os.O_WRONLY | os.O_EXCL, 0o600)
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    config = ROOT / f"{mode}.toml"
    config.write_text(Path(CONFIG).read_text().replace("127.0.0.1:4000", f"127.0.0.1:{port}"))
    command = [BINARY, "mock", "serve", "--config", str(config)]
    if mode == "both_slow_response":
        command.append("--enable-graphiql")
    process = subprocess.Popen(
        command,
        stdin=subprocess.DEVNULL, stdout=write_out, stderr=error,
    )
    os.close(write_out)
    os.close(error)
    workers = []
    output = bytearray()
    partial = None
    try:
        deadline = time.monotonic() + 30
        while True:
            assert process.poll() is None, f"startup failed in {mode}"
            try:
                with socket.create_connection(("127.0.0.1", port), timeout=0.1):
                    break
            except OSError:
                assert time.monotonic() < deadline, "startup watchdog"
                time.sleep(0.01)
        while len(workers) != 2:
            workers = children(process.pid)
            assert time.monotonic() < deadline, "two writers did not start"
            time.sleep(0.01)
        for _ in range(5):
            request(port)
        if mode == "recover":
            deadline = time.monotonic() + 3
            while True:
                drain(read_out, output)
                lines = records(output)
                access = [value for value in lines if value["kind"] == "access"]
                losses = [value for value in lines if value["kind"] == "log_loss"]
                if len(access) == 5 and losses and losses[-1]["dropped"]["hook"] > 0:
                    break
                assert time.monotonic() < deadline, "reserved access/loss did not recover"
                time.sleep(0.005)
            assert len({value["request_id"] for value in access}) == 5
            assert all(value["status"] == 200 and value["outcome"] == "completed" for value in access)
            assert all(value["dropped"]["access"] == 0 for value in losses)
            assert_loss_shape(losses)
        # Saturated or recovering writers must not block readiness.
        request(port)
        if mode == "both_slow_input":
            partial = socket.create_connection(("127.0.0.1", port), timeout=2)
            partial.sendall(b"POST /graphql HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: 100\r\nExpect: 100-continue\r\n\r\n")
            assert b"100 Continue" in partial.recv(1024)
        if mode == "both_slow_response":
            partial = socket.socket()
            partial.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 1024)
            partial.settimeout(2)
            partial.connect(("127.0.0.1", port))
            request_bytes = b"GET /graphiql/assets/app.js HTTP/1.1\r\nHost: localhost\r\n\r\n"
            # One asset can fit in a host's autotuned socket buffers. Pipelining
            # enough responses keeps the connection pending on every Tier 1 host.
            partial.sendall(request_bytes * 32)
            assert partial.recv(1) == b"H"
            time.sleep(0.25)
        started = time.monotonic()
        process.send_signal(signal.SIGTERM)
        while process.poll() is None:
            if mode == "recover":
                drain(read_out, output)
            assert time.monotonic() - started < 5, f"shutdown watchdog: {mode}"
            time.sleep(0.005)
        elapsed = time.monotonic() - started
        expected_exit = 2 if mode == "both_slow_response" else 0
        assert process.returncode == expected_exit, (mode, process.returncode)
        if mode == "both_slow_input":
            assert b"503 Service Unavailable" in partial.recv(1024)
        for pid in workers:
            absent(pid)
        if mode == "recover":
            drain(read_out, output)
            assert output.endswith(b"\n")
            lines = records(output)
            assert all(value["format"] == "chigiri-mock-log/v5" for value in lines)
            access = [value for value in lines if value["kind"] == "access"]
            assert len(access) == 6
            losses = [value for value in lines if value["kind"] == "log_loss"]
            counts = [value["dropped"]["hook"] for value in losses]
            assert counts == sorted(counts) and counts[-1] > 0
            assert all(value["dropped"]["access"] == 0 for value in losses)
            assert_loss_shape(losses)
        if not both:
            notice = (ROOT / f"{mode}.stderr").read_text()
            assert "chigiri mock log loss: hook=" in notice
            # Harness failures and full-pipe notices must remain bounded.
            assert notice.count("chigiri mock log loss:") <= 8
        print(f"{mode}: HTTP 6 succeeded; two writers reaped; exit {process.returncode}; shutdown {elapsed:.3f}s", flush=True)
    finally:
        # Watchdog cleanup is not success and must not leave a test process behind.
        if process.poll() is None:
            for pid in children(process.pid):
                try:
                    os.killpg(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            process.kill()
            process.wait(timeout=2)
        if partial is not None:
            partial.close()
        for descriptor in (read_out, read_err):
            if descriptor is not None:
                os.close(descriptor)


for mode in ("recover", "slow_shutdown", "closed_stdout", "both_slow", "both_closed", "both_slow_input", "both_slow_response"):
    scenario(mode)
