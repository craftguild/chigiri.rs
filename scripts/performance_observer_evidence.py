"""Validate performance diagnostic profiles and observer queues as closed contracts."""

from __future__ import annotations

import math

from performance_evidence_metadata import EvidenceError, require_exact_keys

DIAGNOSTIC_PROFILE_CONTRACTS = (
    {
        "name": "diagnostic-noop-observer",
        "includes": ["lifecycle event construction", "no-op callback"],
        "excludes": ["queue", "request/error ID", "sampling"],
    },
    {
        "name": "diagnostic-queue-only",
        "includes": ["lifecycle event construction", "bounded observer queue"],
        "excludes": ["request/error ID", "sampling"],
    },
    {
        "name": "diagnostic-random-request-id",
        "includes": ["seeded keyed-hash request ID", "request telemetry propagation"],
        "excludes": ["lifecycle observer", "error ID", "sampling"],
    },
    {
        "name": "diagnostic-telemetry-clone",
        "includes": ["fixed request ID", "request telemetry propagation and cloning"],
        "excludes": [
            "request ID derivation", "lifecycle observer", "error ID", "sampling"
        ],
    },
    {
        "name": "diagnostic-full-observer",
        "includes": ["request/error ID", "sampling", "bounded observer queue"],
        "excludes": ["operation source", "rate"],
    },
)
EXPECTED_DIAGNOSTIC_PROFILES = tuple(
    item["name"] for item in DIAGNOSTIC_PROFILE_CONTRACTS
)
OBSERVER_QUEUE_PROFILES = ("observer-enabled", "facade-composition-enabled")
DIAGNOSTIC_QUEUE_PROFILES = (
    "diagnostic-queue-only", "diagnostic-full-observer"
)
OBSERVER_QUEUE_CAPACITY = 65_536
LIFECYCLE_EVENTS_PER_REQUEST = 29


def require_diagnostic_measurements(value: object) -> None:
    if not isinstance(value, list) or not all(isinstance(item, dict) for item in value):
        raise EvidenceError("diagnostic measurements are malformed")
    if [item.get("name") for item in value] != list(EXPECTED_DIAGNOSTIC_PROFILES):
        raise EvidenceError("diagnostic measurement set does not match")
    for item in value:
        require_exact_keys(item, {"name", "medianNs"}, "diagnostic measurement")
        _positive_number(item.get("medianNs"), "diagnostic median")


def require_observer_queues(
    value: object,
    samples: int,
    iterations: int,
    profile_names: tuple[str, ...],
) -> None:
    if not isinstance(value, list) or not all(isinstance(item, dict) for item in value):
        raise EvidenceError("observer queue evidence is malformed")
    if [item.get("name") for item in value] != list(profile_names):
        raise EvidenceError("observer queue profile set does not match")
    expected = samples * iterations * LIFECYCLE_EVENTS_PER_REQUEST
    for item in value:
        require_exact_keys(
            item,
            {
                "name", "capacity", "lifecycleEventsPerRequest",
                "expectedLifecycleEventCount", "acceptedLifecycleEventCount",
                "droppedLifecycleEventCount", "acceptedErrorEventCount",
                "droppedErrorEventCount",
            },
            "observer queue evidence",
        )
        capacity = _positive_integer(item.get("capacity"), "observer queue capacity")
        events = _positive_integer(
            item.get("lifecycleEventsPerRequest"), "lifecycle events per request"
        )
        expected_count = _positive_integer(
            item.get("expectedLifecycleEventCount"), "expected lifecycle event count"
        )
        accepted = _positive_integer(
            item.get("acceptedLifecycleEventCount"), "accepted lifecycle event count"
        )
        dropped = _nonnegative_integer(
            item.get("droppedLifecycleEventCount"), "dropped lifecycle event count"
        )
        accepted_errors = _nonnegative_integer(
            item.get("acceptedErrorEventCount"), "accepted error event count"
        )
        dropped_errors = _nonnegative_integer(
            item.get("droppedErrorEventCount"), "dropped error event count"
        )
        if (
            capacity != OBSERVER_QUEUE_CAPACITY
            or events != LIFECYCLE_EVENTS_PER_REQUEST
            or expected_count != expected
            or accepted != expected
            or dropped != 0
            or accepted_errors != 0
            or dropped_errors != 0
        ):
            raise EvidenceError("observer queue did not deliver every expected event")


def _positive_number(value: object, label: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise EvidenceError(f"{label} must be a finite number")
    number = float(value)
    if not math.isfinite(number) or number <= 0:
        raise EvidenceError(f"{label} must be a positive finite number")
    return number


def _positive_integer(value: object, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
        raise EvidenceError(f"{label} must be a positive integer")
    return value


def _nonnegative_integer(value: object, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise EvidenceError(f"{label} must be a non-negative integer")
    return value
