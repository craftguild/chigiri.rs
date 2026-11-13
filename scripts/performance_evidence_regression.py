"""Reject self-reported success and tampered performance evidence."""

from __future__ import annotations

import copy
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import performance_benchmark_evidence
import performance_evidence
import performance_evidence_metadata
import performance_test_fixtures
import performance_workload_evidence


class WorkloadEvidenceRegressionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.identity = performance_test_fixtures.identity()

    def verify(self, evidence: dict[str, object], kind: str = "load") -> None:
        with mock.patch.object(
            performance_evidence_metadata, "source_identity", return_value=self.identity
        ):
            performance_workload_evidence.require_workload(
                evidence, kind, Path("binary"), "1.95.0"
            )

    def test_rejects_invalid_evidence_even_when_passed_is_true(self) -> None:
        valid = performance_test_fixtures.workload()
        self.verify(valid)
        mutations = (
            ("concurrency", 1),
            ("configuredRequests", 1),
            ("steadyMemoryGrowthBytes", 1024**4),
            ("readiness", {"applicationInFlight": 99, "endpointInFlight": 99}),
            ("serverExit", 137),
        )
        for field, value in mutations:
            with self.subTest(field=field):
                modified = copy.deepcopy(valid)
                modified[field] = value
                with self.assertRaises(performance_evidence.EvidenceError):
                    self.verify(modified)

    def test_rejects_float_and_bool_for_fixed_integer_fields(self) -> None:
        valid = performance_test_fixtures.workload()
        for field, value in (
            ("formatVersion", 4.0),
            ("concurrency", 64.0),
            ("configuredRequests", 10_000.0),
            ("warmupRequests", 256.0),
            ("serverExit", False),
        ):
            with self.subTest(field=field):
                modified = copy.deepcopy(valid)
                modified[field] = value
                with self.assertRaises(performance_evidence.EvidenceError):
                    self.verify(modified)
        modified = copy.deepcopy(valid)
        modified["readiness"]["applicationInFlight"] = False
        with self.assertRaises(performance_evidence.EvidenceError):
            self.verify(modified)

    def test_validates_duration_and_parallelism_independently(self) -> None:
        valid = performance_test_fixtures.workload("soak")
        self.verify(valid, "soak")
        for field, value in (
            ("configuredDurationSeconds", 2),
            ("elapsedSeconds", 899.0),
            ("concurrency", 1),
            ("mutationAttempts", 9),
        ):
            with self.subTest(field=field):
                modified = copy.deepcopy(valid)
                modified[field] = value
                with self.assertRaises(performance_evidence.EvidenceError):
                    self.verify(modified, "soak")

    def test_dirty_short_harness_evidence_cannot_satisfy_formal_load_or_soak(self) -> None:
        valid = performance_test_fixtures.workload("short-soak")
        self.identity["dirty"] = True
        valid["source"] = copy.deepcopy(self.identity)
        self.verify(valid, "short-soak")
        for kind in ("load", "soak"):
            with self.assertRaises(performance_evidence.EvidenceError):
                self.verify(valid, kind)
            formal = performance_test_fixtures.workload(kind)
            formal["source"] = copy.deepcopy(self.identity)
            with self.assertRaises(performance_evidence.EvidenceError):
                self.verify(formal, kind)
        for field, value in (("concurrency", 1), ("configuredDurationSeconds", 1),
                             ("mutationSuccesses", 0), ("errorCount", 1), ("mode", "soak")):
            modified = copy.deepcopy(valid)
            modified[field] = value
            with self.assertRaises(performance_evidence.EvidenceError):
                self.verify(modified, "short-soak")
        self.identity["dirty"] = 1
        valid["source"] = copy.deepcopy(self.identity)
        with self.assertRaises(performance_evidence.EvidenceError):
            self.verify(valid, "short-soak")

    def test_rejects_inconsistent_histogram_and_rss_pass_claims(self) -> None:
        valid = performance_test_fixtures.workload()
        changes = (
            ("latencyHistogram", "bucketCounts", [1] + [0] * 12),
            ("rssSampler", "sampleCount", 1),
            ("rssSampler", "errorCount", 1),
        )
        for parent, field, value in changes:
            with self.subTest(field=field):
                modified = copy.deepcopy(valid)
                modified[parent][field] = value
                modified[parent]["passed"] = True
                with self.assertRaises(performance_evidence.EvidenceError):
                    self.verify(modified)

    def test_recalculates_rss_summary_from_raw_samples(self) -> None:
        valid = performance_test_fixtures.workload()
        for field, value in (
            ("initialSteadyRssBytes", 1),
            ("peakRssBytes", 1),
            ("rssBaselineEndIndex", 6),
        ):
            with self.subTest(field=field):
                modified = copy.deepcopy(valid)
                modified[field] = value
                with self.assertRaises(performance_evidence.EvidenceError):
                    self.verify(modified)

    def test_requires_pre_load_and_during_load_reads_without_a_coverage_gate(self) -> None:
        valid = performance_test_fixtures.workload("soak")
        # Two real observations satisfy the minimum even over a 15-minute soak.
        self.verify(valid, "soak")
        for index, left, right in ((0, 0.9, 1.1), (1, 0.8, 0.9),
                                   (1, 900.9, 901.1), (1, 901.1, 901.2)):
            with self.subTest(index=index, left=left, right=right):
                modified = copy.deepcopy(valid)
                modified["rssSampler"]["elapsedSeconds"] = 902.0
                observation = modified["rssSampler"]["samples"][index]
                observation.update(readStartSeconds=left, readEndSeconds=right)
                with self.assertRaises(performance_evidence.EvidenceError):
                    self.verify(modified, "soak")
        old = copy.deepcopy(valid)
        old["formatVersion"] = 5
        with self.assertRaises(performance_evidence.EvidenceError):
            self.verify(old, "soak")

    def test_baseline_cannot_double_as_during_and_end_point_is_after_load(self) -> None:
        for baseline_at_start in (False, True):
            modified = performance_test_fixtures.workload()
            start, end = modified["workloadStartSeconds"], modified["workloadEndSeconds"]
            samples = modified["rssSampler"]["samples"]
            if baseline_at_start:
                samples[0].update(readStartSeconds=start, readEndSeconds=start)
                samples[1].update(readStartSeconds=end + 0.1, readEndSeconds=end + 0.2)
                modified["rssSampler"]["elapsedSeconds"] = end + 0.3
            else:
                samples[1].update(readStartSeconds=end, readEndSeconds=end)
            with self.subTest(baseline_at_start=baseline_at_start):
                with self.assertRaises(performance_evidence.EvidenceError):
                    self.verify(modified)

    def test_late_high_rss_is_not_counted_during_load_or_discarded_from_peak(self) -> None:
        valid = performance_test_fixtures.workload()
        modified = copy.deepcopy(valid)
        modified["rssSampler"]["samples"].append({
            "readStartSeconds": 2.1, "readEndSeconds": 2.11,
            "rssBytes": 4096 + 65 * 1024 * 1024,
        })
        modified["rssSampler"]["sampleCount"] = 3
        modified["rssSampler"]["elapsedSeconds"] = 2.2
        with self.assertRaises(performance_evidence.EvidenceError):
            self.verify(modified)
        modified["peakRssBytes"] = 4096 + 65 * 1024 * 1024
        modified["steadyMemoryGrowthBytes"] = 65 * 1024 * 1024
        with self.assertRaises(performance_evidence.EvidenceError):
            self.verify(modified)

    def test_rejects_missing_or_unknown_audit_metadata(self) -> None:
        valid = performance_test_fixtures.workload()
        self.verify(valid)
        for field in ("hardware", "recordedAtUnixSeconds", "command"):
            with self.subTest(field=field):
                modified = copy.deepcopy(valid)
                del modified[field]
                with self.assertRaises(performance_evidence.EvidenceError):
                    self.verify(modified)
        modified = copy.deepcopy(valid)
        modified["selfReported"] = True
        with self.assertRaises(performance_evidence.EvidenceError):
            self.verify(modified)

    def test_rejects_invalid_hardware_and_recording_time(self) -> None:
        valid = performance_test_fixtures.workload()
        changes = (
            ("osVersion", ""),
            ("descriptor", ""),
            ("logicalCpus", 0),
            ("logicalCpus", True),
        )
        for field, value in changes:
            with self.subTest(field=field, value=value):
                modified = copy.deepcopy(valid)
                modified["hardware"][field] = value
                with self.assertRaises(performance_evidence.EvidenceError):
                    self.verify(modified)
        modified = copy.deepcopy(valid)
        del modified["hardware"]["osVersion"]
        with self.assertRaises(performance_evidence.EvidenceError):
            self.verify(modified)
        modified = copy.deepcopy(valid)
        modified["hardware"]["unknown"] = "self-reported"
        with self.assertRaises(performance_evidence.EvidenceError):
            self.verify(modified)
        for value in (0, True, 1.5, 4_102_444_800):
            with self.subTest(recorded=value):
                modified = copy.deepcopy(valid)
                modified["recordedAtUnixSeconds"] = value
                with self.assertRaises(performance_evidence.EvidenceError):
                    self.verify(modified)

    def test_validates_recorded_command_mode_and_settings(self) -> None:
        valid = performance_test_fixtures.workload()
        for index, value in (
            (1, "soak"),
            (5, "stable"),
            (7, "1"),
            (9, "1"),
            (11, "1"),
            (13, "0.1"),
            (13, "1"),
        ):
            with self.subTest(index=index):
                modified = copy.deepcopy(valid)
                modified["command"][index] = value
                with self.assertRaises(performance_evidence.EvidenceError):
                    self.verify(modified)
        modified = copy.deepcopy(valid)
        modified["command"].extend(["--unknown", "value"])
        with self.assertRaises(performance_evidence.EvidenceError):
            self.verify(modified)

    def test_revalidates_binary_digest_independently_of_recorded_path(self) -> None:
        valid = performance_test_fixtures.workload()
        valid["command"][3] = "/deleted/worktree/binary"
        self.verify(valid)


class BenchmarkEvidenceRegressionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.baseline = performance_test_fixtures.baseline(accepted=True)
        self.identity = copy.deepcopy(self.baseline["source"])

    def test_captured_real_producer_queues_match_the_verifier(self) -> None:
        path = Path(__file__).with_name("fixtures") / "benchmark-observer-queues-v5.json"
        evidence = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(evidence["formatVersion"], 5)
        self.assertEqual((evidence["samples"], evidence["iterationsPerSample"]), (31, 100))
        for key, names in (
            ("observerQueues", performance_evidence.OBSERVER_QUEUE_PROFILES),
            ("diagnosticObserverQueues", performance_evidence.DIAGNOSTIC_QUEUE_PROFILES),
        ):
            performance_evidence.require_observer_queues(evidence[key], 31, 100, names)

    def test_current_facade_queue_names_accept_and_retired_or_missing_names_fail(self) -> None:
        self.assertEqual(
            [item["name"] for item in self.baseline["observerQueues"]],
            ["observer-enabled", "facade-composition-enabled"],
        )
        performance_evidence.require_baseline_contract(self.baseline)
        for replacement in ("production-preset-enabled", "observer-enabled", "unknown"):
            with self.subTest(replacement=replacement):
                changed = copy.deepcopy(self.baseline)
                changed["observerQueues"][1]["name"] = replacement
                with self.assertRaises(performance_evidence.EvidenceError):
                    performance_evidence.require_baseline_contract(changed)
        changed = copy.deepcopy(self.baseline)
        changed["observerQueues"].pop()
        with self.assertRaises(performance_evidence.EvidenceError):
            performance_evidence.require_baseline_contract(changed)

    def test_rejects_negative_medians_invalid_profiles_and_false_overhead_claims(self) -> None:
        changes = []
        negative = copy.deepcopy(self.baseline)
        negative["measurements"][0]["medianNs"] = -1.0
        changes.append(negative)
        profile = copy.deepcopy(self.baseline)
        profile["profiles"][0]["includes"] = ["self reported fixture"]
        changes.append(profile)
        overhead = copy.deepcopy(self.baseline)
        overhead["overheads"][0]["enabledMedianNs"] = 2.0
        overhead["overheads"][0]["overheadPercent"] = 0.0
        overhead["overheads"][0]["passed"] = True
        changes.append(overhead)
        for evidence in changes:
            with self.assertRaises(performance_evidence.EvidenceError):
                performance_evidence.require_baseline_contract(evidence)

    def test_rejects_changes_to_queue_drop_and_diagnostic_profiles(self) -> None:
        mutations = []
        dropped = copy.deepcopy(self.baseline)
        dropped["observerQueues"][0]["droppedLifecycleEventCount"] = 1
        mutations.append(dropped)
        missing = copy.deepcopy(self.baseline)
        del missing["diagnosticObserverQueues"]
        mutations.append(missing)
        diagnostic = copy.deepcopy(self.baseline)
        diagnostic["diagnosticMeasurements"][0]["medianNs"] = -1.0
        mutations.append(diagnostic)
        profile = copy.deepcopy(self.baseline)
        profile["diagnosticProfiles"][0]["includes"] = ["self reported"]
        mutations.append(profile)
        for evidence in mutations:
            with self.assertRaises(performance_evidence.EvidenceError):
                performance_evidence.require_baseline_contract(evidence)

        candidate = performance_test_fixtures.benchmark(self.baseline)
        candidate["diagnosticObserverQueues"] = copy.deepcopy(
            candidate["diagnosticObserverQueues"]
        )
        candidate["diagnosticObserverQueues"][0]["acceptedLifecycleEventCount"] -= 1
        with mock.patch.object(
            performance_evidence, "source_identity", return_value=self.identity
        ):
            with self.assertRaises(performance_evidence.EvidenceError):
                performance_benchmark_evidence.require_benchmark(
                    candidate, self.baseline, Path("candidate"), "1.95.0",
                    "initial-candidate", None,
                )

    def test_recalculates_candidate_regression_from_raw_medians(self) -> None:
        candidate = performance_test_fixtures.benchmark(self.baseline)
        with mock.patch.object(
            performance_evidence, "source_identity", return_value=self.identity
        ):
            performance_benchmark_evidence.require_benchmark(
                candidate,
                self.baseline,
                Path("candidate"),
                "1.95.0",
                "initial-candidate",
                None,
            )
            candidate["metrics"][0]["candidateMedianNs"] = 2.0
            candidate["metrics"][0]["regressionPercent"] = 0.0
            candidate["metrics"][0]["passed"] = True
            with self.assertRaises(performance_evidence.EvidenceError):
                performance_benchmark_evidence.require_benchmark(
                    candidate,
                    self.baseline,
                    Path("candidate"),
                    "1.95.0",
                    "initial-candidate",
                    None,
                )

    def test_distinguishes_initial_candidate_and_previous_release_identity(self) -> None:
        with mock.patch.object(
            performance_evidence, "source_identity", return_value=self.identity
        ):
            performance_evidence.require_accepted_baseline(
                self.baseline,
                Path("candidate"),
                "1.95.0",
                "initial-candidate",
            )
        with tempfile.NamedTemporaryFile() as previous_binary:
            previous_binary.write(b"previous release benchmark")
            previous_binary.flush()
            previous = copy.deepcopy(self.baseline)
            previous["source"]["binarySha256"] = performance_evidence.binary_sha256(
                Path(previous_binary.name)
            )
            # This verifier unit fixture must also work with a single-root history.
            previous["source"]["revision"] = "a" * 40
            candidate_identity = copy.deepcopy(self.identity)
            candidate_identity["revision"] = "c" * 40
            candidate_identity["revisionUnixSeconds"] += 1
            candidate_identity["binarySha256"] = "d" * 64
            revision_times = {
                previous["source"]["revision"]: previous["source"]["revisionUnixSeconds"],
                candidate_identity["revision"]: candidate_identity["revisionUnixSeconds"],
            }
            with mock.patch.object(
                performance_evidence,
                "source_identity",
                return_value=candidate_identity,
            ), mock.patch.object(
                performance_evidence_metadata,
                "revision_unix_seconds",
                side_effect=revision_times.__getitem__,
            ):
                performance_evidence.require_accepted_baseline(
                    previous,
                    Path("candidate"),
                    "1.95.0",
                    "previous-release",
                    Path(previous_binary.name),
                )
                with self.assertRaises(performance_evidence.EvidenceError):
                    performance_evidence.require_accepted_baseline(
                        previous,
                        Path("candidate"),
                        "1.95.0",
                        "initial-candidate",
                    )

    def test_baseline_audit_metadata_with_closed_contract(self) -> None:
        valid = copy.deepcopy(self.baseline)
        performance_evidence.require_baseline_contract(valid)
        for field in ("hardware", "recordedAtUnixSeconds", "command"):
            with self.subTest(field=field):
                modified = copy.deepcopy(valid)
                del modified[field]
                with self.assertRaises(performance_evidence.EvidenceError):
                    performance_evidence.require_baseline_contract(modified)
        for field, value in (("osVersion", ""), ("logicalCpus", 0)):
            with self.subTest(hardware=field):
                modified = copy.deepcopy(valid)
                modified["hardware"][field] = value
                with self.assertRaises(performance_evidence.EvidenceError):
                    performance_evidence.require_baseline_contract(modified)
        modified = copy.deepcopy(valid)
        del modified["hardware"]["arch"]
        with self.assertRaises(performance_evidence.EvidenceError):
            performance_evidence.require_baseline_contract(modified)
        for field, value in (
            ("formatVersion", 4.0),
            ("samples", 31.0),
            ("iterationsPerSample", 100.0),
        ):
            with self.subTest(integer_field=field):
                modified = copy.deepcopy(valid)
                modified[field] = value
                with self.assertRaises(performance_evidence.EvidenceError):
                    performance_evidence.require_baseline_contract(modified)
        modified = copy.deepcopy(valid)
        modified["unknown"] = True
        with self.assertRaises(performance_evidence.EvidenceError):
            performance_evidence.require_baseline_contract(modified)
        for value in (4096.0, 1.5, False):
            modified = copy.deepcopy(valid)
            modified["rss"]["peakRssBytes"] = value
            with self.assertRaises(performance_evidence.EvidenceError):
                performance_evidence.require_baseline_contract(modified)

    def test_rejects_changes_to_baseline_command_and_time(self) -> None:
        valid = copy.deepcopy(self.baseline)
        for index, value in ((1, "compare"), (3, "stable"), (5, "1"), (7, "1")):
            with self.subTest(index=index):
                modified = copy.deepcopy(valid)
                modified["command"][index] = value
                with self.assertRaises(performance_evidence.EvidenceError):
                    performance_evidence.require_baseline_contract(modified)
        modified = copy.deepcopy(valid)
        modified["command"].extend(["--samples", "31"])
        with self.assertRaises(performance_evidence.EvidenceError):
            performance_evidence.require_baseline_contract(modified)
        modified = copy.deepcopy(valid)
        modified["acceptedAt"] = "2030-01-01T00:00:00+00:00"
        with self.assertRaises(performance_evidence.EvidenceError):
            performance_evidence.require_approval(modified)

    def test_previous_release_reuses_the_baseline_fixed_toolchain(self) -> None:
        performance_evidence.require_baseline_toolchain(self.baseline, "1.95.0")
        for version in ("stable", "1.095.0", "1.96.0"):
            with self.subTest(version=version):
                with self.assertRaises(performance_evidence.EvidenceError):
                    performance_evidence.require_baseline_toolchain(
                        self.baseline, version
                    )

    def test_validates_candidate_audit_metadata_and_baseline_path(self) -> None:
        candidate = performance_test_fixtures.benchmark(self.baseline)
        with mock.patch.object(
            performance_evidence, "source_identity", return_value=self.identity
        ):
            performance_benchmark_evidence.require_benchmark(
                candidate,
                self.baseline,
                Path("candidate"),
                "1.95.0",
                "initial-candidate",
                None,
                Path("/relocated/baseline.json"),
            )
            for field in ("hardware", "recordedAtUnixSeconds", "command"):
                with self.subTest(field=field):
                    modified = copy.deepcopy(candidate)
                    del modified[field]
                    with self.assertRaises(performance_evidence.EvidenceError):
                        performance_benchmark_evidence.require_benchmark(
                            modified,
                            self.baseline,
                            Path("candidate"),
                            "1.95.0",
                            "initial-candidate",
                            None,
                        )
            with self.assertRaises(performance_evidence.EvidenceError):
                performance_benchmark_evidence.require_benchmark(
                    candidate,
                    self.baseline,
                    Path("candidate"),
                    "1.95.0",
                    "initial-candidate",
                    None,
                    Path("different-baseline.json"),
                )
            for field, value in (
                ("formatVersion", 4.0),
                ("samples", 31.0),
                ("iterationsPerSample", 100.0),
            ):
                with self.subTest(integer_field=field):
                    modified = copy.deepcopy(candidate)
                    modified[field] = value
                    with self.assertRaises(performance_evidence.EvidenceError):
                        performance_benchmark_evidence.require_benchmark(
                            modified,
                            self.baseline,
                            Path("candidate"),
                            "1.95.0",
                            "initial-candidate",
                            None,
                        )
            modified = copy.deepcopy(candidate)
            modified["unknown"] = True
            with self.assertRaises(performance_evidence.EvidenceError):
                performance_benchmark_evidence.require_benchmark(
                    modified,
                    self.baseline,
                    Path("candidate"),
                    "1.95.0",
                    "initial-candidate",
                    None,
                )
            for command in (
                [*candidate["command"], "--unknown", "value"],
                [*candidate["command"][:2], "--samples", *candidate["command"][2:]],
            ):
                modified = copy.deepcopy(candidate)
                modified["command"] = command
                with self.assertRaises(performance_evidence.EvidenceError):
                    performance_benchmark_evidence.require_benchmark(
                        modified,
                        self.baseline,
                        Path("candidate"),
                        "1.95.0",
                        "initial-candidate",
                        None,
                    )

    def test_rejects_recording_before_source_revision_or_with_future_skew(self) -> None:
        baseline = copy.deepcopy(self.baseline)
        baseline["recordedAtUnixSeconds"] = baseline["source"]["revisionUnixSeconds"] - 1
        with self.assertRaises(performance_evidence.EvidenceError):
            performance_evidence.require_baseline_contract(baseline)
        future = copy.deepcopy(self.baseline)
        future["recordedAtUnixSeconds"] += 600
        future["acceptedAt"] = "2030-01-01T00:00:00+00:00"
        with self.assertRaises(performance_evidence.EvidenceError):
            performance_evidence.require_baseline_contract(future)

    def test_requires_json_object_roots_for_all_evidence_types(self) -> None:
        with self.assertRaises(performance_evidence.EvidenceError):
            performance_evidence.require_baseline_contract([])
        with self.assertRaises(performance_evidence.EvidenceError):
            performance_workload_evidence.require_workload(
                [], "load", Path("binary"), "1.95.0"
            )
        with self.assertRaises(performance_evidence.EvidenceError):
            performance_benchmark_evidence.require_benchmark(
                [],
                self.baseline,
                Path("candidate"),
                "1.95.0",
                "initial-candidate",
                None,
            )
