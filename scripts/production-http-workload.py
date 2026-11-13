#!/usr/bin/env python3
"""Run a fixed load/soak workload against the books binary and emit attributable evidence."""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path
from typing import Callable

from performance_evidence import WORKLOAD_FORMAT_VERSION
from performance_evidence_metadata import (
    EvidenceError,
    hardware_identity,
    source_identity,
    strict_json_loads,
)
from production_workload_metrics import LATENCY_BOUNDS_MS, Results, RssMonitor

QUERY_DOCUMENT = "query WorkloadQuery { books { id title author } }"
MUTATION_DOCUMENT = (
    "mutation WorkloadMutation($id: ID!, $input: AddBookInput!) { "
    "replaceBook(id: $id, input: $input) { id title author } }"
)
QUERY = json.dumps({"query": QUERY_DOCUMENT}, separators=(",", ":")).encode()
MUTATION = json.dumps(
    {
        "query": MUTATION_DOCUMENT,
        "operationName": "WorkloadMutation",
        "variables": {
            "id": "1",
            "input": {"title": "Soak", "author": "Chigiri"},
        },
    },
    separators=(",", ":"),
).encode()
MAX_MEMORY_GROWTH = 64 * 1024 * 1024
WARMUP_REQUESTS = 256
INITIAL_BOOK = {
    "id": "1",
    "title": "The Left Hand of Darkness",
    "author": "Ursula K. Le Guin",
}
SOAK_BOOK = {"id": "1", "title": "Soak", "author": "Chigiri"}


def raw_request(port: int, path: str, body: bytes | None) -> tuple[int, bytes]:
    if body is None:
        request = f"GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n".encode()
    else:
        request = (
            f"POST {path} HTTP/1.1\r\nHost: localhost\r\n"
            "Content-Type: application/json\r\n"
            "Accept: application/graphql-response+json\r\n"
            f"Content-Length: {len(body)}\r\nConnection: close\r\n\r\n"
        ).encode() + body
    with socket.create_connection(("127.0.0.1", port), timeout=5) as stream:
        stream.settimeout(5)
        stream.sendall(request)
        response = bytearray()
        while chunk := stream.recv(65536):
            response.extend(chunk)
    head, payload = bytes(response).split(b"\r\n\r\n", 1)
    status = int(head.splitlines()[0].split()[1])
    return status, payload


def response_result(
    status: int, body: bytes, mutation: bool, soak: bool
) -> tuple[tuple[str, str] | None, list[str]]:
    if status != 200:
        return ("http", f"unexpected HTTP status {status}"), []
    try:
        response = strict_json_loads(body)
    except (UnicodeDecodeError, json.JSONDecodeError, EvidenceError) as error:
        return ("malformed-response", f"response JSON decode failed: {error}"), []
    if not isinstance(response, dict):
        return ("malformed-response", "response JSON root was not an object"), []
    if "errors" in response:
        errors = response["errors"]
        if not isinstance(errors, list) or not errors or not all(
            isinstance(error, dict) for error in errors
        ):
            return ("malformed-response", "GraphQL errors was not a non-empty object list"), []
        codes: list[str] = []
        for error in errors:
            message = error.get("message")
            if not isinstance(message, str) or not message:
                return ("malformed-response", "GraphQL error message was invalid"), []
            if "extensions" not in error:
                codes.append("UNCLASSIFIED")
                continue
            extensions = error["extensions"]
            if not isinstance(extensions, dict):
                return ("malformed-response", "GraphQL error extensions was not an object"), []
            code = extensions.get("code", "UNCLASSIFIED")
            if not isinstance(code, str) or not code:
                return ("malformed-response", "GraphQL error code was not a non-empty string"), []
            codes.append(code)
        return ("graphql", f"GraphQL errors: {codes or ['UNCLASSIFIED']}"), codes
    data = response.get("data")
    if mutation:
        valid = isinstance(data, dict) and data.get("replaceBook") == SOAK_BOOK
    else:
        books = data.get("books") if isinstance(data, dict) else None
        valid = books == [INITIAL_BOOK] or (soak and books == [SOAK_BOOK])
    if not valid:
        return ("unexpected-data", "GraphQL data did not match workload contract"), []
    return None, []


def one_request(port: int, results: Results, soak: bool) -> None:
    mutation = results.ticket(soak)
    started = time.perf_counter()
    try:
        status, body = raw_request(port, "/graphql", MUTATION if mutation else QUERY)
        error, codes = response_result(status, body, mutation, soak)
    except (OSError, ValueError) as caught:
        error = ("transport", f"transport error: {type(caught).__name__}: {caught}")
        codes = []
    results.record(time.perf_counter() - started, mutation, error, codes)


def free_port() -> int:
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def wait_ready(port: int) -> dict[str, int]:
    last_error: Exception | None = None
    for _ in range(200):
        try:
            status, body = raw_request(port, "/readiness", None)
            if status == 200:
                return json.loads(body)
        except (OSError, ValueError, json.JSONDecodeError) as error:
            last_error = error
        time.sleep(0.05)
    raise RuntimeError(f"server readiness timeout: {last_error}")


def run_load(
    port: int, count: int, concurrency: int, results: Results,
    observe: Callable[[], None],
) -> None:
    def worker(index: int) -> None:
        one_request(port, results, False)
        if index == 0:
            observe()

    with concurrent.futures.ThreadPoolExecutor(max_workers=concurrency) as pool:
        list(pool.map(worker, range(count)))


def run_soak(
    port: int, duration: int, concurrency: int, results: Results,
    observe: Callable[[], None],
) -> None:
    deadline = time.monotonic() + duration

    def worker(index: int) -> None:
        first = True
        while time.monotonic() < deadline:
            one_request(port, results, True)
            if index == 0 and first:
                observe()
                first = False

    with concurrent.futures.ThreadPoolExecutor(max_workers=concurrency) as pool:
        list(pool.map(worker, range(concurrency)))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("load", "soak", "short-soak"))
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--toolchain", required=True)
    parser.add_argument("--requests", type=int, default=10000)
    parser.add_argument("--concurrency", type=int, required=True)
    parser.add_argument("--duration", type=int, default=900)
    parser.add_argument("--rss-baseline-seconds", type=float, default=1.0)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    short = args.mode == "short-soak"
    if short and (args.concurrency, args.duration, args.rss_baseline_seconds) != (2, 2, 0.5):
        parser.error("short-soak is the fixed harness of two parallel//2/0.5 second baseline.")

    source = source_identity(args.binary, args.toolchain)
    recorded_at = int(time.time())
    port = free_port()
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    server_log = output.with_suffix(".server.log")
    with server_log.open("w", encoding="utf-8") as log:
        server = subprocess.Popen(
            [str(args.binary), f"127.0.0.1:{port}"], stdout=log, stderr=log
        )
        monitor = RssMonitor(server.pid)
        results = Results()
        started = time.monotonic()
        readiness: dict[str, int] = {}
        elapsed = 0.0
        baseline_start, baseline_end = 0, 1
        workload_start = workload_end = 0.0
        try:
            wait_ready(port)
            with concurrent.futures.ThreadPoolExecutor(max_workers=args.concurrency) as pool:
                list(
                    pool.map(
                        lambda _: one_request(port, results, False),
                        range(WARMUP_REQUESTS),
                    )
                )
            time.sleep(args.rss_baseline_seconds)
            monitor.observe()
            results = Results()
            workload_start = time.monotonic() - monitor.started
            monitor.start()
            try:
                if args.mode == "load":
                    run_load(port, args.requests, args.concurrency, results, monitor.observe)
                else:
                    run_soak(port, args.duration, args.concurrency, results, monitor.observe)
            finally:
                workload_end = time.monotonic() - monitor.started
                elapsed = workload_end - workload_start
            readiness = wait_ready(port)
        finally:
            rss = monitor.finish()
            if server.poll() is None:
                server.send_signal(signal.SIGINT)
            try:
                server.wait(timeout=30)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait()
            if not rss["passed"]:
                print(json.dumps({"rssSamplingDiagnostics": monitor.timing_diagnostics()}),
                      file=sys.stderr)

    observations = rss["samples"]
    baseline_samples = [sample for sample in observations[:1]
                        if sample["readEndSeconds"] <= workload_start]
    during_samples = [sample for sample in observations[baseline_end:]
                      if workload_start <= sample["readStartSeconds"] < workload_end
                      and sample["readStartSeconds"]
                      <= sample["readEndSeconds"] <= workload_end]
    baseline_rss = baseline_samples[0]["rssBytes"] if baseline_samples else 0
    peak_rss = max((sample["rssBytes"] for sample in observations), default=0)
    growth = max(0, peak_rss - baseline_rss)
    ratio_ok = results.mutations == results.latencies.total // 10
    successes_ok = results.query_successes == results.queries and (
        args.mode == "load"
        or (results.mutation_successes == results.mutations and results.mutation_successes > 0)
    )
    passed = (
        (short or source["dirty"] is False)
        and results.error_total() == 0
        and successes_ok
        and growth <= MAX_MEMORY_GROWTH
        and rss["passed"] is True
        and len(baseline_samples) == 1
        and len(during_samples) >= 1
        and readiness == {"applicationInFlight": 0, "endpointInFlight": 0}
        and server.returncode == 0
        and (args.mode != "load" or results.latencies.total == args.requests)
        and (args.mode == "load" or ratio_ok)
    )
    median_upper, median_greater = results.latencies.percentile_ms(0.5)
    p95_upper, p95_greater = results.latencies.percentile_ms(0.95)
    evidence = {
        "formatVersion": WORKLOAD_FORMAT_VERSION,
        "mode": args.mode,
        "passed": passed,
        "source": source,
        "recordedAtUnixSeconds": recorded_at,
        "command": sys.argv,
        "hardware": hardware_identity(),
        "concurrency": args.concurrency,
        "configuredRequests": args.requests if args.mode == "load" else None,
        "configuredDurationSeconds": args.duration if args.mode != "load" else None,
        "warmupRequests": WARMUP_REQUESTS,
        "requests": results.latencies.total,
        "queryAttempts": results.queries,
        "querySuccesses": results.query_successes,
        "mutationAttempts": results.mutations,
        "mutationSuccesses": results.mutation_successes,
        "graphqlErrorCodes": results.graphql_error_codes,
        "errorCount": results.error_total(),
        "errorCounts": results.error_counts,
        "firstErrors": results.first_errors,
        "elapsedSeconds": elapsed,
        "workloadStartSeconds": workload_start,
        "workloadEndSeconds": workload_end,
        "medianLatencyUpperBoundMs": median_upper,
        "medianLatencyGreaterThanMs": median_greater,
        "p95LatencyUpperBoundMs": p95_upper,
        "p95LatencyGreaterThanMs": p95_greater,
        "latencyHistogram": results.latencies.evidence(),
        "rssSampler": rss,
        "rssBaselineSampleCount": len(baseline_samples),
        "rssBaselineStartIndex": baseline_start,
        "rssBaselineEndIndex": baseline_end,
        "initialSteadyRssBytes": baseline_rss,
        "peakRssBytes": peak_rss,
        "steadyMemoryGrowthBytes": growth,
        "readiness": readiness,
        "serverExit": server.returncode,
        "totalHarnessSeconds": time.monotonic() - started,
    }
    output.write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(evidence, indent=2))
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
