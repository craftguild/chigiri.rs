#!/usr/bin/env python3
"""Reject former-brand identities outside explicit history and rejection owners."""

from __future__ import annotations

import json
import os
import re
import sys
from pathlib import Path

FORMER = re.compile(r"pythia", re.IGNORECASE)
IGNORED_PARTS = {".git", "target", "__pycache__", ".craftguild", ".private", "node_modules", ".beads"}
EXTERNAL_DATA_DIRS = {
    "fuzz/artifacts",
    "fuzz/corpus",
    "vendor/full_moon-2.2.0",
    "vendor/stylua-2.5.2",
}
REJECTION_OWNERS = {
    "crates/cli/tests/brand_cutover.rs",
    "crates/cli/tests/command_tree.rs",
    "crates/codegen/src/contract/check/tests.rs",
    "crates/codegen/src/manifest/tests.rs",
    "crates/mock/src/config/envelope_tests.rs",
    "crates/mock/src/config/simplified_tests.rs",
    "crates/mock/src/project/simplified_tests.rs",
    "scripts/check-chigiri-brand.py",
    "scripts/chigiri-relocated-acceptance.py",
    "scripts/test-chigiri-brand.py",
}
HISTORICAL_TRACE_OWNERS = {
    "scripts/production_acceptance_trace.py",
}
TRANSITION_RECORDS = {
    "docs/adr/0048-chigiri-brand-and-single-binary-cli.md",
    "docs/chigiri-single-binary-implementation-goal-prompt.md",
    "docs/chigiri-single-binary-validation.md",
}
HISTORICAL_EXACT = {
    "scripts/fixtures/historical-acceptance-traces/production-readiness.md",
    "scripts/fixtures/historical-acceptance-traces/usability.md",
    "docs/agent-assisted-mock-inference-workflow.md",
    "docs/implementation-goal-prompt.md",
    "docs/mock-model-reference-usage.md",
    "docs/mock-reference-usability-research.md",
    "docs/mock-typecheck-performance.md",
    "docs/project-name-candidates.md",
    "docs/simplified-mock-directives-sd0.md",
    "docs/simplified-mock-observation-budget-proposal.md",
    "docs/mock-regex-engine-evaluation.md",
}
HISTORICAL_SUFFIXES = (
    "-implementation-goal-prompt.md",
    "-validation.md",
    "-real-acceptance.md",
    "-test-trace.md",
    "-review.md",
)
REQUIRED_IDENTITIES = {
    "chigiri/config-v2",
    "chigiri/generated-crate-v1",
    "chigiri/generator-info-v1",
    "chigiri/diagnostics-v2",
    "chigiri/recovery-v1",
    "chigiri/domain-enums-v1",
    "chigiri-mock-project/v10",
    "chigiri-mock/v11",
    "chigiri-mock-api/v10",
    "chigiri-mock-capabilities/v4",
    "chigiri-mock-reference/v13",
    "chigiri-mock-reference-topic/v9",
    "chigiri-mock-check/v3",
    "chigiri-mock-log/v5",
    "chigiri-mock-update-state/v11",
    "chigiri-mock-schema-snapshot/v11",
    "chigiri-mock-schema-sync/v1",
    "chigiri-mock-sdl-normalization/v1",
    "chigiri-mock-directives/v3",
    "__chigiri-stdio-writer-v1",
    "ChigiriMockProjectConfig",
}


def relative(path: Path, root: Path) -> str:
    return path.relative_to(root).as_posix()


def is_historical(name: str) -> bool:
    if name in HISTORICAL_EXACT or name in TRANSITION_RECORDS:
        return True
    if name.startswith(("docs/reviews/", "docs/releases/")):
        return True
    if name.startswith("docs/adr/"):
        return True
    return name.startswith("docs/") and name.endswith(HISTORICAL_SUFFIXES)


def files(root: Path):
    for directory, names, filenames in os.walk(root):
        directory_name = relative(Path(directory), root)
        if directory_name in EXTERNAL_DATA_DIRS:
            names.clear()
            continue
        names[:] = [name for name in names if name not in IGNORED_PARTS]
        for filename in filenames:
            path = Path(directory, filename)
            if not IGNORED_PARTS.isdisjoint(path.relative_to(root).parts):
                continue
            yield path


def scan(root: Path) -> tuple[list[str], set[str], int]:
    failures = []
    identities = set()
    checked = 0
    for path in files(root):
        name = relative(path, root)
        if name in REJECTION_OWNERS or is_historical(name):
            continue
        allows_historical_trace = name in HISTORICAL_TRACE_OWNERS
        if FORMER.search(name):
            failures.append(f"former brand in active path: {name}")
        try:
            source = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        checked += 1
        match = FORMER.search(source)
        if match and not allows_historical_trace:
            line = source.count("\n", 0, match.start()) + 1
            failures.append(f"former brand in active text: {name}:{line}")
        for identity in REQUIRED_IDENTITIES:
            if identity in source:
                identities.add(identity)
    return failures, identities, checked


def inventory(root: Path, identities: set[str]) -> list[str]:
    import tomllib

    failures = []
    missing = sorted(REQUIRED_IDENTITIES - identities)
    failures.extend(f"required Chigiri identity is absent: {value}" for value in missing)

    cli = tomllib.loads((root / "crates/cli/Cargo.toml").read_text())
    if cli.get("package", {}).get("name") != "chigiri_cli":
        failures.append("unified adapter package must be chigiri_cli")
    if [target.get("name") for target in cli.get("bin", [])] != ["chigiri"]:
        failures.append("unified adapter must own exactly the chigiri binary")
    for package in ("codegen", "mock"):
        manifest = tomllib.loads((root / f"crates/{package}/Cargo.toml").read_text())
        if manifest.get("bin"):
            failures.append(f"crates/{package} must not expose a binary target")

    registry = json.loads((root / "scripts/generated-manifests.json").read_text())
    for manifest_name in registry:
        path = root / manifest_name
        manifest = tomllib.loads(path.read_text())
        output = path.parent / manifest["generation"]["output"]
        for owned in ("chigiri-generated.json", "src/__chigiri"):
            if not (output / owned).exists():
                failures.append(f"canonical output missing {owned}: {output}")
    return failures


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    failures, identities, checked = scan(root)
    failures.extend(inventory(root, identities))
    if failures:
        print("Chigiri brand inventory failed:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1
    print(
        "Chigiri brand inventory passed: "
        f"{checked} active text files, {len(identities)} required identities, "
        f"{len(REJECTION_OWNERS)} rejection owners"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
