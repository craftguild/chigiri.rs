#!/usr/bin/env python3
"""Reject stale, incomplete or substituted evidence in shared language adapters."""

import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import subprocess

from validation_evidence import source_snapshot
from validation_plan import Step
from validation_profiles import COMPLETION_SCHEMA

SPEC = importlib.util.spec_from_file_location("standards_language", Path(__file__).with_name("standards-language.py"))
LANGUAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(LANGUAGE)


class LanguageEvidence(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "FIGlet-LICENSE").write_text("Owned source input without a conventional extension.\n")
        self.step = Step("fixture-language-gate", ("true",))
        digest = source_snapshot(self.root)["sha256"]
        self.report = {"schema_version": COMPLETION_SCHEMA, "profile": "ci", "source_sha256": digest,
                       "commands": [{"argv": ["true"], "elapsedSeconds": 1, "exitCode": 0,
                                     "label": self.step.label, "timedOut": False, "toolchain": "product"}]}
        self.marker = {"schema_version": 1, "profile": "ci", "source_sha256": digest,
                       "completion": "target/product-validation/run-fixture/completed.json"}
        self.write()

    def write(self):
        destination = self.root / "target/product-validation/run-fixture/completed.json"
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(json.dumps(self.report))
        directory = self.root / ".craftguild"
        directory.mkdir(exist_ok=True)
        (directory / "current-ci.json").write_text(json.dumps(self.marker))

    def verify(self):
        with patch.object(LANGUAGE, "ROOT", self.root), patch.object(LANGUAGE, "plan", return_value=[self.step]):
            LANGUAGE.product_receipt([self.step.label])

    def test_actual_source_digest_includes_unfamiliar_asset_names(self):
        self.verify()
        (self.root / "FIGlet-LICENSE").write_text("Changed license input.\n")
        with self.assertRaisesRegex(ValueError, "current complete source"):
            self.verify()

    def test_missing_failed_reordered_and_wrong_profile_results_cannot_satisfy_a_gate(self):
        original = copy.deepcopy(self.report)
        for change in (lambda r: r.update(commands=[]),
                       lambda r: r.update(profile="full"),
                       lambda r: r["commands"][0].update(exitCode=1),
                       lambda r: r["commands"][0].update(timedOut=True),
                       lambda r: r["commands"][0].update(argv=["false"])):
            self.report = copy.deepcopy(original)
            change(self.report)
            self.write()
            with self.assertRaises(ValueError):
                self.verify()

    def test_escaping_receipt_paths_and_missing_markers_are_rejected(self):
        for path in ("/tmp/foreign.json", "../foreign.json", "target/../foreign.json"):
            self.marker["completion"] = path
            self.write()
            with self.assertRaises(ValueError):
                self.verify()
        (self.root / ".craftguild/current-ci.json").unlink()
        with self.assertRaises(FileNotFoundError):
            self.verify()


    def test_missing_or_stale_catalog_is_executed_then_verified(self):
        for missing in (FileNotFoundError("missing"), ValueError("stale source")):
            with patch.object(LANGUAGE, "product_receipt", side_effect=[missing, None]) as receipt, \
                    patch.object(LANGUAGE.subprocess, "run") as execute:
                LANGUAGE.ensure_product_receipt()
                self.assertEqual(receipt.call_count, 2)
                execute.assert_called_once_with(
                    ["bash", "scripts/validate.sh", "--profile", "ci"],
                    cwd=LANGUAGE.ROOT, check=True)

    def test_valid_catalog_is_reused_and_validator_failure_propagates(self):
        with patch.object(LANGUAGE, "product_receipt") as receipt, \
                patch.object(LANGUAGE.subprocess, "run") as execute:
            LANGUAGE.ensure_product_receipt()
            receipt.assert_called_once_with([])
            execute.assert_not_called()
        failure = subprocess.CalledProcessError(1, ["validator"])
        with patch.object(LANGUAGE, "product_receipt", side_effect=ValueError("stale")), \
                patch.object(LANGUAGE.subprocess, "run", side_effect=failure):
            with self.assertRaises(subprocess.CalledProcessError):
                LANGUAGE.ensure_product_receipt()


if __name__ == "__main__":
    unittest.main()
