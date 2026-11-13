#!/usr/bin/env python3
"""Attach verifiable user approval to a benchmark baseline."""

import argparse
import json
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any

from performance_evidence import (
    GENERIC_REVIEWERS,
    OLD_TEMPLATE_REASON,
    PERFORMANCE_FORMAT_VERSION,
    EvidenceError,
    require_baseline_contract,
    require_approval,
)
from performance_evidence_metadata import strict_json_loads


def accept(
    evidence: dict[str, Any], reviewer: str, reason: str, accepted_at: str
) -> dict[str, Any]:
    reviewer = reviewer.strip()
    reason = reason.strip()
    if (
        evidence.get("formatVersion") != PERFORMANCE_FORMAT_VERSION
        or evidence.get("accepted") is not False
    ):
        raise ValueError("Only an unapproved format-version 5 baseline can be accepted.")
    try:
        require_baseline_contract(evidence)
    except EvidenceError as error:
        raise ValueError(str(error)) from error
    source = evidence.get("source")
    if not isinstance(source, dict) or source.get("dirty") is not False:
        raise ValueError("The baseline requires a clean source identity.")
    required_source = ("revision", "binarySha256", "toolchain")
    if any(not source.get(name) for name in required_source):
        raise ValueError("The source identity lacks a revision, toolchain, or binary digest.")
    if not reviewer or reviewer.lower() in GENERIC_REVIEWERS:
        raise ValueError("The approver must have an auditable name or personal ID.")
    if len(reason) < 40 or reason == OLD_TEMPLATE_REASON:
        raise ValueError("Please specify the measurement conditions and decision basis.")
    try:
        accepted_timestamp = datetime.fromisoformat(accepted_at)
    except ValueError as error:
        raise ValueError("Please specify the approval timestamp in ISO 8601 format.") from error
    if accepted_timestamp.utcoffset() != timedelta(0):
        raise ValueError("Please specify the approval timestamp with a UTC offset.")
    accepted_timestamp = accepted_timestamp.astimezone(timezone.utc).replace(microsecond=0)
    evidence["accepted"] = True
    evidence["acceptedBy"] = reviewer
    evidence["acceptedAt"] = accepted_timestamp.isoformat()
    evidence["acceptedAtUnixSeconds"] = int(accepted_timestamp.timestamp())
    evidence["acceptedReason"] = reason
    try:
        require_approval(evidence)
    except EvidenceError as error:
        raise ValueError(str(error)) from error
    return evidence


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--reviewer", required=True)
    parser.add_argument("--reason", required=True)
    args = parser.parse_args()
    evidence = strict_json_loads(args.input.read_text(encoding="utf-8"))
    if not isinstance(evidence, dict):
        raise SystemExit("The benchmark baseline root must be a JSON object.")
    accepted_at = datetime.now(timezone.utc).replace(microsecond=0).isoformat()
    try:
        evidence = accept(evidence, args.reviewer, args.reason, accepted_at)
    except ValueError as error:
        raise SystemExit(str(error)) from error
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
