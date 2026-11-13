#!/usr/bin/env python3
"""Ensure the consumer gate runs five quality commands and stops on failure."""

import json
import os
import signal
import subprocess
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("check-cargo-consumer.sh").resolve()
STEPS = ["fmt", "check", "test", "clippy", "doc"]


class ConsumerGateTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.trace = self.root / "trace.jsonl"
        cargo = self.root / "cargo"
        cargo.write_text(
            "#!/usr/bin/env python3\n"
            "import json, os, signal, sys\n"
            "with open(os.environ['TEST_CARGO_TRACE'], 'a') as trace:\n"
            "    trace.write(json.dumps(sys.argv[1:]) + '\\n')\n"
            "step = sys.argv[2]\n"
            "if step == os.environ.get('TEST_CARGO_CANCEL'):\n"
            "    os.kill(os.getppid(), signal.SIGTERM)\n"
            "sys.exit(1 if step == os.environ.get('TEST_CARGO_FAIL') else 0)\n",
            encoding="utf-8",
        )
        cargo.chmod(0o755)

    def tearDown(self):
        self.temporary.cleanup()

    def run_gate(self, **extra):
        self.trace.unlink(missing_ok=True)
        env = dict(os.environ, PATH=f"{self.root}:{os.environ['PATH']}",
                   TEST_CARGO_TRACE=str(self.trace), **extra)
        return subprocess.run(
            ["/bin/bash", str(SCRIPT), "1.95.0", "consumer with spaces/Cargo.toml",
             "target with spaces", "-p", "selected-consumer", "-p", "selected-sdk"],
            env=env, capture_output=True, timeout=10,
        )

    def records(self):
        return [json.loads(line) for line in self.trace.read_text().splitlines()]

    def test_five_commands_and_package_selections_run_in_order(self):
        self.assertEqual(self.run_gate().returncode, 0)
        records = self.records()
        self.assertEqual([record[1] for record in records], STEPS)
        for record in records:
            self.assertEqual(record[0], "+1.95.0")
            self.assertIn("consumer with spaces/Cargo.toml", record)
            if record[1] != "fmt":
                self.assertIn("--locked", record)
                self.assertIn("target with spaces", record)
                self.assertIn("selected-consumer", record)
                self.assertIn("selected-sdk", record)

    def test_each_quality_command_failure_stops_the_gate_with_nonzero_status(self):
        for index, step in enumerate(STEPS):
            with self.subTest(step=step):
                self.assertEqual(self.run_gate(TEST_CARGO_FAIL=step).returncode, 1)
                self.assertEqual([record[1] for record in self.records()], STEPS[:index + 1])

    def test_cancellation_stops_before_the_next_command(self):
        for index, step in enumerate(STEPS):
            with self.subTest(step=step):
                self.assertEqual(self.run_gate(TEST_CARGO_CANCEL=step).returncode, -signal.SIGTERM)
                self.assertEqual([record[1] for record in self.records()], STEPS[:index + 1])

    def test_rejects_invalid_arguments(self):
        missing = subprocess.run(
            ["/bin/bash", str(SCRIPT), "stable", "Cargo.toml", "target"],
            env={"PATH": "/nonexistent"}, capture_output=True, timeout=10,
        )
        self.assertEqual(missing.returncode, 127)
        invalid = subprocess.run(["/bin/bash", str(SCRIPT)], capture_output=True, timeout=10)
        self.assertEqual(invalid.returncode, 2)


if __name__ == "__main__":
    unittest.main()
