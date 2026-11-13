"""Recompute load/soak evidence from fixed conditions and live counters."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from performance_evidence import (
    WORKLOAD_FORMAT_VERSION,
    EvidenceError,
    close,
    finite_number,
    nonnegative_integer,
    positive_integer,
    positive_number,
)
from performance_evidence_metadata import (
    require_command,
    require_exact_keys,
    require_hardware,
    require_identity,
    require_recorded_time,
)
from production_workload_metrics import LATENCY_BOUNDS_MS

MAX_MEMORY_GROWTH = 64 * 1024 * 1024
WARMUP_REQUESTS = 256
POLICIES = {
    "load": {
        "mode": "load",
        "concurrency": 64,
        "requests": 10_000,
        "duration": None,
        "rss_baseline": 1.0,
        "output": "load.json",
    },
    "soak": {
        "mode": "soak",
        "concurrency": 32,
        "requests": None,
        "duration": 900,
        "rss_baseline": 1.0,
        "output": "soak.json",
    },
    "short-soak": {
        "mode": "short-soak",
        "concurrency": 2,
        "requests": None,
        "duration": 2,
        "rss_baseline": 0.5,
        "output": "harness-short-soak.json",
    },
}


def require_workload(
    evidence: dict[str, Any], kind: str, binary: Path, channel: str
) -> None:
    """Verify fixed workload conditions and results without trusting a self-reported pass."""
    policy = POLICIES.get(kind)
    if policy is None:
        raise EvidenceError("unknown workload evidence kind")
    require_exact_keys(
        evidence,
        {
            "formatVersion", "mode", "passed", "source", "recordedAtUnixSeconds",
            "command", "hardware", "concurrency", "configuredRequests",
            "configuredDurationSeconds", "warmupRequests", "requests",
            "queryAttempts", "querySuccesses", "mutationAttempts",
            "mutationSuccesses", "graphqlErrorCodes", "errorCount", "errorCounts",
            "firstErrors", "elapsedSeconds", "workloadStartSeconds",
            "workloadEndSeconds", "medianLatencyUpperBoundMs",
            "medianLatencyGreaterThanMs", "p95LatencyUpperBoundMs",
            "p95LatencyGreaterThanMs", "latencyHistogram", "rssSampler",
            "rssBaselineSampleCount", "rssBaselineStartIndex", "rssBaselineEndIndex",
            "initialSteadyRssBytes", "peakRssBytes", "steadyMemoryGrowthBytes",
            "readiness", "serverExit", "totalHarnessSeconds",
        },
        "workload evidence",
    )
    format_version = positive_integer(evidence.get("formatVersion"), "format version")
    if format_version != WORKLOAD_FORMAT_VERSION or evidence.get("mode") != policy["mode"]:
        raise EvidenceError("workload evidence format or mode does not match")
    # A short functional harness may use a dirty tree; formal load/soak runs may not.
    require_identity(evidence, binary, channel, require_clean=kind != "short-soak")
    require_recorded_time(
        evidence.get("recordedAtUnixSeconds"), evidence.get("source")
    )
    require_hardware(evidence.get("hardware"), current=True)
    require_workload_command(evidence, policy, binary, channel)
    concurrency = positive_integer(evidence.get("concurrency"), "concurrency")
    warmup = positive_integer(evidence.get("warmupRequests"), "warm-up request count")
    configured_requests = evidence.get("configuredRequests")
    configured_duration = evidence.get("configuredDurationSeconds")
    if configured_requests is not None:
        configured_requests = positive_integer(configured_requests, "configured request count")
    if configured_duration is not None:
        configured_duration = positive_integer(configured_duration, "configured duration")
    if (
        concurrency != policy["concurrency"]
        or configured_requests != policy["requests"]
        or configured_duration != policy["duration"]
        or warmup != WARMUP_REQUESTS
    ):
        raise EvidenceError("workload fixed configuration does not match")

    requests = positive_integer(evidence.get("requests"), "request count")
    queries = nonnegative_integer(evidence.get("queryAttempts"), "Query attempts")
    mutations = nonnegative_integer(evidence.get("mutationAttempts"), "Mutation attempts")
    query_successes = nonnegative_integer(
        evidence.get("querySuccesses"), "Query successes"
    )
    mutation_successes = nonnegative_integer(
        evidence.get("mutationSuccesses"), "Mutation successes"
    )
    if queries + mutations != requests:
        raise EvidenceError("Query and Mutation attempts do not equal request count")
    if query_successes != queries or mutation_successes != mutations:
        raise EvidenceError("not every operation reached its expected result")
    if kind == "load":
        if requests != 10_000 or queries != requests or mutations != 0:
            raise EvidenceError("load must contain exactly 10,000 successful Queries")
    elif mutations <= 0 or mutations != requests // 10 or queries != requests - mutations:
        raise EvidenceError("soak Query to Mutation schedule does not match 9:1")

    require_error_counts(evidence)
    require_latency(evidence, requests)
    samples, baseline_samples = require_rss(evidence)
    require_memory(evidence, samples, baseline_samples)
    elapsed = positive_number(evidence.get("elapsedSeconds"), "workload elapsed time")
    total = positive_number(evidence.get("totalHarnessSeconds"), "total harness time")
    if total < elapsed:
        raise EvidenceError("total harness time is shorter than workload time")
    duration = policy["duration"]
    if duration is not None and elapsed < duration:
        raise EvidenceError("soak elapsed time is shorter than configured duration")
    readiness = evidence.get("readiness")
    require_exact_keys(
        readiness, {"applicationInFlight", "endpointInFlight"}, "readiness evidence"
    )
    assert isinstance(readiness, dict)
    application = nonnegative_integer(
        readiness.get("applicationInFlight"), "application in-flight count"
    )
    endpoint = nonnegative_integer(
        readiness.get("endpointInFlight"), "endpoint in-flight count"
    )
    if application != 0 or endpoint != 0:
        raise EvidenceError("workload did not drain in-flight requests")
    if nonnegative_integer(evidence.get("serverExit"), "server exit status") != 0:
        raise EvidenceError("workload server did not exit successfully")
    if evidence.get("passed") is not True:
        raise EvidenceError("workload passed flag disagrees with recomputed result")


def require_workload_command(
    evidence: dict[str, Any], policy: dict[str, Any], binary: Path, version: str
) -> None:
    flags = (
        "--binary", "--toolchain", "--requests", "--concurrency", "--duration",
        "--rss-baseline-seconds", "--output",
    )
    arguments = require_command(
        evidence.get("command"), "production-http-workload.py", policy["mode"], flags
    )
    if Path(arguments["--binary"]).name != binary.name:
        raise EvidenceError("workload command binary does not match verifier input")
    if arguments["--toolchain"] != version:
        raise EvidenceError("workload command toolchain does not match verifier input")
    expected = {
        "--requests": 10_000,
        "--concurrency": policy["concurrency"],
        "--duration": policy["duration"] or 900,
    }
    for flag, value in expected.items():
        if _canonical_integer(arguments[flag], flag) != value:
            raise EvidenceError("workload command fixed integer does not match")
    expected_rss = "0.5" if policy["rss_baseline"] == 0.5 else "1.0"
    if arguments["--rss-baseline-seconds"] != expected_rss:
        raise EvidenceError("workload command RSS baseline does not match")
    if Path(arguments["--output"]).name != policy["output"]:
        raise EvidenceError("workload command output name does not match")


def _canonical_integer(value: str, label: str) -> int:
    try:
        parsed = int(value)
    except ValueError as error:
        raise EvidenceError(f"{label} is not an integer") from error
    if parsed <= 0 or str(parsed) != value:
        raise EvidenceError(f"{label} is not a canonical positive integer")
    return parsed


def require_error_counts(evidence: dict[str, Any]) -> None:
    errors = evidence.get("errorCounts")
    graphql = evidence.get("graphqlErrorCodes")
    first = evidence.get("firstErrors")
    if not isinstance(errors, dict) or not isinstance(graphql, dict):
        raise EvidenceError("workload error counters are malformed")
    error_total = nonnegative_integer(evidence.get("errorCount"), "error count")
    for value in errors.values():
        nonnegative_integer(value, "classified error count")
    for value in graphql.values():
        nonnegative_integer(value, "GraphQL error count")
    if error_total != sum(errors.values()) or error_total != 0 or errors or graphql:
        raise EvidenceError("workload contains errors")
    if not isinstance(first, list) or len(first) > 100 or first:
        raise EvidenceError("workload first error samples do not match zero errors")


def require_latency(evidence: dict[str, Any], requests: int) -> None:
    histogram = evidence.get("latencyHistogram")
    require_exact_keys(
        histogram,
        {"upperBoundsMs", "bucketCounts", "overflowIsGreaterThanMs"},
        "latency histogram",
    )
    assert isinstance(histogram, dict)
    if histogram.get("upperBoundsMs") != list(LATENCY_BOUNDS_MS):
        raise EvidenceError("latency histogram bounds do not match")
    if histogram.get("overflowIsGreaterThanMs") != LATENCY_BOUNDS_MS[-1]:
        raise EvidenceError("latency overflow marker does not match")
    counts = histogram.get("bucketCounts")
    if not isinstance(counts, list) or len(counts) != len(LATENCY_BOUNDS_MS) + 1:
        raise EvidenceError("latency histogram bucket count does not match")
    parsed = [nonnegative_integer(value, "latency bucket count") for value in counts]
    if sum(parsed) != requests:
        raise EvidenceError("latency histogram does not equal request count")
    require_percentile(evidence, "median", parsed, 0.5)
    require_percentile(evidence, "p95", parsed, 0.95)


def require_percentile(
    evidence: dict[str, Any], prefix: str, counts: list[int], fraction: float
) -> None:
    upper, greater = percentile(counts, fraction)
    reported_upper = evidence.get(f"{prefix}LatencyUpperBoundMs")
    reported_greater = evidence.get(f"{prefix}LatencyGreaterThanMs")
    if reported_upper != upper or reported_greater != greater:
        raise EvidenceError(f"{prefix} latency was not derived from histogram")
    if upper is not None and upper <= 0:
        raise EvidenceError(f"{prefix} latency upper bound must be positive")


def percentile(counts: list[int], fraction: float) -> tuple[float | None, float | None]:
    total = sum(counts)
    target = max(1, int(total * fraction + 0.999999))
    seen = 0
    for index, count in enumerate(counts):
        seen += count
        if seen >= target:
            if index < len(LATENCY_BOUNDS_MS):
                return float(LATENCY_BOUNDS_MS[index]), None
            return None, float(LATENCY_BOUNDS_MS[-1])
    raise EvidenceError("latency histogram is empty")


def require_memory(
    evidence: dict[str, Any], samples: list[int], baseline_samples: list[int]
) -> None:
    initial = positive_integer(evidence.get("initialSteadyRssBytes"), "initial RSS")
    peak = positive_integer(evidence.get("peakRssBytes"), "peak RSS")
    growth = nonnegative_integer(evidence.get("steadyMemoryGrowthBytes"), "RSS growth")
    calculated_initial = baseline_samples[0]
    calculated_peak = max(samples)
    calculated_growth = max(0, calculated_peak - calculated_initial)
    if (
        initial != calculated_initial
        or peak != calculated_peak
        or growth != calculated_growth
        or growth > MAX_MEMORY_GROWTH
    ):
        raise EvidenceError("memory growth was not derived or exceeded 64 MiB")


def require_rss(evidence: dict[str, Any]) -> tuple[list[int], list[int]]:
    sampler = evidence.get("rssSampler")
    require_exact_keys(
        sampler,
        {"passed", "samples", "sampleCount", "elapsedSeconds",
         "errorCount", "errors", "threadStopped"},
        "RSS sampler evidence",
    )
    assert isinstance(sampler, dict)
    observations = sampler.get("samples")
    if not isinstance(observations, list):
        raise EvidenceError("RSS raw observations are missing")
    samples = positive_integer(sampler.get("sampleCount"), "RSS sample count")
    errors = nonnegative_integer(sampler.get("errorCount"), "RSS error count")
    elapsed = positive_number(sampler.get("elapsedSeconds"), "RSS elapsed time")
    start = positive_number(evidence.get("workloadStartSeconds"), "workload start")
    end = positive_number(evidence.get("workloadEndSeconds"), "workload end")
    workload_elapsed = positive_number(evidence.get("elapsedSeconds"), "workload time")
    if not start < end <= elapsed or not close(end - start, workload_elapsed):
        raise EvidenceError("RSS workload interval does not match elapsed time")
    values: list[int] = []
    intervals: list[tuple[float, float]] = []
    previous_end = 0.0
    for observation in observations:
        require_exact_keys(
            observation, {"readStartSeconds", "readEndSeconds", "rssBytes"},
            "RSS observation",
        )
        read_start = finite_number(observation["readStartSeconds"], "RSS read start")
        read_end = finite_number(observation["readEndSeconds"], "RSS read end")
        if not previous_end <= read_start <= read_end <= elapsed:
            raise EvidenceError("RSS read intervals are invalid or out of order")
        previous_end = read_end
        values.append(positive_integer(observation["rssBytes"], "RSS bytes"))
        intervals.append((read_start, read_end))
    baseline = positive_integer(
        evidence.get("rssBaselineSampleCount"), "RSS baseline sample count"
    )
    baseline_start = nonnegative_integer(
        evidence.get("rssBaselineStartIndex"), "RSS baseline start"
    )
    baseline_end = positive_integer(
        evidence.get("rssBaselineEndIndex"), "RSS baseline end"
    )
    if (
        samples != len(values)
        or samples < 2
        or (baseline, baseline_start, baseline_end) != (1, 0, 1)
        or intervals[0][1] > start
        or not any(start <= left < end and left <= right <= end
                   for left, right in intervals[baseline_end:])
        or errors != 0
        or sampler.get("errors") != []
        or sampler.get("threadStopped") is not True
        or sampler.get("passed") is not True
    ):
        raise EvidenceError("RSS requires one pre-load and at least one during-load observation")
    return values, values[:1]
