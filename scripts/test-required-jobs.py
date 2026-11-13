#!/usr/bin/env python3
"""Test required-job validation with real subprocess execution."""

import copy
import json
import os
import subprocess
import sys
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("check-required-jobs.py")
VALID = {job: {"result": "success", "outputs": {"validation": "complete", "profile": "ci"}}
         for job in ("linux", "macos")}


class RequiredJobsTests(unittest.TestCase):
    def run_check(self, text, profile="ci"):
        return subprocess.run([sys.executable, str(SCRIPT)],
                              env=dict(os.environ, CHIGIRI_REQUIRED_JOBS=text,
                                       CHIGIRI_VALIDATION_PROFILE=profile),
                              capture_output=True, timeout=10).returncode

    def test_both_jobs_and_completion(self):
        self.assertEqual(self.run_check(json.dumps(VALID)), 0)
        full = copy.deepcopy(VALID)
        for value in full.values():
            value["outputs"]["profile"] = "full"
        self.assertEqual(self.run_check(json.dumps(full), "full"), 0)
        self.assertNotEqual(self.run_check(json.dumps(full)), 0)
        self.assertNotEqual(self.run_check(json.dumps(VALID), "full"), 0)
        self.assertNotEqual(self.run_check(json.dumps(VALID), "unknown"), 0)

    def test_failure_skip_cancel_missing_and_empty_completion(self):
        for job in VALID:
            for state in ("failure", "skipped", "cancelled", "", None, True):
                with self.subTest(job=job, state=state):
                    changed = copy.deepcopy(VALID)
                    changed[job]["result"] = state
                    self.assertNotEqual(self.run_check(json.dumps(changed)), 0)
            mutations = []
            changed = copy.deepcopy(VALID)
            del changed[job]
            mutations.append(changed)
            for output in ({}, {"validation": ""}, {"validation": True},
                           {"validation": "complete"},
                           {"validation": "complete", "profile": "full"}):
                changed = copy.deepcopy(VALID)
                changed[job]["outputs"] = output
                mutations.append(changed)
            for changed in mutations:
                self.assertNotEqual(self.run_check(json.dumps(changed)), 0)

    def test_malformed_duplicate_and_unknown_jobs(self):
        for text in ("", "null", "[]", "{}", "x" * 65_537,
                     json.dumps(dict(VALID, extra=VALID["linux"])),
                     json.dumps(VALID).replace('"linux":', '"linux":{},"linux":', 1)):
            self.assertNotEqual(self.run_check(text), 0)


if __name__ == "__main__":
    unittest.main()
