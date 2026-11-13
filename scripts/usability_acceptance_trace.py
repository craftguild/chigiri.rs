"""Validate ADR-0053 retirements against explicit, test-owned trace migrations."""

import json
import re
from pathlib import Path

from historical_trace import load_trace


EXPECTED = {
    f"{prefix}-{number:02d}"
    for prefix, count in (("API", 12), ("INJ", 12), ("PROJ", 14), ("LIMIT", 16))
    for number in range(1, count + 1)
}
RETIRED = {f"PROJ-{number:02d}" for number in (1, 2, 3, 4, 5, 8)}
MIGRATED = {f"PROJ-{number:02d}" for number in (7, 9, 10, 11, 12, 13)}


def rows(document):
    found = re.findall(
        r"^\|\s*((?:API|INJ|PROJ|LIMIT)-[0-9]{2})\s*\|\s*([^|]+?)\s*\|",
        document, re.MULTILINE,
    )
    if len(found) != len(EXPECTED) or {key for key, _ in found} != EXPECTED:
        raise ValueError("The usability trace must contain exactly its 54 unique IDs.")
    return {key: evidence.strip() for key, evidence in found}


def validate(repository, spec, history):
    fixture = json.loads(
        (repository / "scripts/fixtures/usability-trace-migrations.json").read_text()
    )
    if (set(fixture) != {"retired", "migrated"}
            or set(fixture["retired"]) != RETIRED
            or set(fixture["migrated"]) != MIGRATED):
        raise ValueError("The ADR-0053 migration fixture has an invalid ID inventory.")
    previous, current = rows(history), rows(spec)
    for key in sorted(EXPECTED):
        recorded = fixture["retired"].get(key, fixture["migrated"].get(key))
        if recorded is None:
            if previous[key] != current[key]:
                raise ValueError(f"Unapproved usability evidence change: {key}")
        elif recorded != [previous[key], current[key]]:
            raise ValueError(f"Usability evidence differs from its explicit migration: {key}")
    return {key: current[key] for key in sorted(EXPECTED - RETIRED)}


def main():
    repository = Path(__file__).resolve().parent.parent
    active = validate(
        repository, (repository / "docs/spec-conformance.md").read_text(),
        load_trace(repository, "usability"),
    )
    for key, evidence in active.items():
        print(f"{key}\t{evidence}")


if __name__ == "__main__":
    main()
