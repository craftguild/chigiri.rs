#!/usr/bin/env python3
"""Reject failed, skipped, cancelled, or missing required CI jobs."""

import json
import os

from validation_profiles import DEFAULT_PROFILE, PROFILES


REQUIRED = {"linux", "macos"}


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("The required-jobs JSON contains a duplicate field.")
        result[key] = value
    return result


def validate(text, profile=DEFAULT_PROFILE):
    if profile not in PROFILES:
        raise ValueError("The required validation profile must be ci or full.")
    if len(text.encode("utf-8")) > 65_536:
        raise ValueError("The required-jobs JSON exceeds its size limit.")
    result = json.loads(text, object_pairs_hook=unique_object)
    if not isinstance(result, dict) or set(result) != REQUIRED:
        raise ValueError("The required-job set must be exactly linux and macos.")
    for job in sorted(REQUIRED):
        if result[job] != {"result": "success", "outputs": {"validation": "complete", "profile": profile}}:
            raise ValueError(f"Required job did not succeed with validation complete: {job}")


if __name__ == "__main__":
    try:
        validate(os.environ.get("CHIGIRI_REQUIRED_JOBS", ""),
                 os.environ.get("CHIGIRI_VALIDATION_PROFILE", DEFAULT_PROFILE))
    except ValueError as error:
        raise SystemExit(str(error)) from error
    print("Required Linux and macOS validation jobs completed successfully.")
