"""Provide bounded production-workload metrics and RSS sampling."""

from __future__ import annotations

import subprocess
import threading
import time
from typing import Callable

LATENCY_BOUNDS_MS = (1, 2, 5, 10, 20, 50, 100, 250, 500, 1_000, 2_500, 5_000)
RSS_INTERVAL_SECONDS = 0.5


class LatencyHistogram:
    """Tracks request count and latency distribution in independent fixed buckets."""

    def __init__(self) -> None:
        self.counts = [0] * (len(LATENCY_BOUNDS_MS) + 1)
        self.total = 0

    def record(self, seconds: float) -> None:
        milliseconds = seconds * 1_000
        position = next(
            (
                index
                for index, bound in enumerate(LATENCY_BOUNDS_MS)
                if milliseconds <= bound
            ),
            len(LATENCY_BOUNDS_MS),
        )
        self.counts[position] += 1
        self.total += 1

    def percentile_ms(self, fraction: float) -> tuple[float | None, float | None]:
        target = max(1, int(self.total * fraction + 0.999999))
        seen = 0
        for index, count in enumerate(self.counts):
            seen += count
            if seen >= target:
                if index < len(LATENCY_BOUNDS_MS):
                    return float(LATENCY_BOUNDS_MS[index]), None
                return None, float(LATENCY_BOUNDS_MS[-1])
        return None, None

    def evidence(self) -> dict[str, object]:
        return {
            "upperBoundsMs": list(LATENCY_BOUNDS_MS),
            "bucketCounts": self.counts,
            "overflowIsGreaterThanMs": LATENCY_BOUNDS_MS[-1],
        }


class Results:
    """Record parallel-worker successes, categorized errors, and bounded latency."""

    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.latencies = LatencyHistogram()
        self.first_errors: list[str] = []
        self.error_counts: dict[str, int] = {}
        self.graphql_error_codes: dict[str, int] = {}
        self.queries = 0
        self.mutations = 0
        self.query_successes = 0
        self.mutation_successes = 0
        self.next_ticket = 0

    def ticket(self, soak: bool) -> bool:
        with self.lock:
            ticket = self.next_ticket
            self.next_ticket += 1
            mutation = soak and ticket % 10 == 9
            if mutation:
                self.mutations += 1
            else:
                self.queries += 1
            return mutation

    def record(
        self,
        latency: float,
        mutation: bool,
        error: tuple[str, str] | None,
        graphql_codes: list[str],
    ) -> None:
        with self.lock:
            self.latencies.record(latency)
            for code in graphql_codes:
                self.graphql_error_codes[code] = (
                    self.graphql_error_codes.get(code, 0) + 1
                )
            if error is None:
                if mutation:
                    self.mutation_successes += 1
                else:
                    self.query_successes += 1
                return
            category, detail = error
            self.error_counts[category] = self.error_counts.get(category, 0) + 1
            if len(self.first_errors) < 100:
                self.first_errors.append(detail)

    def error_total(self) -> int:
        return sum(self.error_counts.values())


class RssMonitor:
    """Collect serialized, timestamped RSS observations with best-effort spacing."""

    def __init__(self, pid: int, reader: Callable[[int], int] | None = None) -> None:
        self.pid = pid
        self.reader = reader or read_rss
        self.samples: list[dict[str, float | int]] = []
        self.errors: list[str] = []
        self.error_count = 0
        self.lock = threading.Lock()
        self.stop = threading.Event()
        self.read_attempts = 0
        self.read_total_seconds = 0.0
        self.read_max_seconds = 0.0
        self.started = time.monotonic()
        self.thread = threading.Thread(target=self._run, daemon=True)

    def observe(self) -> None:
        """Record the whole read interval so phase membership can be verified."""
        with self.lock:
            read_started = time.monotonic() - self.started
            self.read_attempts += 1
            try:
                value = self.reader(self.pid)
                if type(value) is not int or value <= 0:
                    raise ValueError("RSS must be a positive integer byte count")
                self.samples.append({
                    "readStartSeconds": read_started,
                    "readEndSeconds": time.monotonic() - self.started,
                    "rssBytes": value,
                })
            except (OSError, subprocess.SubprocessError, ValueError) as error:
                self.error_count += 1
                if len(self.errors) < 20:
                    self.errors.append(f"{type(error).__name__}: {error}")
            finally:
                read_seconds = time.monotonic() - self.started - read_started
                self.read_total_seconds += read_seconds
                self.read_max_seconds = max(self.read_max_seconds, read_seconds)

    def _run(self) -> None:
        # This interval is a collection target, not a scheduling deadline.
        while not self.stop.wait(RSS_INTERVAL_SECONDS):
            self.observe()

    def start(self) -> None:
        self.thread.start()

    def finish(self) -> dict[str, object]:
        self.stop.set()
        if self.thread.ident is not None:
            self.thread.join()
        with self.lock:
            samples = self.samples.copy()
            errors = self.errors.copy()
            error_count = self.error_count
        stopped = not self.thread.is_alive()
        return {
            "passed": len(samples) >= 2 and error_count == 0 and stopped,
            "samples": samples,
            "sampleCount": len(samples),
            "elapsedSeconds": time.monotonic() - self.started,
            "errorCount": error_count,
            "errors": errors,
            "threadStopped": stopped,
        }

    def timing_diagnostics(self) -> dict[str, object]:
        """Report read costs after finish without imposing a cadence gate."""
        return {
            "readerBackend": "ps",
            "targetIntervalSeconds": RSS_INTERVAL_SECONDS,
            "readAttempts": self.read_attempts,
            "readTotalSeconds": self.read_total_seconds,
            "readMaxSeconds": self.read_max_seconds,
        }


def read_rss(pid: int) -> int:
    result = subprocess.run(
        ["ps", "-o", "rss=", "-p", str(pid)],
        check=False,
        capture_output=True,
        text=True,
        timeout=5,
    )
    value = result.stdout.strip()
    if result.returncode != 0 or not value:
        raise OSError(result.stderr.strip() or "ps returned no RSS sample")
    return int(value) * 1024
