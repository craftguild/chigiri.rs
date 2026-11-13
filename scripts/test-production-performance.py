#!/usr/bin/env python3
"""Regression-test performance harness failures and evidence attribution."""

from __future__ import annotations

import importlib.util
import threading
import json
import os
import subprocess
import sys
import tempfile
import time
import unittest
from datetime import datetime, timezone
from pathlib import Path
from unittest import mock

SCRIPT_ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_ROOT))

import production_workload_metrics as metrics
import performance_evidence
import performance_evidence_metadata
import performance_evidence_regression
import performance_test_fixtures

SPEC = importlib.util.spec_from_file_location(
    "production_http_workload", SCRIPT_ROOT / "production-http-workload.py"
)
workload = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(workload)
ACCEPT_SPEC = importlib.util.spec_from_file_location(
    "accept_benchmark", SCRIPT_ROOT / "accept-benchmark-baseline.py"
)
accept_benchmark = importlib.util.module_from_spec(ACCEPT_SPEC)
assert ACCEPT_SPEC.loader is not None
ACCEPT_SPEC.loader.exec_module(accept_benchmark)


class WorkloadContractTests(unittest.TestCase):
    def test_payloads_are_standard_json_for_query_and_mutation(self) -> None:
        query = json.loads(workload.QUERY)
        mutation = json.loads(workload.MUTATION)
        self.assertEqual(query["query"], workload.QUERY_DOCUMENT)
        self.assertEqual(mutation["query"], workload.MUTATION_DOCUMENT)
        self.assertEqual(mutation["variables"]["input"]["title"], "Soak")
        self.assertNotIn("addBook", mutation["query"])

    def test_graphql_error_and_data_contract_violations_fail(self) -> None:
        error, codes = workload.response_result(
            200,
            b'{"data":null,"errors":[{"message":"capacity",'
            b'"extensions":{"code":"CATALOG_CAPACITY_REACHED"}}]}',
            True,
            True,
        )
        self.assertEqual(error[0], "graphql")
        self.assertEqual(codes, ["CATALOG_CAPACITY_REACHED"])
        error, _ = workload.response_result(200, b'{"data":{"books":null}}', False, False)
        self.assertEqual(error[0], "unexpected-data")

    def test_rejects_invalid_top_level_and_error_json_shapes(self) -> None:
        bodies = [
            b"[]", b"null", b'"text"', b"123", b"true",
            b'{"errors":"invalid"}', b'{"errors":[]}', b'{"errors":[null]}',
            b'{"errors":[{"message":"failure","extensions":[]}]}',
            b'{"errors":[{"message":"failure","extensions":{"code":1}}]}',
            b'{"errors":[{"message":"failure","extensions":null}]}',
            b'{"errors":[{}]}', b'{"errors":[{"message":1}]}',
            b'{"data":null,"data":{"books":[]}}',
        ]
        for body in bodies:
            with self.subTest(body=body):
                error, codes = workload.response_result(200, body, False, False)
                self.assertEqual(error[0], "malformed-response")
                self.assertEqual(codes, [])

    def test_accepts_expected_query_and_mutation_responses(self) -> None:
        query = json.dumps({"data": {"books": [workload.INITIAL_BOOK]}}).encode()
        soaked_query = json.dumps({"data": {"books": [workload.SOAK_BOOK]}}).encode()
        mutation = (
            b'{"data":{"replaceBook":{"id":"1","title":"Soak",'
            b'"author":"Chigiri"}}}'
        )
        self.assertEqual(workload.response_result(200, query, False, False), (None, []))
        self.assertEqual(workload.response_result(200, query, False, True), (None, []))
        self.assertEqual(workload.response_result(200, soaked_query, False, True), (None, []))
        self.assertEqual(workload.response_result(200, mutation, True, True), (None, []))

    def test_rejects_empty_incomplete_and_unknown_book_lists(self) -> None:
        invalid_books = [
            [],
            [{"id": "1", "title": "missing author"}],
            [{"id": "1", "title": "Unknown", "author": "Unknown"}],
        ]
        for books in invalid_books:
            body = json.dumps({"data": {"books": books}}).encode()
            error, _ = workload.response_result(200, body, False, True)
            self.assertEqual(error[0], "unexpected-data")

    def test_latency_and_error_storage_remains_bounded_as_requests_grow(self) -> None:
        results = workload.Results()
        for _ in range(10_000):
            results.record(0.001, False, ("transport", "failure"), [])
        self.assertEqual(results.latencies.total, 10_000)
        self.assertEqual(len(results.latencies.counts), len(workload.LATENCY_BOUNDS_MS) + 1)
        self.assertEqual(len(results.first_errors), 100)
        self.assertEqual(results.error_total(), 10_000)

    def test_overflow_percentile_does_not_invent_an_upper_bound(self) -> None:
        histogram = workload.Results().latencies
        histogram.record(6.0)
        self.assertEqual(histogram.percentile_ms(0.95), (None, 5_000.0))

class RssSamplingTests(unittest.TestCase):
    def test_serializes_reads_and_records_real_read_intervals(self) -> None:
        def reader(_: int) -> int:
            time.sleep(0.02)
            return 4096

        monitor = metrics.RssMonitor(123, reader)
        threads = [threading.Thread(target=monitor.observe) for _ in range(2)]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join()
        evidence = monitor.finish()
        self.assertTrue(evidence["passed"])
        first, second = evidence["samples"]
        self.assertLessEqual(first["readEndSeconds"], second["readStartSeconds"])
        self.assertGreater(first["readEndSeconds"], first["readStartSeconds"])
        self.assertEqual(first["rssBytes"], 4096)
        self.assertTrue(evidence["threadStopped"])

    def test_missing_invalid_or_failed_reads_cannot_pass(self) -> None:
        def failed(_: int) -> int:
            raise OSError("synthetic ps failure")

        for reader in (failed, lambda _: 0, lambda _: True):
            monitor = metrics.RssMonitor(123, reader)
            monitor.observe()
            evidence = monitor.finish()
            self.assertFalse(evidence["passed"])
            self.assertEqual(evidence["errorCount"], 1)
            self.assertEqual(evidence["samples"], [])
        empty = metrics.RssMonitor(123, lambda _: 4096).finish()
        self.assertFalse(empty["passed"])

    def test_ps_reader_converts_kib_to_bytes_and_reports_failure(self) -> None:
        result = subprocess.CompletedProcess([], 0, " 12345\n", "")
        with mock.patch.object(metrics.subprocess, "run", return_value=result):
            self.assertEqual(metrics.read_rss(123), 12345 * 1024)
        for result in (subprocess.CompletedProcess([], 1, "", "no process"),
                       subprocess.CompletedProcess([], 0, "", "")):
            with mock.patch.object(metrics.subprocess, "run", return_value=result):
                with self.assertRaises(OSError):
                    metrics.read_rss(123)

    def test_short_workloads_take_one_read_inside_worker_after_a_request(self) -> None:
        for mode in ("load", "soak"):
            with self.subTest(mode=mode):
                completed = []
                observed = []

                def request(*_):
                    time.sleep(0.001)
                    completed.append(True)

                def observe():
                    self.assertIsNot(threading.current_thread(), threading.main_thread())
                    self.assertGreater(len(completed), 0)
                    observed.append(time.monotonic())

                started = time.monotonic()
                with mock.patch.object(workload, "one_request", side_effect=request):
                    if mode == "load":
                        workload.run_load(1, 8, 2, metrics.Results(), observe)
                    else:
                        workload.run_soak(1, 0.02, 2, metrics.Results(), observe)
                ended = time.monotonic()
                self.assertEqual(len(observed), 1)
                self.assertLessEqual(started, observed[0])
                self.assertLessEqual(observed[0], ended)


class EvidenceIdentityTests(unittest.TestCase):
    def setUp(self) -> None:
        self.identity = performance_test_fixtures.identity()

    def test_rejects_revision_and_binary_digest_mismatches(self) -> None:
        with mock.patch.object(
            performance_evidence_metadata, "source_identity", return_value=self.identity
        ):
            performance_evidence.require_identity(
                {"source": self.identity.copy()}, Path("binary"), "1.95.0"
            )
            for field, value in (("revision", "c" * 40), ("binarySha256", "d" * 64)):
                modified = self.identity.copy()
                modified[field] = value
                with self.assertRaises(performance_evidence.EvidenceError):
                    performance_evidence.require_identity(
                        {"source": modified}, Path("binary"), "1.95.0"
                    )
            uppercase = self.identity.copy()
            uppercase["revision"] = uppercase["revision"].upper()
            with self.assertRaises(performance_evidence.EvidenceError):
                performance_evidence_metadata.require_source_identity(uppercase)

    def test_json_rejects_duplicate_keys_and_nonstandard_numbers(self) -> None:
        for document in ('{"source":{},"source":{}}', '{"value":NaN}'):
            with self.subTest(document=document):
                with self.assertRaises(performance_evidence.EvidenceError):
                    performance_evidence_metadata.strict_json_loads(document)

    def test_optional_command_tolerates_an_unavailable_program(self) -> None:
        with mock.patch.object(
            performance_evidence_metadata.subprocess,
            "run",
            side_effect=FileNotFoundError("missing"),
        ):
            self.assertIsNone(
                performance_evidence_metadata.optional_command_output(["missing"])
            )

    def test_lscpu_descriptor_excludes_the_dynamic_field(self) -> None:
        first = """Vendor ID: GenuineIntel
Model name: Example CPU
CPU family: 6
Model: 1
Stepping: 2
CPU(s) scaling MHz: 10%
"""
        second = first.replace("10%", "90%")
        expected = "Vendor ID=GenuineIntel;Model name=Example CPU;CPU family=6;Model=1;Stepping=2"
        self.assertEqual(
            performance_evidence_metadata.stable_lscpu_descriptor(first), expected
        )
        self.assertEqual(
            performance_evidence_metadata.stable_lscpu_descriptor(second), expected
        )
        with self.assertRaises(performance_evidence.EvidenceError):
            performance_evidence_metadata.stable_lscpu_descriptor("Architecture: x86_64")

    def test_rejects_dirty_source_and_toolchain_mismatches(self) -> None:
        with mock.patch.object(
            performance_evidence_metadata, "source_identity", return_value=self.identity
        ):
            dirty = self.identity.copy()
            dirty["dirty"] = True
            with self.assertRaises(performance_evidence.EvidenceError):
                performance_evidence.require_identity(
                    {"source": dirty}, Path("binary"), "1.95.0"
                )
            wrong_toolchain = self.identity.copy()
            wrong_toolchain["toolchain"] = {
                "version": "1.95.0",
                "cargo": "cargo changed",
                "rustc": "rustc changed",
            }
            with self.assertRaises(performance_evidence.EvidenceError):
                performance_evidence.require_identity(
                    {"source": wrong_toolchain}, Path("binary"), "1.95.0"
                )

    def test_rejects_invalid_initial_baseline_approval_metadata(self) -> None:
        baseline = performance_test_fixtures.baseline()
        with self.assertRaises(ValueError):
            accept_benchmark.accept(
                baseline.copy(),
                "release-owner",
                "Measurements and the uncontended environment were verified before approving the initial baseline.",
                "2026-08-28T00:00:00+00:00",
            )
        with self.assertRaises(ValueError):
            accept_benchmark.accept(
                baseline.copy(),
                "reviewer-123",
                accept_benchmark.OLD_TEMPLATE_REASON,
                "2026-08-28T00:00:00+00:00",
            )

    def test_acceptance_records_auditable_date_and_reason(self) -> None:
        baseline = performance_test_fixtures.baseline()
        accepted_at = datetime.fromtimestamp(
            baseline["recordedAtUnixSeconds"], timezone.utc
        ).isoformat()
        accepted = accept_benchmark.accept(
            baseline,
            "reviewer-123",
            "Binary digest, CPU, process isolation, and all medians were verified before initial baseline approval.",
            accepted_at,
        )
        self.assertEqual(accepted["acceptedBy"], "reviewer-123")
        self.assertEqual(accepted["acceptedAt"], accepted_at)
        self.assertEqual(
            accepted["acceptedAtUnixSeconds"], baseline["recordedAtUnixSeconds"]
        )

    def test_approval_date_is_normalized_to_canonical_utc_seconds(self) -> None:
        baseline = performance_test_fixtures.baseline()
        accepted_at = datetime.fromtimestamp(
            baseline["recordedAtUnixSeconds"], timezone.utc
        ).replace(microsecond=123_456).isoformat()
        accepted = accept_benchmark.accept(
            baseline,
            "reviewer-123",
            "Binary digest, CPU, process isolation, and all medians were verified before initial baseline approval.",
            accepted_at,
        )
        self.assertNotIn(".", accepted["acceptedAt"])
        performance_evidence.require_approval(accepted)
        accepted["acceptedAt"] = accepted["acceptedAt"].replace("+00:00", "Z")
        with self.assertRaises(performance_evidence.EvidenceError):
            performance_evidence.require_approval(accepted)

    def test_rejects_generic_approver_and_missing_profile(self) -> None:
        accepted = performance_test_fixtures.baseline()
        accepted.update(
            {
                "accepted": True,
                "acceptedBy": "release-owner",
                "acceptedAt": "2026-08-28T00:00:00+00:00",
                "acceptedReason": "Binary digest, CPU, process isolation, and all medians were verified before initial baseline approval.",
            }
        )
        with self.assertRaises(performance_evidence.EvidenceError):
            performance_evidence.require_approval(accepted)
        accepted["acceptedBy"] = "reviewer-123"
        accepted["profiles"] = accepted["profiles"][:-1]
        with self.assertRaises(performance_evidence.EvidenceError):
            performance_evidence.require_baseline_contract(accepted)


class ToolchainRunnerTests(unittest.TestCase):
    @staticmethod
    def clean_environment() -> dict[str, str]:
        environment = os.environ.copy()
        exact = {
            "RUSTC", "RUSTDOC", "CARGO", "RUSTUP_TOOLCHAIN", "RUSTFLAGS",
            "RUSTDOCFLAGS", "CARGO_HOME", "CARGO_CONFIG", "CARGO_INCREMENTAL",
            "CARGO_ENCODED_RUSTFLAGS", "RUSTC_BOOTSTRAP", "RUSTC_WRAPPER",
            "RUSTC_WORKSPACE_WRAPPER", "CC", "CXX", "CPP", "AR", "CFLAGS",
            "CXXFLAGS", "CPPFLAGS", "LDFLAGS", "SDKROOT",
            "MACOSX_DEPLOYMENT_TARGET",
        }
        prefixes = ("CARGO_BUILD_", "CARGO_PROFILE_", "CARGO_TARGET_")
        for name in tuple(environment):
            if name in exact or name.startswith(prefixes):
                environment.pop(name)
        return environment

    def test_rejects_ambient_compiler_overrides_before_build(self) -> None:
        overrides = (
            "RUSTC", "CARGO_BUILD_RUSTC", "RUSTC_WRAPPER", "RUSTUP_TOOLCHAIN",
            "RUSTC_WORKSPACE_WRAPPER", "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS",
            "RUSTC_BOOTSTRAP", "CARGO_PROFILE_RELEASE_LTO",
            "CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS",
            "CARGO_INCREMENTAL", "CARGO_HOME", "CARGO_TARGET_DIR",
        )
        for name in overrides:
            with self.subTest(name=name):
                environment = self.clean_environment()
                environment[name] = "/tmp/forbidden-performance-override"
                result = subprocess.run(
                    [
                        str(SCRIPT_ROOT / "run-release-benchmarks.sh"),
                        "record",
                        "1.95.0",
                    ],
                    cwd=SCRIPT_ROOT.parent,
                    env=environment,
                    capture_output=True,
                    text=True,
                    check=False,
                )
                self.assertEqual(result.returncode, 2)
                self.assertIn(name, result.stderr)

    def test_rejects_global_and_parent_cargo_config(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            repository = root / "parent" / "repository"
            repository.mkdir(parents=True)
            home = root / "home"
            home.mkdir()
            command = (
                f'source "{SCRIPT_ROOT / "performance-toolchain.sh"}"; '
                'require_performance_toolchain 1.95.0 "$1"'
            )
            for config in (
                home / ".cargo" / "config.toml",
                root / "parent" / ".cargo" / "config.toml",
            ):
                with self.subTest(config=config):
                    config.parent.mkdir(parents=True, exist_ok=True)
                    config.write_text("[build]\nrustflags = ['-Copt-level=0']\n")
                    environment = self.clean_environment()
                    environment["HOME"] = str(home)
                    result = subprocess.run(
                        ["bash", "-c", command, "--", str(repository)],
                        cwd=SCRIPT_ROOT.parent,
                        env=environment,
                        capture_output=True,
                        text=True,
                        check=False,
                    )
                    self.assertEqual(result.returncode, 2)
                    self.assertIn("Cargo config", result.stderr)
                    config.unlink()

    def test_rejects_previous_binary_and_new_output_on_the_same_inode(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            previous = root / "previous-binary"
            previous.write_bytes(b"immutable previous release")
            linked_output = root / "next-binary"
            os.link(previous, linked_output)
            command = (
                f'source "{SCRIPT_ROOT / "performance-toolchain.sh"}"; '
                'require_distinct_performance_files "$1" "$2"'
            )
            rejected = subprocess.run(
                ["bash", "-c", command, "--", str(previous), str(linked_output)],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(rejected.returncode, 1)
            self.assertIn("Same file.", rejected.stderr)
            allowed = subprocess.run(
                ["bash", "-c", command, "--", str(previous), str(root / "fresh")],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(allowed.returncode, 0)


def load_tests(loader: unittest.TestLoader, tests: unittest.TestSuite, _: str | None):
    tests.addTests(loader.loadTestsFromModule(performance_evidence_regression))
    return tests


if __name__ == "__main__":
    unittest.main()
