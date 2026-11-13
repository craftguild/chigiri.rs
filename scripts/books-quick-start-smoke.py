#!/usr/bin/env python3
"""Verify README requests and application-owned draining against a real server."""

from __future__ import annotations

import argparse
import json
import signal
import socket
import subprocess
import time
from pathlib import Path


def free_port() -> int:
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def parse_response(response: bytes) -> tuple[int, dict[str, object]]:
    head, body = response.split(b"\r\n\r\n", 1)
    status = int(head.splitlines()[0].split()[1])
    return status, json.loads(body)


def raw_request(port: int, method: str, target: str, body: bytes | None) -> tuple[int, dict[str, object]]:
    headers = [
        f"{method} {target} HTTP/1.1",
        "Host: localhost",
        "Connection: close",
        "Accept: application/graphql-response+json",
    ]
    if method == "GET":
        headers.append("X-Chigiri-Preflight: 1")
    if body is not None:
        headers.extend(("Content-Type: application/json", f"Content-Length: {len(body)}"))
    request = "\r\n".join(headers).encode() + b"\r\n\r\n" + (body or b"")
    with socket.create_connection(("127.0.0.1", port), timeout=3) as stream:
        stream.settimeout(3)
        stream.sendall(request)
        response = bytearray()
        while chunk := stream.recv(65536):
            response.extend(chunk)
    return parse_response(bytes(response))


def wait_ready(port: int, expected_in_flight: int = 0) -> dict[str, object]:
    last_error: Exception | None = None
    for _ in range(200):
        try:
            status, response = raw_request(port, "GET", "/readiness", None)
            if status == 200 and response.get("applicationInFlight") == expected_in_flight:
                return response
        except (OSError, ValueError, json.JSONDecodeError) as error:
            last_error = error
        time.sleep(0.025)
    raise RuntimeError(f"readiness timeout: {last_error}")


def assert_equal(actual: object, expected: object, label: str) -> None:
    if actual != expected:
        raise RuntimeError(f"{label} mismatch: {actual!r}")


def drain_active_request(server: subprocess.Popen[str], port: int) -> dict[str, object]:
    body = b'{"query":"{ books { id } }"}'
    headers = (
        b"POST /graphql HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n"
        b"Content-Type: application/json\r\nAccept: application/graphql-response+json\r\n"
        + f"Content-Length: {len(body)}\r\n\r\n".encode()
    )
    with socket.create_connection(("127.0.0.1", port), timeout=3) as stream:
        stream.settimeout(5)
        midpoint = len(body) // 2
        stream.sendall(headers + body[:midpoint])
        wait_ready(port, 1)
        server.send_signal(signal.SIGINT)
        time.sleep(0.1)
        if server.poll() is not None:
            raise RuntimeError("The server shut down before the active request was completed.")
        stream.sendall(body[midpoint:])
        response = bytearray()
        while chunk := stream.recv(65536):
            response.extend(chunk)
    status, payload = parse_response(bytes(response))
    assert_equal(status, 200, "drained request status")
    server.wait(timeout=10)
    assert_equal(server.returncode, 0, "server exit")
    return payload


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    port = free_port()
    server = subprocess.Popen(
        [args.binary, f"127.0.0.1:{port}"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    evidence: dict[str, object] = {}
    try:
        initial_readiness = wait_ready(port)
        assert_equal(
            initial_readiness,
            {"applicationInFlight": 0, "endpointInFlight": 0},
            "initial readiness",
        )
        evidence["initialReadiness"] = initial_readiness
        query = b'{"query":"{ books { id title author } }"}'
        query_status, query_response = raw_request(port, "POST", "/graphql", query)
        assert_equal(query_status, 200, "POST Query status")
        assert_equal(query_response, {
            "data": {"books": [{"id": "1", "title": "The Left Hand of Darkness", "author": "Ursula K. Le Guin"}]},
            "extensions": {"requestCount": 1},
        }, "POST Query response")

        mutation = (
            b'{"query":"mutation Add($input:AddBookInput!){addBook(input:$input){id title author}}",'
            b'"operationName":"Add","variables":{"input":{"title":"A Wizard of Earthsea",'
            b'"author":"Ursula K. Le Guin"}}}'
        )
        mutation_status, mutation_response = raw_request(port, "POST", "/graphql", mutation)
        assert_equal(mutation_status, 200, "Mutation status")
        assert_equal(mutation_response, {
            "data": {"addBook": {"id": "2", "title": "A Wizard of Earthsea", "author": "Ursula K. Le Guin"}},
            "extensions": {"requestCount": 2},
        }, "Mutation response")

        target = "/graphql?query=%7Bbooks%7Btitle%7D%7D"
        get_status, get_response = raw_request(port, "GET", target, None)
        assert_equal(get_status, 200, "GET Query status")
        assert_equal(get_response, {
            "data": {"books": [{"title": "The Left Hand of Darkness"}, {"title": "A Wizard of Earthsea"}]},
            "extensions": {"requestCount": 3},
        }, "GET Query response")
        evidence.update(
            {
                "postQuery": query_response,
                "mutation": mutation_response,
                "getQuery": get_response,
                "drainedRequest": drain_active_request(server, port),
                "serverExit": server.returncode,
                "passed": True,
            }
        )
        assert_equal(evidence["drainedRequest"], {
            "data": {"books": [{"id": "1"}, {"id": "2"}]},
            "extensions": {"requestCount": 4},
        }, "drained request response")
    finally:
        if server.poll() is None:
            server.send_signal(signal.SIGINT)
            try:
                server.wait(timeout=3)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait()

    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(evidence, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(evidence, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
