#!/usr/bin/env python3
"""Verify the source attribution of performance evidence and the accepted baseline contract."""

from __future__ import annotations

import math
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any

from performance_evidence_metadata import (
    EvidenceError,
    binary_sha256,
    require_command,
    require_exact_keys,
    require_hardware,
    require_identity,
    require_immutable_toolchain,
    require_recorded_time,
    require_source_identity,
    source_identity,
)
from performance_observer_evidence import (
    DIAGNOSTIC_PROFILE_CONTRACTS,
    DIAGNOSTIC_QUEUE_PROFILES,
    EXPECTED_DIAGNOSTIC_PROFILES,
    LIFECYCLE_EVENTS_PER_REQUEST,
    OBSERVER_QUEUE_CAPACITY,
    OBSERVER_QUEUE_PROFILES,
    require_diagnostic_measurements,
    require_observer_queues,
)

PERFORMANCE_FORMAT_VERSION = 5
WORKLOAD_FORMAT_VERSION = 6
PROFILE_ORDER_STRATEGY = "alternating-forward-reverse"
EXPECTED_MEASUREMENTS = ("parse", "validate", "cache-hit", "execute", "policy-disabled")
MEASUREMENT_LIMITS = {
    "parse": 15.0,
    "validate": 15.0,
    "cache-hit": 15.0,
    "execute": 15.0,
    "policy-disabled": 5.0,
}
PROFILE_CONTRACTS = (
    {
        "name": "policy-disabled",
        "includes": ["neutral endpoint", "document cache"],
        "excludes": ["operation", "rate", "observer"],
    },
    {
        "name": "operation-enabled",
        "includes": ["safelist catalog digest lookup"],
        "excludes": ["rate", "observer"],
    },
    {
        "name": "rate-enabled",
        "includes": ["Stage A token bucket", "Stage B cost/token bucket"],
        "excludes": ["operation source", "observer"],
    },
    {
        "name": "observer-enabled",
        "includes": ["request/error ID", "sampling", "bounded observer queue"],
        "excludes": ["operation source", "rate"],
    },
    {
        "name": "facade-composition-enabled",
        "includes": [
            "Endpoint component graph composition",
            "safelist catalog digest lookup",
            "Stage A and Stage B token buckets",
            "request/error ID and bounded observer queue",
        ],
        "excludes": ["external network provider", "backend exporter worker"],
    },
)
EXPECTED_PROFILES = tuple(item["name"] for item in PROFILE_CONTRACTS)
EXPECTED_PROFILE_LIMITS = {
    "operation-enabled": 25.0,
    "rate-enabled": 25.0,
    "observer-enabled": 25.0,
    "facade-composition-enabled": 50.0,
}
GENERIC_REVIEWERS = {"release-owner", "owner", "reviewer", "unknown"}
OLD_TEMPLATE_REASON = "The same hardware/toolchain, no competing process, approved as the first production baseline."


def require_accepted_baseline(
    evidence: dict[str, Any],
    binary: Path,
    channel: str,
    comparison_mode: str,
    baseline_binary: Path | None = None,
) -> dict[str, Any]:
    version = positive_integer(evidence.get("formatVersion"), "format version")
    if version != PERFORMANCE_FORMAT_VERSION or evidence.get("accepted") is not True:
        raise EvidenceError("accepted formatVersion 5 baseline is required")
    require_baseline_contract(evidence)
    require_approval(evidence)
    candidate = source_identity(binary, channel)
    require_source_identity(candidate)
    baseline_source = evidence["source"]
    if baseline_source["toolchain"] != candidate["toolchain"]:
        raise EvidenceError("baseline and candidate toolchains do not match")
    if comparison_mode == "initial-candidate":
        if baseline_source != candidate:
            raise EvidenceError("initial baseline and candidate identities do not match")
    elif comparison_mode == "previous-release":
        if baseline_source["revision"] == candidate["revision"]:
            raise EvidenceError("previous release baseline must use a different revision")
        if baseline_binary is None:
            raise EvidenceError("previous release comparison requires its immutable binary")
        if binary_sha256(baseline_binary.resolve()) != baseline_source["binarySha256"]:
            raise EvidenceError("previous release baseline binary digest does not match")
    else:
        raise EvidenceError("unknown baseline comparison mode")
    require_hardware(evidence["hardware"], current=True)
    return candidate


def require_baseline_toolchain(evidence: dict[str, Any], version: str) -> None:
    """Compare the baseline accepted before build with the immutable compiler specification."""
    version = require_immutable_toolchain(version)
    format_version = positive_integer(evidence.get("formatVersion"), "format version")
    if format_version != PERFORMANCE_FORMAT_VERSION or evidence.get("accepted") is not True:
        raise EvidenceError("accepted formatVersion 5 baseline is required")
    require_baseline_contract(evidence)
    require_approval(evidence)
    require_hardware(evidence["hardware"], current=True)
    if evidence["source"]["toolchain"]["version"] != version:
        raise EvidenceError("requested compiler does not match baseline immutable toolchain")


def require_baseline_contract(evidence: dict[str, Any]) -> None:
    """Validate profile, measurement, and RSS records in a fixed baseline."""
    require_exact_keys(
        evidence,
        {
            "formatVersion", "accepted", "acceptedBy", "acceptedAt",
            "acceptedAtUnixSeconds", "acceptedReason", "recordedAtUnixSeconds",
            "command", "source",
            "hardware", "samples", "iterationsPerSample", "rss",
            "profileOrderStrategy", "profiles", "observerQueues",
            "diagnosticProfiles", "diagnosticMeasurements",
            "diagnosticObserverQueues", "measurements", "overheads",
        },
        "baseline evidence",
    )
    require_recorded_time(
        evidence.get("recordedAtUnixSeconds"), evidence.get("source")
    )
    require_source_identity(evidence.get("source"))
    require_hardware(evidence.get("hardware"), current=False)
    require_benchmark_command(
        evidence.get("command"), "record", evidence["source"], 31, 100
    )
    if evidence.get("profiles") != list(PROFILE_CONTRACTS):
        raise EvidenceError("baseline profile include/exclude contract does not match")
    if evidence.get("diagnosticProfiles") != list(DIAGNOSTIC_PROFILE_CONTRACTS):
        raise EvidenceError("diagnostic profile include/exclude contract does not match")
    format_version = positive_integer(evidence.get("formatVersion"), "format version")
    samples = positive_integer(evidence.get("samples"), "baseline sample count")
    iterations = positive_integer(
        evidence.get("iterationsPerSample"), "baseline iteration count"
    )
    if (
        format_version != PERFORMANCE_FORMAT_VERSION
        or samples != 31
        or iterations != 100
        or evidence.get("profileOrderStrategy") != PROFILE_ORDER_STRATEGY
    ):
        raise EvidenceError("baseline measurement conditions do not match")
    measurements = evidence.get("measurements")
    if not isinstance(measurements, list) or not all(isinstance(item, dict) for item in measurements):
        raise EvidenceError("baseline measurements are malformed")
    if [item.get("name") for item in measurements] != list(EXPECTED_MEASUREMENTS):
        raise EvidenceError("baseline measurement set does not match")
    medians = {
        item["name"]: _measurement_median(item)
        for item in measurements
    }
    require_diagnostic_measurements(evidence.get("diagnosticMeasurements"))
    require_observer_queues(
        evidence.get("observerQueues"), samples, iterations, OBSERVER_QUEUE_PROFILES
    )
    require_observer_queues(
        evidence.get("diagnosticObserverQueues"),
        samples,
        iterations,
        DIAGNOSTIC_QUEUE_PROFILES,
    )
    require_benchmark_rss(evidence.get("rss"))
    require_overheads(evidence.get("overheads"), medians["policy-disabled"], True)


def require_benchmark_rss(value: object) -> None:
    require_exact_keys(
        value,
        {"peakRssBytes", "sampleCount", "expectedSampleCount", "coverage", "errorCount", "threadStopped"},
        "benchmark RSS evidence",
    )
    assert isinstance(value, dict)
    peak = positive_integer(value.get("peakRssBytes"), "peak RSS")
    samples = positive_integer(value.get("sampleCount"), "RSS sample count")
    expected = positive_integer(value.get("expectedSampleCount"), "RSS attempt count")
    errors = nonnegative_integer(value.get("errorCount"), "RSS error count")
    coverage = finite_number(value.get("coverage"), "RSS coverage")
    calculated = samples / expected
    if peak <= 0 or samples > expected or errors != 0 or value.get("threadStopped") is not True:
        raise EvidenceError("benchmark RSS sampling did not complete without errors")
    if not close(coverage, calculated) or coverage != 1.0:
        raise EvidenceError("benchmark RSS coverage does not match raw sample counts")


def require_overheads(value: object, disabled: float, require_pass: bool) -> bool:
    if not isinstance(value, list) or not all(isinstance(item, dict) for item in value):
        raise EvidenceError("enabled profile measurements are malformed")
    if [item.get("name") for item in value] != list(EXPECTED_PROFILE_LIMITS):
        raise EvidenceError("enabled profile measurements are missing")
    all_passed = True
    for item in value:
        require_exact_keys(
            item,
            {"name", "disabledMedianNs", "enabledMedianNs", "overheadPercent", "limitPercent", "passed"},
            "benchmark overhead",
        )
        name = item["name"]
        base = positive_number(item.get("disabledMedianNs"), "disabled median")
        enabled = positive_number(item.get("enabledMedianNs"), "enabled median")
        reported = finite_number(item.get("overheadPercent"), "overhead percent")
        limit = finite_number(item.get("limitPercent"), "overhead limit")
        calculated = percent(enabled, base)
        passed = calculated <= EXPECTED_PROFILE_LIMITS[name]
        if not close(base, disabled) or limit != EXPECTED_PROFILE_LIMITS[name]:
            raise EvidenceError("enabled profile baseline or limit does not match")
        if not close(reported, calculated) or item.get("passed") is not passed:
            raise EvidenceError("enabled profile overhead was not derived from medians")
        all_passed = all_passed and passed
    if require_pass and not all_passed:
        raise EvidenceError("enabled profile exceeded its fixed threshold")
    return all_passed


def require_benchmark_command(
    value: object,
    mode: str,
    source: dict[str, Any],
    samples: int,
    iterations: int,
    comparison_mode: str | None = None,
    baseline_path: Path | None = None,
    baseline_binary: Path | None = None,
) -> dict[str, str]:
    if mode == "record":
        flags = ("--toolchain", "--samples", "--iterations", "--output")
    elif comparison_mode == "previous-release":
        flags = (
            "--toolchain", "--samples", "--iterations", "--comparison-mode",
            "--baseline", "--baseline-binary", "--output",
        )
    else:
        flags = (
            "--toolchain", "--samples", "--iterations", "--comparison-mode",
            "--baseline", "--output",
        )
    arguments = require_command(value, "production-benchmark", mode, flags)
    if arguments["--toolchain"] != source["toolchain"]["version"]:
        raise EvidenceError("benchmark command toolchain does not match source identity")
    if _command_integer(arguments["--samples"], "benchmark samples") != samples:
        raise EvidenceError("benchmark command sample count does not match evidence")
    if _command_integer(arguments["--iterations"], "benchmark iterations") != iterations:
        raise EvidenceError("benchmark command iteration count does not match evidence")
    if mode == "record":
        if Path(arguments["--output"]).name != "benchmark-baseline-unaccepted.json":
            raise EvidenceError("baseline command output name does not match")
        return arguments
    if arguments["--comparison-mode"] != comparison_mode:
        raise EvidenceError("benchmark command comparison mode does not match")
    if baseline_path is not None and Path(arguments["--baseline"]).name != baseline_path.name:
        raise EvidenceError("benchmark command baseline path does not match verifier input")
    if Path(arguments["--output"]).name != "benchmark.json":
        raise EvidenceError("benchmark command output name does not match")
    if comparison_mode == "previous-release":
        assert baseline_binary is not None
        if Path(arguments["--baseline-binary"]).name != baseline_binary.name:
            raise EvidenceError("benchmark command baseline binary does not match verifier input")
    return arguments


def _measurement_median(item: dict[str, Any]) -> float:
    require_exact_keys(item, {"name", "medianNs"}, "baseline measurement")
    return positive_number(item.get("medianNs"), "baseline median")


def _command_integer(value: str, label: str) -> int:
    try:
        parsed = int(value)
    except ValueError as error:
        raise EvidenceError(f"{label} command value is not an integer") from error
    if str(parsed) != value or parsed <= 0:
        raise EvidenceError(f"{label} command value is not canonical")
    return parsed


def require_approval(evidence: dict[str, Any]) -> None:
    """Require a specific, attributable baseline approval."""
    reviewer = evidence.get("acceptedBy")
    reason = evidence.get("acceptedReason")
    accepted_at = evidence.get("acceptedAt")
    if (
        not isinstance(reviewer, str)
        or not reviewer.strip()
        or reviewer.strip().lower() in GENERIC_REVIEWERS
    ):
        raise EvidenceError("baseline reviewer is not individually auditable")
    if (
        not isinstance(reason, str)
        or len(reason.strip()) < 40
        or reason.strip() == OLD_TEMPLATE_REASON
    ):
        raise EvidenceError("baseline acceptance reason is not specific")
    try:
        timestamp = datetime.fromisoformat(accepted_at)
    except (TypeError, ValueError) as error:
        raise EvidenceError("baseline acceptedAt is not an ISO-8601 timestamp") from error
    if timestamp.utcoffset() != timedelta(0):
        raise EvidenceError("baseline acceptedAt must include the UTC offset")
    recorded_at = require_recorded_time(
        evidence.get("recordedAtUnixSeconds"), evidence.get("source")
    )
    accepted_unix = int(timestamp.timestamp())
    require_recorded_time(accepted_unix)
    recorded_accepted_unix = positive_integer(
        evidence.get("acceptedAtUnixSeconds"), "baseline acceptance timestamp"
    )
    canonical = datetime.fromtimestamp(recorded_accepted_unix, timezone.utc).isoformat()
    if recorded_accepted_unix != accepted_unix or accepted_at != canonical:
        raise EvidenceError("baseline acceptedAt fields do not identify the same instant")
    if accepted_unix < recorded_at:
        raise EvidenceError("baseline acceptedAt predates the recorded measurement")
    if accepted_unix > recorded_at + 86_400:
        raise EvidenceError("baseline acceptance is more than 24 hours after measurement")


def finite_number(value: object, label: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise EvidenceError(f"{label} must be a finite number")
    number = float(value)
    if not math.isfinite(number):
        raise EvidenceError(f"{label} must be a finite number")
    return number


def positive_number(value: object, label: str) -> float:
    number = finite_number(value, label)
    if number <= 0:
        raise EvidenceError(f"{label} must be positive")
    return number


def positive_integer(value: object, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
        raise EvidenceError(f"{label} must be a positive integer")
    return value


def nonnegative_integer(value: object, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise EvidenceError(f"{label} must be a non-negative integer")
    return value


def percent(candidate: float, baseline: float) -> float:
    return (candidate / baseline - 1.0) * 100.0


def close(left: float, right: float) -> bool:
    return math.isclose(left, right, rel_tol=1e-12, abs_tol=1e-9)
