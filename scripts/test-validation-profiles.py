#!/usr/bin/env python3
"""Verify the ADR-0058 scope and prevent ordinary evidence replacing full runs."""

import copy
import json
import shlex
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import validate
from validation_evidence import validate_report
from validation_plan import Step, plan
from validation_profiles import COMPLETION_SCHEMA


class ProfileTests(unittest.TestCase):
    def test_boundary_modes_keep_product_conversion_once_and_propagate_its_failure(self):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            scripts = root / "scripts"
            scripts.mkdir()
            checker = scripts / "check-implementation-rules.sh"
            checker.write_bytes((Path(__file__).parent / checker.name).read_bytes())
            (scripts / "python-with-toml.sh").write_text(
                f'exec {shlex.quote(sys.executable)} "$@"\n')
            helper = '''from pathlib import Path
root = Path(__file__).resolve().parents[1]
name = Path(__file__).stem
with (root / "trace").open("a") as trace:
    trace.write(name + "\\n")
if name == "refresh-standards-ownership" and (root / "reject-ownership").exists():
    raise SystemExit(7)
'''
            for stem in ("implementation_rules", "check_repository_language",
                         "check_host_boundary", "refresh-standards-ownership"):
                (scripts / f"{stem}.py").write_text(helper)
            tools = root / "tools"
            tools.mkdir()
            assessor = tools / "standards.sh"
            assessor.write_text('echo assess >> "$(dirname "$0")/../trace"\nprintf "{}\\n"\n')

            for mode, conversion, assessment in (([], 1, 1), (["--product-only"], 1, 0),
                                                  (["--boundaries-only"], 0, 0)):
                with self.subTest(mode=mode):
                    (root / "trace").write_text("")
                    result = subprocess.run(["bash", str(checker), *mode],
                                            capture_output=True, text=True)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    calls = (root / "trace").read_text().splitlines()
                    self.assertEqual(calls.count("refresh-standards-ownership"), conversion)
                    self.assertEqual(calls.count("assess"), assessment)
                    for boundary in ("implementation_rules", "check_repository_language",
                                     "check_host_boundary"):
                        self.assertEqual(calls.count(boundary), 1)

            (root / "reject-ownership").touch()
            for mode in ([], ["--product-only"]):
                with self.subTest(rejected_mode=mode):
                    (root / "trace").write_text("")
                    result = subprocess.run(["bash", str(checker), *mode],
                                            capture_output=True, text=True)
                    self.assertEqual(result.returncode, 7)
                    self.assertNotIn("assess", (root / "trace").read_text().splitlines())

    def test_debug_artifact_is_built_once_and_preparation_is_restored_after_release(self):
        for profile in ("ci", "full"):
            steps = plan(profile)
            builds = [step for step in steps
                      if step.argv[:3] == ("sh", "tools/standards.sh", "build")
                      and "--profile" in step.argv
                      and step.argv[step.argv.index("--profile") + 1] == "debug"]
            self.assertEqual([step.label for step in builds], ["chigiri-build"])
            self.assertNotIn("chigiri-prepare-debug-1.95.0", [step.label for step in steps])
            restore = next(i for i, step in enumerate(steps)
                           if step.label == "chigiri-prepare-contracts-debug-1.95.0")
            release_tests = [i for i, step in enumerate(steps) if "--release" in step.argv]
            self.assertTrue(release_tests)
            self.assertGreater(restore, max(release_tests))
            self.assertEqual(steps[restore].argv, (
                "sh", "tools/standards.sh", "prepare", "--artifact", "chigiri-cli",
                "--toolchain", "1.95.0", "--profile", "debug",
            ))

    def test_ci_keeps_fixed_functional_and_optimized_safety_checks(self):
        ci = {step.label: step for step in plan("ci")}
        required = {
            "workspace-test-1.95.0", "workspace-doctest-1.95.0",
            "workspace-clippy-1.95.0", "workspace-doc-1.95.0",
            "cli-mock-clippy-1.95.0",
            "cli-mock-doc-1.95.0", "mock-test-1.95.0",
            "cli-tests-1.95.0",
            "mock-reentry-release-1.95.0", "mock-runtime-vm-release-1.95.0",
            "repository-consumer-1.95.0", "generated-exact", "generated-surface",
            "fuzz-regression-1.95.0", "dependency-policy",
            "check-production-performance-harness", "test-production-performance",
            "check-usability-acceptance-trace", "implementation-rules",
            "final-implementation-rules",
        }
        self.assertTrue(required.issubset(ci))
        for step in ci.values():
            self.assertFalse(step.label.endswith("-stable"))
            self.assertNotIn("+stable", step.argv)
        self.assertEqual(ci["check-production-performance-harness"].argv,
                         ("./scripts/check-production-performance-harness.sh", "1.95.0"))

    def test_ci_reports_native_failures_early_without_repeating_workspace_tests(self):
        steps = plan("ci")
        labels = [step.label for step in steps]
        native = labels.index("workspace-test-1.95.0")
        self.assertEqual(native, labels.index("chigiri-build") + 1)
        self.assertEqual(labels.count("workspace-test-1.95.0"), 1)
        self.assertLess(native, labels.index("mock-test-1.95.0"))
        self.assertLess(native, labels.index("cli-tests-1.95.0"))
        full = next(step for step in plan("full") if step.label == "workspace-test-1.95.0")
        self.assertEqual(steps[native], full)

    def test_full_retains_broad_checks_and_ci_is_a_strict_subset(self):
        full_steps = plan("full")
        full = {step.label: step for step in full_steps}
        self.assertEqual(len(full), len(full_steps))
        for step in full.values():
            self.assertFalse(step.label.endswith("-stable"))
            self.assertNotIn("+stable", step.argv)
        ci = {step.label: step for step in plan("ci")}
        self.assertLess(set(ci), set(full))
        for label in (
            "host-package", "chigiri-relocated-acceptance", "large-consumer-conformance",
            "generated-quality-1.95.0",
            "full-schema-consumer-1.95.0",
            "mock-all-http-release-1.95.0",
        ):
            self.assertIn(label, full)
            self.assertNotIn(label, ci)
        for label, step in ci.items():
            if label not in ("check-production-performance-harness", "implementation-rules"):
                self.assertEqual(step, full[label])
        self.assertEqual(ci["implementation-rules"].argv,
                         ("./scripts/check-implementation-rules.sh", "--product-only"))
        self.assertEqual(full["implementation-rules"].argv,
                         ("./scripts/check-implementation-rules.sh",))
        for profile in (ci, full):
            self.assertEqual(profile["final-implementation-rules"].argv,
                             ("./scripts/check-implementation-rules.sh", "--boundaries-only"))
            self.assertNotIn("cli-mock-build-1.95.0", profile)
        with self.assertRaises(ValueError):
            plan("unknown")

    def test_profile_substitution_and_incomplete_reports_are_rejected(self):
        steps = [Step("candidate", ("true",), 10)]
        result = {"argv": ["true"], "elapsedSeconds": 1, "exitCode": 0,
                  "label": "candidate", "timedOut": False, "toolchain": "product"}
        report = {"schema_version": COMPLETION_SCHEMA, "profile": "ci",
                  "source_sha256": "a" * 64, "commands": [result]}
        validate_report("ci", steps, report)
        with self.assertRaises(ValueError):
            validate_report("full", steps, report)
        for field, value in (
            ("schema_version", "chigiri/product-validation-v1"),
            ("profile", "unknown"), ("source_sha256", "unknown"), ("commands", []),
        ):
            changed = dict(report, **{field: value})
            with self.subTest(field=field), self.assertRaises(ValueError):
                validate_report("ci", steps, changed)
        for field in report:
            changed = dict(report)
            del changed[field]
            with self.assertRaises(ValueError):
                validate_report("ci", steps, changed)
        changed = copy.deepcopy(report)
        changed["commands"][0]["exitCode"] = 1
        with self.assertRaises(ValueError):
            validate_report("ci", steps, changed)
        with self.assertRaises(ValueError):
            validate_report("ci", steps, dict(report, extra=True))

    def test_cli_defaults_to_ci_and_records_explicit_profiles(self):
        for profile, arguments in (("ci", []), ("full", ["--profile", "full"])):
            with self.subTest(profile=profile), tempfile.TemporaryDirectory() as name:
                root = Path(name)
                step = Step("candidate", ("true",), 10)
                result = {"argv": ["true"], "elapsedSeconds": 1, "exitCode": 0,
                          "label": "candidate", "timedOut": False, "toolchain": "product"}
                with patch.object(validate, "__file__", str(root / "scripts/validate.py")), \
                        patch.object(validate, "plan", return_value=[step]) as selected, \
                        patch.object(validate, "run_step", return_value=result), \
                        patch("sys.argv", ["validate.py", *arguments]):
                    validate.main()
                selected.assert_called_once_with(profile)
                completed, = root.glob("target/product-validation/*/completed.json")
                report = json.loads(completed.read_text())
                validate_report(profile, [step], report)
        with patch("sys.argv", ["validate.py", "--profile", "unknown"]), \
                self.assertRaises(SystemExit):
            validate.main()


if __name__ == "__main__":
    unittest.main()
