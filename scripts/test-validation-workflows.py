#!/usr/bin/env python3
"""Verify native profile gates, weekly scheduling and isolated fuzz evidence."""

import os
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

from fixed_fuzz_corpus import corpus_inventory
from validation_sources import source_paths


ROOT = Path(__file__).resolve().parent.parent
SCRIPTS = ROOT / "scripts"


def job(workflow, name):
    match = re.search(rf"^  {re.escape(name)}:\n(.*?)(?=^  [\w-]+:|\Z)",
                      workflow.split("jobs:\n", 1)[1], re.MULTILINE | re.DOTALL)
    if match is None:
        raise ValueError(f"Required workflow job is missing: {name}")
    return match.group(1)


class WorkflowTests(unittest.TestCase):
    def test_fixed_input_inventory_matches_the_authored_repository_corpus(self):
        inventory = corpus_inventory(ROOT, source_paths(ROOT))
        self.assertEqual(set(inventory), {"parser", "sdl_composition", "http_decode",
                                          "json_conversion", "persisted_id", "config_parser"})

    def test_native_ci_and_weekly_full_use_distinct_profile_receipts(self):
        ci = (ROOT / ".github/workflows/ci.yml").read_text()
        weekly = (ROOT / ".github/workflows/extended-validation.yml").read_text()
        for document, profile in ((ci, "ci"), (weekly, "full")):
            for name, operating_system in (("linux", "Linux"), ("macos", "Darwin")):
                block = job(document, name)
                self.assertIn("uses: ./.github/workflows/product-validation.yml", block)
                self.assertIn(f"expected-os: {operating_system}", block)
                self.assertIn(f"profile: {profile}", block)
            aggregate = job(document, "required" if profile == "ci" else "full-required")
            self.assertIn("if: ${{ always() }}", aggregate)
            self.assertIn("needs: [linux, macos]", aggregate)
            self.assertIn("python3 scripts/check-required-jobs.py", aggregate)
        self.assertIn("CHIGIRI_VALIDATION_PROFILE: full", job(weekly, "full-required"))
        reusable = (ROOT / ".github/workflows/product-validation.yml").read_text()
        self.assertIn('./scripts/validate.sh --profile "$VALIDATION_PROFILE"', reusable)
        self.assertIn(
            'if [[ "$VALIDATION_PROFILE" == ci ]]; then\n'
            '            ./scripts/validate.sh --profile ci\n'
            '            make check',
            reusable,
        )
        self.assertIn('cargo +1.95.0 install tokei --version 15.0.0 --locked', reusable)
        self.assertIn('echo "profile=$VALIDATION_PROFILE" >> "$GITHUB_OUTPUT"', reusable)
        self.assertIn("            target/product-validation/\n", reusable)
        self.assertIn("            target/production-evidence/\n", reusable)

    def test_long_runs_have_one_weekly_schedule_and_preserve_failure_artifacts(self):
        weekly = (ROOT / ".github/workflows/extended-validation.yml").read_text()
        fuzz = (ROOT / ".github/workflows/fuzz.yml").read_text()
        self.assertEqual(weekly.count('cron: "17 15 * * 0"'), 1)
        self.assertNotIn("schedule:", fuzz)
        self.assertIn("workflow_dispatch:", weekly)
        self.assertIn("uses: ./.github/workflows/fuzz.yml", job(weekly, "fuzz"))
        for target in ("parser", "sdl_composition", "http_decode", "json_conversion",
                       "persisted_id", "config_parser"):
            self.assertIn(f"          - {target}\n", fuzz)
        self.assertIn('CHIGIRI_FUZZ_SECONDS: "1800"', fuzz)
        self.assertIn("cargo +nightly install cargo-fuzz --locked", fuzz)
        self.assertIn('./scripts/run-release-fuzz.sh --target "$FUZZ_TARGET"', fuzz)
        self.assertIn("timeout-minutes: 60", fuzz)
        workload = job(weekly, "load-soak")
        self.assertIn("./scripts/run-release-load-soak.sh 1.95.0", workload)
        self.assertIn("timeout-minutes: 60", workload)
        for document in (fuzz, workload,
                         (ROOT / ".github/workflows/product-validation.yml").read_text()):
            self.assertRegex(document, r"if: \$\{\{ always\(\) \}\}\n\s+uses: actions/upload-artifact@v4")
            self.assertIn("retention-days: 30", document)


class FuzzIsolationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        scripts = self.root / "scripts"
        scripts.mkdir()
        for name in ("run-release-fuzz.sh", "validation_evidence.py",
                     "implementation_rule_io.py", "validation_profiles.py", "validation_sources.py",
                     "fixed_fuzz_corpus.py"):
            shutil.copy(SCRIPTS / name, scripts / name)
        rows = []
        for target in ("parser", "sdl_composition", "http_decode", "json_conversion", "persisted_id", "config_parser"):
            path = self.root / "fuzz/corpus" / target / "seed"
            path.parent.mkdir(parents=True)
            path.write_text('{"fixed":true}\n')
            rows.append(f"{target}/seed\n")
        (self.root / "fuzz/fixed-corpus.txt").write_text("".join(rows))
        self.seed = self.root / "fuzz/corpus/json_conversion/seed"
        self.bin = self.root / "bin"
        self.bin.mkdir()
        cargo = self.bin / "cargo"
        cargo.write_text('''#!/usr/bin/env bash
set -euo pipefail
if [[ "$*" == "+nightly --version" || "$*" == "+nightly fuzz --version" ]]; then
  echo "test toolchain"
elif [[ "$1 $2 $3" == "+nightly fuzz run" ]]; then
  printf 'new input' >"$5/discovered"
  if [[ "${MUTATE_FUZZ_SOURCE:-0}" == 1 ]]; then
    printf 'changed source' >"fuzz/corpus/json_conversion/seed"
  fi
  exit "${FAKE_FUZZ_STATUS:-0}"
else
  exit 99
fi
''')
        cargo.chmod(0o755)
        rustc = self.bin / "rustc"
        rustc.write_text("#!/bin/sh\nprintf 'test compiler\\n'\n")
        rustc.chmod(0o755)

    def tearDown(self):
        self.temporary.cleanup()

    def run_fuzz(self, **extra):
        environment = dict(os.environ, PATH=f"{self.bin}:{os.environ['PATH']}",
                           CHIGIRI_FUZZ_SECONDS="1", **extra)
        return subprocess.run(["bash", "scripts/run-release-fuzz.sh", "--target", "json_conversion"],
                              cwd=self.root, env=environment, capture_output=True, text=True, timeout=10)

    def test_discovered_inputs_are_isolated_and_evidence_survives_failure(self):
        before = self.seed.read_bytes()
        for status in ("0", "23"):
            result = self.run_fuzz(FAKE_FUZZ_STATUS=status)
            self.assertEqual(result.returncode, int(status), result.stderr)
            self.assertEqual(self.seed.read_bytes(), before)
        runs = list((self.root / "target/production-evidence/fuzz").glob("run-*"))
        self.assertEqual(len(runs), 2)
        for run in runs:
            for name in ("source.json", "rustc.txt", "revision.txt", "json_conversion.log",
                         "corpus/json_conversion/discovered"):
                self.assertTrue((run / name).is_file(), name)

    def test_source_mutation_and_unknown_targets_fail(self):
        result = self.run_fuzz(MUTATE_FUZZ_SOURCE="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Source changed during fuzz qualification", result.stderr)
        result = subprocess.run(["bash", "scripts/run-release-fuzz.sh", "--target", "unknown"],
                                cwd=self.root, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 2)


if __name__ == "__main__":
    unittest.main()
