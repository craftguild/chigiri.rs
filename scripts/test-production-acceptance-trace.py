#!/usr/bin/env python3
"""Test current and historical production acceptance trace validation."""

import tempfile
import unittest
from pathlib import Path

from production_acceptance_trace import MIGRATIONS, validate
from historical_trace import load_trace
from usability_acceptance_trace import EXPECTED, RETIRED, rows, validate as validate_usability


class TraceTests(unittest.TestCase):
    def setUp(self):
        self.repository = Path(__file__).resolve().parent.parent
        self.spec = (self.repository / "docs/spec-conformance.md").read_text()
        self.history = load_trace(self.repository, "production-readiness")

    def test_current_and_historical_trace(self):
        self.assertEqual(len(MIGRATIONS), 47)
        validate(self.repository, self.spec, self.history)

    def test_missing_duplicate_and_unrecorded_changes(self):
        for document in ("spec", "history"):
            original = getattr(self, document)
            mutations = [
                original.replace("| DX-01 |", "| DX-02 |", 1),
                original.replace("| REL-01 |", "| REL-04 |", 1),
                original.replace("| OBS-01 |", "| OBS-01 | mutation ", 1),
                original.replace(MIGRATIONS["REL-01"][document == "spec"], "rewritten", 1),
            ]
            for mutated in mutations:
                with self.subTest(document=document, mutation=mutations.index(mutated)):
                    self.assertNotEqual(mutated, original)
                    with self.assertRaises(ValueError):
                        validate(self.repository,
                                 mutated if document == "spec" else self.spec,
                                 mutated if document == "history" else self.history)

    def test_missing_migrated_evidence_markers(self):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            with self.assertRaises(ValueError):
                validate(root, self.spec, self.history)
            for path in (
                "scripts/check-cargo-consumer.sh",
                "crates/codegen/src/contract/check/tests.rs",
                "crates/testkit/tests/rate_conformance/stage_b.rs",
            ):
                target = root / path
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text("No markers, \n.")
            with self.assertRaises(ValueError):
                validate(root, self.spec, self.history)


class HistoricalTraceTests(unittest.TestCase):
    def test_usability_trace_preserves_history_and_explicit_adr_0053_changes(self):
        root = Path(__file__).resolve().parent.parent
        history = load_trace(root, "usability")
        current = (root / "docs/spec-conformance.md").read_text()
        self.assertEqual(set(rows(history)), EXPECTED)
        active = validate_usability(root, current, history)
        self.assertEqual(len(active), 48)
        self.assertEqual(set(active), EXPECTED - RETIRED)
        for document in ("history", "current"):
            original = history if document == "history" else current
            for key in ("API-01", "PROJ-01", "PROJ-07", "PROJ-12", "PROJ-13"):
                line = next(line for line in original.splitlines() if line.startswith(f"| {key} |"))
                for replacement in ("", line + "\n" + line, line.replace(" | ", " | changed ", 1)):
                    with self.subTest(document=document, key=key, replacement=replacement):
                        changed = original.replace(line, replacement, 1)
                        with self.assertRaises(ValueError):
                            validate_usability(root,
                                               changed if document == "current" else current,
                                               changed if document == "history" else history)

    def test_invalid_trace_key_fails(self):
        with self.assertRaises(ValueError):
            load_trace(Path(__file__).resolve().parent.parent, "../usability")


if __name__ == "__main__":
    unittest.main()
