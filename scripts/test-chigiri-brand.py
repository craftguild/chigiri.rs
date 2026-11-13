#!/usr/bin/env python3
"""Mutation tests for the active-brand path and text classifier."""

from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check-chigiri-brand.py")
SPEC = importlib.util.spec_from_file_location("chigiri_brand", SCRIPT)
assert SPEC and SPEC.loader
BRAND = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BRAND)


class BrandInventoryTests(unittest.TestCase):
    def scan(self, entries: dict[str, str]) -> list[str]:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, source in entries.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(source)
            failures, _, _ = BRAND.scan(root)
            return failures

    def test_active_text_and_paths_reject_the_former_brand(self) -> None:
        failures = self.scan(
            {
                "src/lib.rs": "pub const NAME: &str = \"Pythia\";",
                "config/pythia.toml": "current = false",
            }
        )
        self.assertEqual(len(failures), 2)

    def test_history_external_artifacts_and_rejection_owners_are_explicit(self) -> None:
        failures = self.scan(
            {
                "docs/adr/0001-own-graphql-core.md": "Pythia history",
                "scripts/fixtures/historical-acceptance-traces/usability.md": "pythia historical evidence",
                "vendor/stylua-2.5.2/upstream.txt": "PYTHIA external",
                "fuzz/corpus/parser/former-brand": "Pythia user bytes",
                "target/cache/pythia.txt": "build artifact",
                "crates/cli/tests/command_tree.rs": "\"cargo-pythia\"",
                "scripts/chigiri-relocated-acceptance.py": "\"pythia-mock\"",
                "scripts/production_acceptance_trace.py": "pythia historical row",
            }
        )
        self.assertEqual(failures, [])

    def test_normative_document_is_not_hidden_by_the_history_allowlist(self) -> None:
        failures = self.scan({"docs/architecture.md": "Pythia is current"})
        self.assertEqual(failures, ["former brand in active text: docs/architecture.md:1"])

    def test_first_party_vendor_notice_is_scanned(self) -> None:
        failures = self.scan({"vendor/README.md": "Pythia is current"})
        self.assertEqual(failures, ["former brand in active text: vendor/README.md:1"])


if __name__ == "__main__":
    unittest.main()
