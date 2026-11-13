#!/usr/bin/env python3
"""Reject SG-ID and test-owner trace defects."""

import unittest
from pathlib import Path

from sibling_acceptance_trace import EXPECTED, validate


class TraceTests(unittest.TestCase):
    def test_exact_owners_and_mutations(self):
        root = Path(__file__).resolve().parent.parent
        spec = (root / "docs/spec-conformance.md").read_text()
        trace = (root / "docs/sibling-generator-test-trace.md").read_text()
        self.assertGreater(validate(root, spec, trace), len(EXPECTED))
        mutations = [
            (spec.replace("| SG-CLI-01 |", "| SG-CLI-02 |", 1), trace),
            (spec.replace("#sg-cli-01)", "#sg-cli-02)", 1), trace),
            (spec, trace.replace("## SG-CLI-01\n", "## SG-CLI-02\n", 1)),
            (spec, trace.replace("## SG-CLI-01\n", "## SG-CLI-99\n", 1)),
            (spec, trace.replace("crates/cli/tests/rust_terminal.rs#", "crates/cli/tests/missing.rs#", 1)),
            (spec, trace.replace("#sg_cli_01_help", "#missing_help", 1)),
        ]
        first = next(line for line in trace.splitlines() if line.startswith("- `"))
        mutations.append((spec, trace.replace(first, first + "\n" + first, 1)))
        mutations.append((spec, trace.replace(first + "\n", "", 1)))
        for changed_spec, changed_trace in mutations:
            with self.subTest(mutation=mutations.index((changed_spec, changed_trace))):
                self.assertNotEqual((changed_spec, changed_trace), (spec, trace))
                with self.assertRaises(ValueError):
                    validate(root, changed_spec, changed_trace)


if __name__ == "__main__":
    unittest.main()
