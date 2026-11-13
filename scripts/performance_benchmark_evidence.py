"""Recompute benchmark regressions and overheads from raw medians."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from performance_evidence import (
    EXPECTED_MEASUREMENTS,
    DIAGNOSTIC_PROFILE_CONTRACTS,
    DIAGNOSTIC_QUEUE_PROFILES,
    MEASUREMENT_LIMITS,
    PERFORMANCE_FORMAT_VERSION,
    PROFILE_CONTRACTS,
    PROFILE_ORDER_STRATEGY,
    OBSERVER_QUEUE_PROFILES,
    EvidenceError,
    close,
    finite_number,
    percent,
    positive_integer,
    positive_number,
    require_accepted_baseline,
    require_benchmark_command,
    require_benchmark_rss,
    require_diagnostic_measurements,
    require_exact_keys,
    require_hardware,
    require_overheads,
    require_observer_queues,
    require_recorded_time,
)
from performance_evidence_metadata import require_nonempty_string


def require_benchmark(
    evidence: dict[str, Any],
    baseline: dict[str, Any],
    binary: Path,
    channel: str,
    comparison_mode: str,
    baseline_binary: Path | None,
    baseline_path: Path | None = None,
) -> None:
    """Validate candidate results against an accepted baseline and raw medians."""
    require_exact_keys(
        evidence,
        {
            "formatVersion", "passed", "recordedAtUnixSeconds", "baselinePath",
            "comparisonMode", "baselineSource", "source", "hardware", "command",
            "samples", "iterationsPerSample", "rss", "profileOrderStrategy",
            "profiles", "observerQueues", "diagnosticProfiles",
            "diagnosticMeasurements", "diagnosticObserverQueues", "metrics",
            "overheads",
        },
        "benchmark evidence",
    )
    candidate = require_accepted_baseline(
        baseline,
        binary,
        channel,
        comparison_mode,
        baseline_binary,
    )
    format_version = positive_integer(evidence.get("formatVersion"), "format version")
    if format_version != PERFORMANCE_FORMAT_VERSION:
        raise EvidenceError("benchmark evidence format does not match")
    if evidence.get("comparisonMode") != comparison_mode:
        raise EvidenceError("benchmark comparison mode does not match")
    if evidence.get("source") != candidate or evidence.get("baselineSource") != baseline["source"]:
        raise EvidenceError("benchmark source identities do not match")
    if evidence.get("hardware") != baseline.get("hardware"):
        raise EvidenceError("benchmark hardware does not match accepted baseline")
    require_hardware(evidence.get("hardware"), current=True)
    recorded_at = require_recorded_time(
        evidence.get("recordedAtUnixSeconds"), evidence.get("source")
    )
    if recorded_at < baseline["recordedAtUnixSeconds"]:
        raise EvidenceError("benchmark candidate predates its accepted baseline")
    samples = positive_integer(evidence.get("samples"), "benchmark sample count")
    iterations = positive_integer(
        evidence.get("iterationsPerSample"), "benchmark iteration count"
    )
    if (
        samples != 31
        or iterations != 100
        or evidence.get("profileOrderStrategy") != PROFILE_ORDER_STRATEGY
        or evidence.get("profiles") != list(PROFILE_CONTRACTS)
        or evidence.get("diagnosticProfiles") != list(DIAGNOSTIC_PROFILE_CONTRACTS)
    ):
        raise EvidenceError("benchmark candidate conditions do not match")
    arguments = require_benchmark_command(
        evidence.get("command"),
        "compare",
        candidate,
        31,
        100,
        comparison_mode,
        baseline_path,
        baseline_binary,
    )
    reported_baseline = require_nonempty_string(
        evidence.get("baselinePath"), "benchmark baseline path"
    )
    if reported_baseline != arguments["--baseline"]:
        raise EvidenceError("benchmark baselinePath does not match recorded command")
    if baseline_path is not None and Path(reported_baseline).name != baseline_path.name:
        raise EvidenceError("benchmark baselinePath does not match verifier input")
    require_benchmark_rss(evidence.get("rss"))
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
    baseline_medians = {
        item["name"]: positive_number(item.get("medianNs"), "baseline median")
        for item in baseline["measurements"]
    }
    metrics = evidence.get("metrics")
    if not isinstance(metrics, list) or not all(isinstance(item, dict) for item in metrics):
        raise EvidenceError("benchmark metrics are malformed")
    if [item.get("name") for item in metrics] != list(EXPECTED_MEASUREMENTS):
        raise EvidenceError("benchmark metric set does not match")
    metric_passed = True
    candidate_disabled = 0.0
    for item in metrics:
        require_exact_keys(
            item,
            {
                "name", "baselineMedianNs", "candidateMedianNs",
                "regressionPercent", "limitPercent", "passed",
            },
            "benchmark metric",
        )
        name = item["name"]
        baseline_median = positive_number(
            item.get("baselineMedianNs"), "reported baseline median"
        )
        candidate_median = positive_number(
            item.get("candidateMedianNs"), "candidate median"
        )
        reported = finite_number(item.get("regressionPercent"), "regression percent")
        limit = finite_number(item.get("limitPercent"), "regression limit")
        calculated = percent(candidate_median, baseline_median)
        passed = calculated <= MEASUREMENT_LIMITS[name]
        if not close(baseline_median, baseline_medians[name]):
            raise EvidenceError("reported baseline median does not match accepted baseline")
        if limit != MEASUREMENT_LIMITS[name] or not close(reported, calculated):
            raise EvidenceError("regression was not derived from raw medians")
        if item.get("passed") is not passed:
            raise EvidenceError("metric passed flag disagrees with recomputed regression")
        metric_passed = metric_passed and passed
        if name == "policy-disabled":
            candidate_disabled = candidate_median
    overhead_passed = require_overheads(
        evidence.get("overheads"), candidate_disabled, False
    )
    recomputed = metric_passed and overhead_passed
    if evidence.get("passed") is not recomputed or not recomputed:
        raise EvidenceError("benchmark passed flag disagrees with recomputed result")
