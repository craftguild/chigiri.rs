#!/usr/bin/env python3
"""Reject legacy or incomplete large-consumer diagnostic evidence."""

import copy
import importlib.util
import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True

spec = importlib.util.spec_from_file_location(
    "large_consumer_contract", Path(__file__).with_name("large-consumer-contract.py")
)
contract = importlib.util.module_from_spec(spec)
spec.loader.exec_module(contract)


def diagnostic(code, origin, message):
    return {"code": code, "origin": origin, "severity": "error", "message": message}


def report(records):
    return {
        "schema_version": "chigiri/diagnostics-v2",
        "diagnostics": records,
        "truncated": False,
    }


class TraitMutationTests(unittest.TestCase):
    def test_missing_field_and_wrong_output_isolate_one_trait_error(self):
        fixture = Path(__file__).resolve().parent.parent / "tests/fixtures/vnext-large-product/consumer/src/products_00.rs"
        original = fixture.read_text(encoding="utf-8")
        for case in ("missing-field", "wrong-output"):
            with self.subTest(case=case), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                path = root / "consumer/src/products_00.rs"
                path.parent.mkdir(parents=True)
                shutil.copyfile(fixture, path)
                contract.mutate(root, case)
                changed = path.read_text(encoding="utf-8")
                self.assertIn("impl ::generated::ProductObject001Resolver", changed)
                self.assertIn("async fn related<'a>(", changed)
                if case == "missing-field":
                    self.assertNotIn("async fn name<'a>(", changed)
                    self.assertNotIn("removed_name", changed)
                    self.assertIn("chigiri::FieldResult<chigiri::Id>", changed)
                else:
                    self.assertIn("async fn name<'a>(", changed)
                    self.assertNotIn("chigiri::FieldResult<chigiri::Id>", changed)
                    self.assertEqual(changed.count('Ok("synthetic".to_owned())'), 2)
                self.assertEqual(fixture.read_text(encoding="utf-8"), original)


class DiagnosticReaderTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.log = Path(self.temporary.name) / "command.log"
        self.output = Path(self.temporary.name) / "evidence.json"

    def write(self, value):
        self.log.write_text(json.dumps(value) + "\n0.02 real 0.01 user\n", encoding="utf-8")

    def test_current_report_preserves_originals_and_one_supplement(self):
        native = diagnostic("E0271", "rustc", "mismatched types")
        hint = diagnostic("SF4019", "chigiri", "ProductObject001.id: ID! / ::chigiri::Id")
        self.write(report([native, native, hint]))
        contract.diagnostic("wrong-output", "1.95.0", self.log, 1, self.output)
        evidence = json.loads(self.output.read_text())
        self.assertEqual(evidence["rustcCode"], "E0271")
        self.assertEqual(evidence["chigiriCode"], "SF4019")
        for records in ([native, native], [hint], [native, hint, hint]):
            with self.subTest(records=records):
                self.write(report(records))
                with self.assertRaises(SystemExit):
                    contract.diagnostic("wrong-output", "1.95.0", self.log, 1, self.output)

    def test_legacy_multiple_unknown_duplicate_and_truncated_are_rejected(self):
        good = report([diagnostic("SF4018", "chigiri", "field")])
        truncated = copy.deepcopy(good)
        truncated["truncated"] = True
        unknown = dict(good, unexpected=True)
        old = dict(good, schema_version="chigiri/diagnostics-v1")
        samples = [json.dumps(value) for value in [good["diagnostics"][0], truncated, unknown, old, []]]
        samples.extend([
            json.dumps(good) + "\n" + json.dumps(good),
            json.dumps(good) + "\nfalse",
            json.dumps(good).replace('"truncated": false', '"truncated": false, "truncated": false'),
        ])
        for sample in samples:
            with self.subTest(sample=sample):
                self.log.write_text(sample + "\n", encoding="utf-8")
                with self.assertRaises(SystemExit):
                    contract.read_diagnostics(self.log)

    def test_static_failure_requires_inventory_cause_and_no_cargo(self):
        summary = diagnostic("SF4016", "chigiri", "generated crate: chigiri rust codegen")
        cause = diagnostic("SF4020", "chigiri", "inventory")
        self.write(report([summary, cause]))
        contract.diagnostic("generated-drift", "1.95.0", self.log, 1, self.output)
        for records in ([summary], [summary, cause, diagnostic("SF4017", "cargo", "check")]):
            with self.subTest(records=records):
                self.write(report(records))
                with self.assertRaises(SystemExit):
                    contract.diagnostic("generated-drift", "1.95.0", self.log, 1, self.output)

    def test_positive_requires_observed_locked_all_targets_check(self):
        invocation = diagnostic(
            "SF4017", "cargo",
            "Ran a real consumer cargo check. argv: cargo check --locked --all-targets chigiri_vnext_large_consumer",
        )
        invocation["severity"] = "note"
        self.write(report([invocation]))
        contract.positive(self.log)
        for key, value in [("origin", "chigiri"), ("severity", "warning"), ("message", "synthetic warning")]:
            with self.subTest(key=key):
                changed = dict(invocation, **{key: value})
                self.write(report([changed]))
                with self.assertRaises(SystemExit):
                    contract.positive(self.log)


if __name__ == "__main__":
    unittest.main()
