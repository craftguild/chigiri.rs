"""Validate the exact owner and specification reference for all active SG IDs."""

import re
from pathlib import Path


COUNTS = {"CLI": 7, "CRATE": 6, "FS": 6, "PROC": 3, "INFO": 5, "VERIFY": 6, "LAYOUT": 8}
# ADR-0059 retires repetitive build matrices while preserving SDK behavior.
RETIRED = {"SG-INFO-02", "SG-INFO-03"}
EXPECTED = {f"SG-{kind}-{number:02d}" for kind, count in COUNTS.items()
            for number in range(1, count + 1)} - RETIRED


def validate(repository, specification, trace):
    rows = re.findall(
        r"^\|\s*(SG-[A-Z]+-[0-9]{2})\s*\|[^\n|]+\|\s*([^\n|]+?)\s*\|$",
        specification,
        re.MULTILINE,
    )
    if len(rows) != len(EXPECTED) or {key for key, _ in rows} != EXPECTED:
        raise ValueError("The specification does not contain the exact active SG IDs.")
    sections = re.findall(r"^## (SG-[A-Z]+-[0-9]{2})\n(.*?)(?=^## |\Z)", trace, re.MULTILINE | re.DOTALL)
    if len(sections) != len(EXPECTED) or {key for key, _ in sections} != EXPECTED:
        raise ValueError("The exact trace does not contain the exact active SG IDs.")
    table = dict(rows)
    count = 0
    for key, section in sections:
        references = re.findall(r"^- `([^`]+)`$", section, re.MULTILINE)
        if not references or len(set(references)) != len(references):
            raise ValueError(f"The trace must contain unique owner references: {key}")
        expected = (
            f"`{references[0]}`, "
            f"[All assertions](sibling-generator-test-trace.md#{key.lower()})"
        )
        if table[key] != expected:
            raise ValueError(f"The specification and exact trace disagree: {key}")
        for reference in references:
            name, separator, marker = reference.partition("#")
            path = Path(name)
            if (not separator or not marker or path.is_absolute() or ".." in path.parts
                    or path.parts[0] not in {"crates", "scripts", "tests"}):
                raise ValueError(f"The exact owner path or marker is invalid: {key}")
            target = (repository / path).resolve()
            if not target.is_relative_to(repository.resolve()) or not target.is_file():
                raise ValueError(f"The exact owner file does not exist: {key}")
            if marker not in target.read_text(encoding="utf-8"):
                raise ValueError(f"The exact owner marker does not exist: {key}")
            count += 1
    return count


if __name__ == "__main__":
    root = Path(__file__).resolve().parent.parent
    count = validate(root, (root / "docs/spec-conformance.md").read_text(),
                     (root / "docs/sibling-generator-test-trace.md").read_text())
    print(f"Sibling acceptance trace passed: {count} owner references across {len(EXPECTED)} active IDs")
