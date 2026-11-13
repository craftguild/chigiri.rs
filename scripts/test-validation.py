#!/usr/bin/env python3
"""Ensure missing checks, failures, cancellation, and source changes cannot pass."""

import copy
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import patch

import validate
from validation_evidence import source_snapshot, validate_completion
from validation_plan import CLI_TEST_TARGETS, MOCK_TEST_TARGETS, NORMAL_PACKAGES, Step, plan
from validation_sources import copy_corpus


SCRIPTS = Path(__file__).resolve().parent


class ValidationTests(unittest.TestCase):
    def test_subscription_recipe_is_registered_once_with_its_official_command(self):
        steps = plan()
        recipes = [step for step in steps if step.label == "subscription-recipes"]
        self.assertEqual(len(recipes), 1)
        self.assertEqual(recipes[0].argv, ("./scripts/check-subscription-recipes.sh",))
        self.assertEqual(sum(step.argv == recipes[0].argv for step in steps), 1)
        self.assertGreater(recipes[0].seconds, 0)
        self.assertLessEqual(recipes[0].seconds, 600)

    def test_cli_and_mock_are_checked_separately_from_normal_toolchains(self):
        steps = {step.label: step.argv for step in plan()}
        for operation in ("clippy", "test", "doc"):
            for toolchain in ("1.95.0",):
                command = steps[f"workspace-{operation}-{toolchain}"]
                self.assertIn("chigiri_mock", command)
                self.assertIn("chigiri_cli", command)
                self.assertEqual(command.count("--exclude"), 2)
        self.assertNotIn("cli-mock-build-1.95.0", steps)
        for operation in ("clippy", "doc"):
            for toolchain in ("1.95.0",):
                command = steps[f"cli-mock-{operation}-{toolchain}"]
                self.assertIn(f"+{toolchain}", command)
                self.assertIn("chigiri_mock", command)
                self.assertIn("chigiri_cli", command)
        self.assertTrue({"chigiri_mock", "chigiri_cli"}.isdisjoint(NORMAL_PACKAGES))
        for toolchain in ("1.95.0",):
            command = steps[f"mock-test-{toolchain}"]
            for selector in ("--lib", "--examples", "--benches"):
                self.assertIn(selector, command)
            self.assertEqual(command[-2:], ("--", "--test-threads=1"))
            for target in MOCK_TEST_TARGETS:
                self.assertEqual(steps[f"mock-test-{target}-{toolchain}"], (
                    "cargo", f"+{toolchain}", "test", "--locked", "-p", "chigiri_mock",
                    "--all-features", "--test", target,
                ))
            command = steps[f"cli-tests-{toolchain}"]
            self.assertEqual(command[:7], (
                "cargo", f"+{toolchain}", "test", "--locked", "-p", "chigiri_cli",
                "--all-features",
            ))
            self.assertEqual(command[7::2], ("--test",) * len(CLI_TEST_TARGETS))
            self.assertCountEqual(command[8::2], CLI_TEST_TARGETS)
            self.assertEqual(len(command[8::2]), len(set(command[8::2])))
            self.assertFalse(any(label.startswith("cli-test-") for label in steps))
            self.assertEqual(steps[f"cli-mock-doctest-{toolchain}"], (
                "cargo", f"+{toolchain}", "test", "--locked", "-p", "chigiri_mock",
                "-p", "chigiri_cli", "--all-features", "--doc",
            ))
        for toolchain in ("1.95.0",):
            self.assertEqual(steps[f"workspace-doctest-{toolchain}"], (
                "cargo", f"+{toolchain}", "test", "--locked", "--workspace", "--all-features",
                "--exclude", "chigiri_mock", "--exclude", "chigiri_cli", "--doc",
            ))
        self.assertEqual(len(NORMAL_PACKAGES), 20)
        self.assertIn("chigiri_host", NORMAL_PACKAGES)
        self.assertIn("chigiri_graphiql", NORMAL_PACKAGES)
        self.assertIn("chigiri_subscriptions_example", NORMAL_PACKAGES)
        self.assertEqual(steps["host-boundary-regression"], ("./scripts/test-host-boundary.sh",))
        self.assertEqual(steps["host-manifest"], (
            "bash", "scripts/python-with-toml.sh", "scripts/check-host-manifest.py",
        ))
        self.assertEqual(steps["host-manifest-regression"], (
            "bash", "scripts/python-with-toml.sh", "scripts/test-host-manifest.py",
        ))
        self.assertEqual(steps["host-package"], (
            "cargo", "+1.95.0", "package", "--locked", "-p", "chigiri_host", "--allow-dirty",
        ))
        self.assertIn("+1.95.0", steps["cli-mock-format"])
        self.assertIn("chigiri-build", steps)
        self.assertEqual(steps["chigiri-relocated-acceptance"], (
            "python3", "scripts/chigiri-relocated-acceptance.py",
        ))
        self.assertIn("mock-contract", steps)
        self.assertEqual(steps["mock-reentry-release-1.95.0"], (
            "cargo", "+1.95.0", "test", "--locked", "--release", "-p", "chigiri_cli",
            "--all-features", "--test", "mock_cli", "reentry::", "--", "--nocapture",
        ))
        self.assertEqual(steps["mock-runtime-vm-release-1.95.0"], (
            "cargo", "+1.95.0", "test", "--locked", "--release", "-p", "chigiri_mock",
            "--all-features", "--test", "runtime_vm", "--", "--nocapture",
        ))
        for label, package, targets in (
            ("mock-model-unit-release-1.95.0", "chigiri_mock", ("--lib",)),
            ("mock-model-http-release-1.95.0", "chigiri_cli", ("--test", "mock_cli", "runtime_schema::model_api::")),
            ("mock-project-http-release-1.95.0", "chigiri_cli", ("--test", "mock_cli", "project_")),
            ("mock-project-vm-release-1.95.0", "chigiri_mock", ("--test", "project_vm", "--test", "module_vm")),
            ("mock-reference-release-1.95.0", "chigiri_cli", ("--test", "mock_reference")),
            ("mock-vm-spike-release-1.95.0", "chigiri_mock", ("--test", "vm_spike")),
            ("mock-graphiql-release-1.95.0", "chigiri_cli", ("--test", "mock_cli", "graphiql::")),
            ("mock-hooks-http-release-1.95.0", "chigiri_cli", ("--test", "mock_cli", "hk_")),
            ("mock-all-http-release-1.95.0", "chigiri_cli", ("--test", "mock_cli")),
        ):
            self.assertEqual(steps[label], (
                "cargo", "+1.95.0", "test", "--locked", "--release", "-p", package,
                "--all-features", *targets, "--", "--nocapture",
            ))
        self.assertEqual(steps["index-whitespace"], ("git", "diff", "--cached", "--check"))
        for version in ("1.95.0",):
            self.assertIn(f"RUSTUP_TOOLCHAIN={version}", steps[f"mock-comparison-doctor-{version}"])
        workflow = (SCRIPTS.parent / ".github/workflows/product-validation.yml").read_text()
        self.assertIn('node-version: "24.15.0"', workflow)
        for label, arguments in (
            ("graphiql-assets-inventory", ("scripts/graphiql_inventory.py",)),
            ("graphiql-assets-regression", ("scripts/test-graphiql-inventory.py",)),
            ("graphiql-assets-drift", ("scripts/graphiql_build.py", "--check", "--typecheck")),
            ("graphiql-assets-license", ("scripts/graphiql_inventory.py", "licenses")),
            ("graphiql-consumer-feature", ("scripts/check-graphiql-contract.py",)),
        ):
            self.assertEqual(steps[label], ("python3", *arguments))
        for version in ("1.95.0",):
            self.assertIn(f"rustup toolchain install {version} --profile minimal --component clippy --component rustfmt", workflow)

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name).resolve()
        (self.root / "scripts").mkdir()
        shutil.copy(SCRIPTS / "run-command-with-timeout.py", self.root / "scripts")

    def tearDown(self):
        self.temporary.cleanup()

    def test_rejects_missing_duplicate_out_of_order_failed_skipped_and_cancelled_results(self):
        steps = plan()
        valid = [{"argv": list(step.argv), "elapsedSeconds": 1, "exitCode": 0,
                  "label": step.label, "timedOut": False, "toolchain": "product"} for step in steps]
        validate_completion(steps, valid)
        for index in range(len(steps)):
            with self.subTest(label=steps[index].label):
                for field, value in (("exitCode", 1), ("exitCode", 143), ("exitCode", "skipped"),
                                     ("exitCode", False), ("timedOut", True), ("argv", []),
                                     ("label", "missing"), ("elapsedSeconds", float("nan")),
                                     ("elapsedSeconds", steps[index].seconds + 1)):
                    changed = copy.deepcopy(valid)
                    changed[index][field] = value
                    with self.assertRaises(ValueError):
                        validate_completion(steps, changed)
                with self.assertRaises(ValueError):
                    validate_completion(steps, valid[:index] + valid[index + 1:])
        for changed in (valid + valid[:1], list(reversed(valid))):
            with self.assertRaises(ValueError):
                validate_completion(steps, changed)
        with self.assertRaises(ValueError):
            validate_completion(steps + steps[:1], valid + valid[:1])

    def run_candidate(self, command):
        step = Step("candidate", tuple(command), 10)
        with patch.object(validate, "__file__", str(self.root / "scripts/validate.py")), \
                patch.object(validate, "plan", return_value=[step]), patch.object(sys, "argv", ["validate.py"]):
            validate.main()

    def test_completion_requires_all_commands_to_pass_against_unchanged_source(self):
        self.run_candidate([sys.executable, "-c", "pass"])
        self.assertEqual(len(list(self.root.glob("target/product-validation/*/completed.json"))), 1)
        (self.root / "Cargo.toml").write_text("before")
        with self.assertRaisesRegex(ValueError, "source tree"):
            self.run_candidate([sys.executable, "-c", "from pathlib import Path; Path('Cargo.toml').write_text('after')"])
        self.assertEqual(len(list(self.root.glob("target/product-validation/*/completed.json"))), 1)
        with self.assertRaisesRegex(ValueError, "ended with nonzero"):
            self.run_candidate([sys.executable, "-c", "raise SystemExit(1)"])
        self.assertEqual(len(list(self.root.glob("target/product-validation/*/completed.json"))), 1)

    def test_source_inventory_excludes_local_state_and_includes_untracked_sources(self):
        before = source_snapshot(self.root)
        (self.root / "consumer.rs").write_text("Handwriting.")
        (self.root / "fuzz/corpus/config_parser").mkdir(parents=True)
        (self.root / "fuzz/corpus/config_parser/valid").write_text("Fixed input.")
        inputs = ("Makefile", "LICENSE", "golden.txt", "tests/fixture/input.bin", "crates/example/tests/fixture.raw", "crates/mock/src/types.luau", "crates/mock/fixtures/seed.jsonl",
                  "crates/graphiql/frontend/entry.mjs", "crates/graphiql/assets/app.js", "crates/graphiql/assets/app.css", "crates/graphiql/assets/codicon.ttf",
                  "frontend/view.tsx", "frontend/index.html", "vendor/pinned/LICENSE", "vendor/pinned/source.raw")
        for name in inputs:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("Check in the file.")
        after = source_snapshot(self.root)
        self.assertNotEqual(before["sha256"], after["sha256"])
        names = {file["path"] for file in after["files"]}
        self.assertIn("consumer.rs", names)
        self.assertIn("fuzz/corpus/config_parser/valid", names)
        for name in inputs:
            self.assertIn(name, names)
            (self.root / name).write_text("Change the way you look.")
            self.assertNotEqual(source_snapshot(self.root), after)
            (self.root / name).write_text("Check in the file.")
        (self.root / "Makefile").unlink()
        self.assertNotEqual(source_snapshot(self.root), after)
        (self.root / "Makefile").write_text("Check in the file.")
        (self.root / "target").mkdir()
        (self.root / "target/cache.rs").write_text("generated cache entry")
        for directory in (".claude", ".direnv", ".idea", ".vscode"):
            local = self.root / directory / "settings.json"
            local.parent.mkdir()
            local.write_text("local state")
        self.assertEqual(source_snapshot(self.root), after)

    def test_timeout_and_term_stop_the_child_process(self):
        for cancelled in (False, True):
            with self.subTest(cancelled=cancelled):
                ready = self.root / "ready"
                ready.unlink(missing_ok=True)
                result = self.root / "result.json"
                code = "import os,time; from pathlib import Path; Path(%r).write_text(str(os.getpid())); time.sleep(60)" % str(ready)
                process = subprocess.Popen([
                    sys.executable, str(SCRIPTS / "run-command-with-timeout.py"),
                    "10" if cancelled else "0.5", str(self.root / "child.log"), sys.executable, "-c", code,
                ], env=dict(os.environ, CHIGIRI_COMMAND_RESULT=str(result)))
                try:
                    deadline = time.monotonic() + 5
                    while not ready.exists() and time.monotonic() < deadline:
                        time.sleep(0.01)
                    self.assertTrue(ready.exists())
                    if cancelled:
                        process.send_signal(signal.SIGTERM)
                    self.assertEqual(process.wait(timeout=10), 143 if cancelled else 124)
                    report = json.loads(result.read_text())
                    self.assertEqual(report["exitCode"], 143 if cancelled else 124)
                    self.assertEqual(report["timedOut"], not cancelled)
                    child = int(ready.read_text())
                    state = subprocess.run(["ps", "-o", "stat=", "-p", str(child)], capture_output=True, text=True).stdout.strip()
                    self.assertTrue(not state or state.startswith("Z"), state)
                finally:
                    if process.poll() is None:
                        process.terminate()
                        process.wait(timeout=10)

    def test_source_snapshot_is_independent_of_beads_review_and_database_state(self):
        (self.root / "Cargo.toml").write_text("product source")
        before = source_snapshot(self.root)
        state = self.root / ".beads/backup/backup_state.json"
        state.parent.mkdir(parents=True)
        for contents in ('{"status":"open"}', '{"status":"closed","review":"Verified"}'):
            state.write_text(contents)
            self.assertEqual(source_snapshot(self.root), before)
        shutil.rmtree(self.root / ".beads")
        self.assertEqual(source_snapshot(self.root), before)

    def test_git_inventory_keeps_fixed_and_untracked_sources_but_excludes_ignored_corpus(self):
        subprocess.run(["git", "init", "--quiet", str(self.root)], check=True)
        (self.root / ".gitignore").write_text("fuzz/corpus/*/*\n")
        rows = []
        for target in ("parser", "sdl_composition", "http_decode", "json_conversion", "persisted_id", "config_parser"):
            name = "fixed" if target == "parser" else "seed"
            path = self.root / "fuzz/corpus" / target / name
            path.parent.mkdir(parents=True)
            path.write_text("fixed regression")
            rows.append(f"{target}/{name}\n")
            subprocess.run(["git", "-C", str(self.root), "add", "-f", str(path)], check=True)
        manifest = self.root / "fuzz/fixed-corpus.txt"
        contents = "".join(rows)
        manifest.write_text(contents)
        corpus = self.root / "fuzz/corpus/parser"
        before = source_snapshot(self.root)
        self.assertIn("fuzz/corpus/parser/fixed", {item["path"] for item in before["files"]})
        (corpus / "discovered").write_bytes(b"x" * (256 * 1024 + 1))
        self.assertEqual(source_snapshot(self.root), before)
        copied = self.root / "target/copied-corpus"
        copy_corpus(self.root, "parser", copied)
        self.assertEqual({file.name for file in copied.iterdir()}, {"fixed"})
        for changed in ("".join(rows[1:]), contents + rows[0], contents + "parser/../escape\n",
                        contents + "unknown/input\n", contents + "\n", contents + "parser/.\n"):
            manifest.write_text(changed)
            with self.subTest(inventory=changed), self.assertRaises(ValueError):
                copy_corpus(self.root, "parser", copied)
        manifest.write_text(contents)
        unregistered = corpus / "unregistered"
        unregistered.write_text("authored but unregistered")
        subprocess.run(["git", "-C", str(self.root), "add", "-f", str(unregistered)], check=True)
        with self.assertRaises(ValueError):
            copy_corpus(self.root, "parser", copied)
        (self.root / "new.rs").write_text("authored source")
        self.assertNotEqual(source_snapshot(self.root), before)

    def test_aggregate_cancellation_does_not_create_completion(self):
        ready = self.root / "target/ready"
        command = [sys.executable, "-c", "import os,time; from pathlib import Path; Path(%r).write_text(str(os.getpid())); time.sleep(60)" % str(ready)]
        source = (
            f"import sys; sys.path.insert(0, {str(SCRIPTS)!r}); import validate; from validation_plan import Step\n"
            f"validate.__file__ = {str(self.root / 'scripts/validate.py')!r}\n"
            f"validate.plan = lambda profile: [Step('cancelled-child', {tuple(command)!r}, 30)]\n"
            "sys.argv = ['validate.py']\n"
            "try: validate.main()\n"
            "except validate.ValidationCancelled as error: raise SystemExit(128 + error.signum)\n"
        )
        process = subprocess.Popen([sys.executable, "-c", source], stdout=subprocess.DEVNULL)
        try:
            deadline = time.monotonic() + 5
            while not ready.exists() and time.monotonic() < deadline:
                time.sleep(0.01)
            self.assertTrue(ready.exists())
            process.send_signal(signal.SIGTERM)
            self.assertEqual(process.wait(timeout=15), 143)
            self.assertFalse(list(self.root.glob("target/product-validation/*/completed.json")))
            reports = list(self.root.glob("target/product-validation/*/cancelled-child.json"))
            self.assertEqual(len(reports), 1)
            self.assertEqual(json.loads(reports[0].read_text())["exitCode"], 143)
            state = subprocess.run(["ps", "-o", "stat=", "-p", ready.read_text()], capture_output=True, text=True).stdout.strip()
            self.assertTrue(not state or state.startswith("Z"), state)
        finally:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=15)


if __name__ == "__main__":
    unittest.main()
