"""Provide valid format-5 fixtures for performance-evidence regression tests."""

from __future__ import annotations

import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import performance_evidence
import performance_evidence_metadata
from production_workload_metrics import LATENCY_BOUNDS_MS


def identity(revision: str | None = None, digest: str = "b") -> dict[str, Any]:
    if revision is None:
        revision = performance_evidence_metadata.command_output(
            ["git", "rev-parse", "HEAD"]
        )
    return {
        "revision": revision,
        "revisionUnixSeconds": performance_evidence_metadata.revision_unix_seconds(
            revision
        ),
        "dirty": False,
        "binarySha256": digest * 64,
        "toolchain": {
            "version": "1.95.0",
            "cargo": "cargo 1.95.0 (test)\nrelease: 1.95.0",
            "rustc": "rustc 1.95.0 (test)\nrelease: 1.95.0",
        },
    }


def hardware() -> dict[str, object]:
    return performance_evidence_metadata.hardware_identity()


def rss() -> dict[str, object]:
    return {
        "peakRssBytes": 4096,
        "sampleCount": 10,
        "expectedSampleCount": 10,
        "coverage": 1.0,
        "errorCount": 0,
        "threadStopped": True,
    }


def overheads() -> list[dict[str, object]]:
    return [
        {
            "name": name,
            "disabledMedianNs": 1.0,
            "enabledMedianNs": 1.0,
            "overheadPercent": 0.0,
            "limitPercent": limit,
            "passed": True,
        }
        for name, limit in performance_evidence.EXPECTED_PROFILE_LIMITS.items()
    ]


def observer_queues(names: tuple[str, ...]) -> list[dict[str, object]]:
    expected = 31 * 100 * 29
    return [
        {
            "name": name,
            "capacity": 65_536,
            "lifecycleEventsPerRequest": 29,
            "expectedLifecycleEventCount": expected,
            "acceptedLifecycleEventCount": expected,
            "droppedLifecycleEventCount": 0,
            "acceptedErrorEventCount": 0,
            "droppedErrorEventCount": 0,
        }
        for name in names
    ]


def baseline(*, accepted: bool = False) -> dict[str, Any]:
    recorded_at = int(time.time())
    evidence = {
        "formatVersion": performance_evidence.PERFORMANCE_FORMAT_VERSION,
        "accepted": accepted,
        "acceptedBy": None,
        "acceptedAt": None,
        "acceptedAtUnixSeconds": None,
        "acceptedReason": None,
        "recordedAtUnixSeconds": recorded_at,
        "command": [
            "production-benchmark",
            "record",
            "--toolchain", "1.95.0",
            "--samples", "31",
            "--iterations", "100",
            "--output", "target/production-evidence/benchmark-baseline-unaccepted.json",
        ],
        "source": identity(),
        "hardware": hardware(),
        "samples": 31,
        "iterationsPerSample": 100,
        "profileOrderStrategy": performance_evidence.PROFILE_ORDER_STRATEGY,
        "profiles": [
            {
                "name": item["name"],
                "includes": list(item["includes"]),
                "excludes": list(item["excludes"]),
            }
            for item in performance_evidence.PROFILE_CONTRACTS
        ],
        "observerQueues": observer_queues(
            ("observer-enabled", "facade-composition-enabled")
        ),
        "diagnosticProfiles": [
            {
                "name": item["name"],
                "includes": list(item["includes"]),
                "excludes": list(item["excludes"]),
            }
            for item in performance_evidence.DIAGNOSTIC_PROFILE_CONTRACTS
        ],
        "diagnosticMeasurements": [
            {"name": name, "medianNs": 1.0}
            for name in performance_evidence.EXPECTED_DIAGNOSTIC_PROFILES
        ],
        "diagnosticObserverQueues": observer_queues(
            ("diagnostic-queue-only", "diagnostic-full-observer")
        ),
        "measurements": [
            {"name": name, "medianNs": 1.0}
            for name in performance_evidence.EXPECTED_MEASUREMENTS
        ],
        "overheads": overheads(),
        "rss": rss(),
    }
    if accepted:
        evidence.update(
            {
                "acceptedBy": "reviewer-123",
                "acceptedAt": datetime.fromtimestamp(
                    recorded_at, timezone.utc
                ).isoformat(),
                "acceptedAtUnixSeconds": recorded_at,
                "acceptedReason": "Binary digest, CPU, process isolation, and all medians were verified before initial baseline approval.",
            }
        )
    return evidence


def workload(kind: str = "load") -> dict[str, Any]:
    soak = kind != "load"
    requests = 100 if soak else 10_000
    mutations = requests // 10 if soak else 0
    queries = requests - mutations
    duration = 2 if kind == "short-soak" else 900 if soak else None
    counts = [requests] + [0] * len(LATENCY_BOUNDS_MS)
    return {
        "formatVersion": performance_evidence.WORKLOAD_FORMAT_VERSION,
        "mode": kind,
        "passed": True,
        "source": identity(),
        "recordedAtUnixSeconds": int(time.time()),
        "command": [
            "scripts/production-http-workload.py",
            kind,
            "--binary", str(Path("binary").resolve()),
            "--toolchain", "1.95.0",
            "--requests", "10000",
            "--concurrency", str(2 if kind == "short-soak" else 32 if soak else 64),
            "--duration", str(duration or 900),
            "--rss-baseline-seconds", "0.5" if kind == "short-soak" else "1.0",
            "--output", "target/production-evidence/" + (
                "harness-short-soak.json" if kind == "short-soak" else "soak.json" if soak else "load.json"
            ),
        ],
        "hardware": hardware(),
        "concurrency": 2 if kind == "short-soak" else 32 if soak else 64,
        "configuredRequests": None if soak else 10_000,
        "configuredDurationSeconds": duration,
        "warmupRequests": 256,
        "requests": requests,
        "queryAttempts": queries,
        "querySuccesses": queries,
        "mutationAttempts": mutations,
        "mutationSuccesses": mutations,
        "graphqlErrorCodes": {},
        "errorCount": 0,
        "errorCounts": {},
        "firstErrors": [],
        "elapsedSeconds": float(duration or 1),
        "workloadStartSeconds": 1.0,
        "workloadEndSeconds": 1.0 + float(duration or 1),
        "medianLatencyUpperBoundMs": 1.0,
        "medianLatencyGreaterThanMs": None,
        "p95LatencyUpperBoundMs": 1.0,
        "p95LatencyGreaterThanMs": None,
        "latencyHistogram": {
            "upperBoundsMs": list(LATENCY_BOUNDS_MS),
            "bucketCounts": counts,
            "overflowIsGreaterThanMs": LATENCY_BOUNDS_MS[-1],
        },
        "rssSampler": {
            "passed": True,
            "samples": [
                {"readStartSeconds": 0.5, "readEndSeconds": 0.51, "rssBytes": 4096},
                {"readStartSeconds": 1.1, "readEndSeconds": 1.11, "rssBytes": 4096},
            ],
            "sampleCount": 2,
            "elapsedSeconds": float(duration or 1) + 1.0,
            "errorCount": 0,
            "errors": [],
            "threadStopped": True,
        },
        "rssBaselineSampleCount": 1,
        "rssBaselineStartIndex": 0,
        "rssBaselineEndIndex": 1,
        "initialSteadyRssBytes": 4096,
        "peakRssBytes": 4096,
        "steadyMemoryGrowthBytes": 0,
        "readiness": {"applicationInFlight": 0, "endpointInFlight": 0},
        "serverExit": 0,
        "totalHarnessSeconds": float(duration or 1) + 1.0,
    }


def benchmark(baseline_evidence: dict[str, Any]) -> dict[str, Any]:
    return {
        "formatVersion": performance_evidence.PERFORMANCE_FORMAT_VERSION,
        "passed": True,
        "recordedAtUnixSeconds": max(
            int(time.time()), baseline_evidence["recordedAtUnixSeconds"]
        ),
        "baselinePath": "baseline.json",
        "comparisonMode": "initial-candidate",
        "baselineSource": baseline_evidence["source"],
        "source": baseline_evidence["source"],
        "hardware": baseline_evidence["hardware"],
        "command": [
            "production-benchmark",
            "compare",
            "--toolchain", "1.95.0",
            "--samples", "31",
            "--iterations", "100",
            "--comparison-mode", "initial-candidate",
            "--baseline", "baseline.json",
            "--output", "target/production-evidence/benchmark.json",
        ],
        "samples": 31,
        "iterationsPerSample": 100,
        "rss": rss(),
        "profileOrderStrategy": performance_evidence.PROFILE_ORDER_STRATEGY,
        "profiles": baseline_evidence["profiles"],
        "observerQueues": baseline_evidence["observerQueues"],
        "diagnosticProfiles": baseline_evidence["diagnosticProfiles"],
        "diagnosticMeasurements": baseline_evidence["diagnosticMeasurements"],
        "diagnosticObserverQueues": baseline_evidence["diagnosticObserverQueues"],
        "metrics": [
            {
                "name": name,
                "baselineMedianNs": 1.0,
                "candidateMedianNs": 1.0,
                "regressionPercent": 0.0,
                "limitPercent": performance_evidence.MEASUREMENT_LIMITS[name],
                "passed": True,
            }
            for name in performance_evidence.EXPECTED_MEASUREMENTS
        ],
        "overheads": overheads(),
    }
