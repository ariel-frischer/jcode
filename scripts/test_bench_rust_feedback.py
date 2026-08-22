#!/usr/bin/env python3
"""Contract tests for the Rust development-feedback benchmark artifacts."""

from __future__ import annotations

import copy
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest import mock


SCRIPTS_DIR = Path(__file__).resolve().parent
BENCHMARK_MODULE_PATH = SCRIPTS_DIR / "bench_rust_feedback.py"
MATRIX_PATH = SCRIPTS_DIR / "rust_feedback_matrix.json"

REQUIRED_SCENARIO_CLASSES = {
    "warm_touched_file_check_build",
    "cold_fresh_worktree_build",
    "focused_test",
    "invalid_zero_test",
    "broad_validation",
    "coordinated_duplicate",
}
REQUIRED_VARIANTS = {
    "adaptive_jobs",
    "jobs_6",
    "jobs_8",
    "nextest_focused",
    "sccache_cold_miss",
    "sccache_reusable_hit",
}


def load_benchmark_module():
    """Load the implementation while reporting a contract failure, not ImportError."""
    if not BENCHMARK_MODULE_PATH.is_file():
        raise AssertionError(
            f"missing benchmark implementation: {BENCHMARK_MODULE_PATH}"
        )
    spec = importlib.util.spec_from_file_location(
        "bench_rust_feedback", BENCHMARK_MODULE_PATH
    )
    if spec is None or spec.loader is None:
        raise AssertionError(
            f"unable to create an import specification for {BENCHMARK_MODULE_PATH}"
        )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    for name in ("validate_matrix", "validate_receipt"):
        if not callable(getattr(module, name, None)):
            raise AssertionError(
                f"benchmark implementation must define callable {name}()"
            )
    return module


def scenario(scenario_class: str) -> dict[str, object]:
    return {
        "id": scenario_class,
        "scenario_class": scenario_class,
        "source_preparation": {
            "mode": "touched_file" if scenario_class.startswith("warm_") else "fixture",
            "instructions": "prepare a deterministic repository source state",
        },
        "temperature": (
            "warm"
            if scenario_class.startswith("warm_")
            else "cold"
            if scenario_class.startswith("cold_")
            else "controlled"
        ),
        "command_intent": "exercise the declared Rust feedback path",
        "sample_policy": {"minimum_valid_samples": 3, "maximum_attempts": 5},
        "timeout_seconds": 1800,
        "validity_rules": {
            "expected_exit": "nonzero"
            if scenario_class == "invalid_zero_test"
            else "zero",
            "invalidate_on_interference": True,
        },
        "required_metrics": [
            "wall_time_ms",
            "execution_time_ms",
            "queue_time_ms",
            "gate_time_ms",
            "peak_rss_bytes",
            "swap_bytes",
            "cache",
            "exit_status",
            "retry_count",
            "action_counts",
        ],
        "expected_evidence": ["raw_samples", "provenance", "validity"],
    }


def valid_matrix() -> dict[str, object]:
    variants = []
    for variant_id in sorted(REQUIRED_VARIANTS):
        variants.append(
            {
                "id": variant_id,
                "eligibility": "only the compatible declared scenario lanes",
                "experiment_boundary": "isolated and bounded; no default change",
                "fallback": "use the established Cargo path",
                "resource_constraints": {
                    "serialize_cargo": True,
                    "record_rss_swap_and_disk": True,
                },
            }
        )
    return {
        "schema_version": "1.0.0",
        "scenarios": [scenario(name) for name in sorted(REQUIRED_SCENARIO_CLASSES)],
        "experiment_variants": variants,
    }


def not_applicable(reason: str) -> dict[str, str]:
    return {"status": "not_applicable", "reason": reason}


def valid_receipt() -> dict[str, object]:
    return {
        "schema_version": "1.0.0",
        "run_identity": {
            "label": "candidate",
            "source_revision": "0123456789abcdef",
            "dirty_fingerprint": "clean",
            "lockfile_fingerprint": "sha256:lockfile",
            "toolchain": "rustc 1.89.0",
            "host_conditions": {
                "os": "linux",
                "architecture": "x86_64",
                "cpu_count": 16,
                "memory_bytes": 32_000_000_000,
            },
            "effective_configuration": {
                "jobs": "adaptive",
                "cache": "disabled",
            },
            "timestamp": "2026-08-22T00:00:00Z",
            "matrix_version": "1.0.0",
        },
        "samples": [
            {
                "scenario_id": "focused_test",
                "attempt": 1,
                "valid": True,
                "validity_reason": "completed_without_interference",
                "retry": {
                    "is_retry": False,
                    "retry_of_attempt": not_applicable("first attempt"),
                },
                "metrics": {
                    "wall_time_ms": 1250,
                    "execution_time_ms": 1100,
                    "queue_time_ms": 80,
                    "gate_time_ms": 70,
                    "peak_rss_bytes": 500_000_000,
                    "swap_bytes": 0,
                    "cache": not_applicable("cache disabled for this lane"),
                    "exit_status": 0,
                    "retry_count": 0,
                },
            }
        ],
        "aggregates": {
            "focused_test": {
                "valid_sample_count": 1,
                "invalid_sample_count": 0,
                "retry_sample_count": 0,
                "p50_wall_time_ms": 1250,
                "p95_wall_time_ms": 1250,
                "p50_execution_time_ms": 1100,
                "p95_execution_time_ms": 1100,
                "p50_queue_time_ms": 80,
                "p95_queue_time_ms": 80,
                "p50_gate_time_ms": 70,
                "p95_gate_time_ms": 70,
                "p50_peak_rss_bytes": 500_000_000,
                "p95_peak_rss_bytes": 500_000_000,
                "p50_swap_bytes": 0,
                "p95_swap_bytes": 0,
                "exit_statuses": [0],
                "cache_observations": [
                    not_applicable("cache disabled for this lane")
                ],
            }
        },
        "action_counts": {
            "requested": 1,
            "executed": 1,
            "followers": 0,
            "coalesced": 0,
            "reused": 0,
            "cancelled": 0,
            "underlying_actions": 1,
        },
        "fallback": {
            "used": False,
            "reason": not_applicable("primary path completed"),
        },
        "experiment_boundary": {
            "variant": "adaptive_jobs",
            "bounded": True,
            "default_changed": False,
        },
    }


def benchmark_sample(
    wall_time_ms: int,
    *,
    attempt: int,
    valid: bool = True,
    validity_reason: str = "completed_without_interference",
    retry_of_attempt: int | None = None,
) -> dict[str, object]:
    sample = copy.deepcopy(valid_receipt()["samples"][0])
    sample["attempt"] = attempt
    sample["valid"] = valid
    sample["validity_reason"] = validity_reason
    sample["metrics"]["wall_time_ms"] = wall_time_ms
    sample["metrics"]["retry_count"] = 1 if retry_of_attempt is not None else 0
    if retry_of_attempt is None:
        sample["retry"] = {
            "is_retry": False,
            "retry_of_attempt": not_applicable("first attempt"),
        }
    else:
        sample["retry"] = {
            "is_retry": True,
            "retry_of_attempt": retry_of_attempt,
        }
    return sample


def receipt_with_wall_times(wall_times_ms: list[int]) -> dict[str, object]:
    receipt = valid_receipt()
    receipt["samples"] = [
        benchmark_sample(wall_time_ms, attempt=index)
        for index, wall_time_ms in enumerate(wall_times_ms, start=1)
    ]
    sorted_times = sorted(wall_times_ms)
    midpoint = len(sorted_times) // 2
    if len(sorted_times) % 2:
        p50 = sorted_times[midpoint]
    else:
        p50 = (sorted_times[midpoint - 1] + sorted_times[midpoint]) / 2
    p95_index = max(0, (95 * len(sorted_times) + 99) // 100 - 1)
    receipt["aggregates"] = {
        "focused_test": {
            "valid_sample_count": len(sorted_times),
            "invalid_sample_count": 0,
            "retry_sample_count": 0,
            "p50_wall_time_ms": p50,
            "p95_wall_time_ms": sorted_times[p95_index],
            "p50_execution_time_ms": 1100,
            "p95_execution_time_ms": 1100,
            "p50_queue_time_ms": 80,
            "p95_queue_time_ms": 80,
            "p50_gate_time_ms": 70,
            "p95_gate_time_ms": 70,
            "p50_peak_rss_bytes": 500_000_000,
            "p95_peak_rss_bytes": 500_000_000,
            "p50_swap_bytes": 0,
            "p95_swap_bytes": 0,
            "exit_statuses": [0] * len(sorted_times),
            "cache_observations": [not_applicable("cache disabled for this lane")]
            * len(sorted_times),
        }
    }
    return receipt


def runner_scenario(
    scenario_class: str = "focused_test",
    *,
    minimum_valid_samples: int = 1,
    maximum_attempts: int = 1,
    timeout_seconds: float = 2,
) -> dict[str, object]:
    fixture = scenario(scenario_class)
    fixture["sample_policy"] = {
        "minimum_valid_samples": minimum_valid_samples,
        "maximum_attempts": maximum_attempts,
        "retry_invalid_samples": maximum_attempts > minimum_valid_samples,
    }
    fixture["timeout_seconds"] = timeout_seconds
    return fixture


def single_sample_matrix() -> dict[str, object]:
    matrix = valid_matrix()
    for fixture in matrix["scenarios"]:
        fixture["sample_policy"] = {
            "minimum_valid_samples": 1,
            "maximum_attempts": 1,
        }
    return matrix


class MatrixContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.module = load_benchmark_module()

    def assert_invalid_matrix(self, matrix: dict[str, object]) -> None:
        with self.assertRaises((ValueError, TypeError)):
            self.module.validate_matrix(matrix)

    def test_fixture_covers_required_scenario_classes_and_variants(self) -> None:
        matrix = valid_matrix()
        self.assertEqual(
            REQUIRED_SCENARIO_CLASSES,
            {item["scenario_class"] for item in matrix["scenarios"]},
        )
        self.assertEqual(
            REQUIRED_VARIANTS,
            {item["id"] for item in matrix["experiment_variants"]},
        )
        self.module.validate_matrix(matrix)

    def test_checked_in_matrix_satisfies_the_same_contract(self) -> None:
        if not MATRIX_PATH.is_file():
            self.fail(f"missing versioned benchmark matrix: {MATRIX_PATH}")
        with MATRIX_PATH.open(encoding="utf-8") as matrix_file:
            matrix = json.load(matrix_file)
        self.module.validate_matrix(matrix)

    def test_rejects_a_missing_required_scenario_class(self) -> None:
        matrix = valid_matrix()
        matrix["scenarios"] = [
            item
            for item in matrix["scenarios"]
            if item["scenario_class"] != "coordinated_duplicate"
        ]
        self.assert_invalid_matrix(matrix)

    def test_rejects_missing_scenario_or_experiment_contract_fields(self) -> None:
        scenario_fields = (
            "source_preparation",
            "temperature",
            "command_intent",
            "sample_policy",
            "timeout_seconds",
            "validity_rules",
            "required_metrics",
            "expected_evidence",
        )
        for field in scenario_fields:
            with self.subTest(contract="scenario", field=field):
                matrix = valid_matrix()
                matrix["scenarios"][0].pop(field)
                self.assert_invalid_matrix(matrix)

        variant_fields = (
            "eligibility",
            "experiment_boundary",
            "fallback",
            "resource_constraints",
        )
        for field in variant_fields:
            with self.subTest(contract="experiment", field=field):
                matrix = valid_matrix()
                matrix["experiment_variants"][0].pop(field)
                self.assert_invalid_matrix(matrix)


class ReceiptContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.module = load_benchmark_module()

    def assert_invalid_receipt(self, receipt: dict[str, object]) -> None:
        with self.assertRaises((ValueError, TypeError)):
            self.module.validate_receipt(receipt)

    def test_complete_receipt_fixture_is_accepted(self) -> None:
        self.module.validate_receipt(valid_receipt())

    def test_rejects_missing_provenance_fields(self) -> None:
        required = (
            "label",
            "source_revision",
            "dirty_fingerprint",
            "lockfile_fingerprint",
            "toolchain",
            "host_conditions",
            "effective_configuration",
            "timestamp",
            "matrix_version",
        )
        for field in required:
            with self.subTest(field=field):
                receipt = valid_receipt()
                receipt["run_identity"].pop(field)
                self.assert_invalid_receipt(receipt)

    def test_rejects_missing_metric_validity_retry_and_action_counts(self) -> None:
        metric_fields = (
            "wall_time_ms",
            "execution_time_ms",
            "queue_time_ms",
            "gate_time_ms",
            "peak_rss_bytes",
            "swap_bytes",
            "cache",
            "exit_status",
            "retry_count",
        )
        for field in metric_fields:
            with self.subTest(contract="metric", field=field):
                receipt = valid_receipt()
                receipt["samples"][0]["metrics"].pop(field)
                self.assert_invalid_receipt(receipt)

        mutations = (
            ("validity", lambda receipt: receipt["samples"][0].pop("valid")),
            (
                "validity reason",
                lambda receipt: receipt["samples"][0].pop("validity_reason"),
            ),
            ("retry", lambda receipt: receipt["samples"][0].pop("retry")),
            ("action counts", lambda receipt: receipt.pop("action_counts")),
        )
        for label, mutate in mutations:
            with self.subTest(field=label):
                receipt = valid_receipt()
                mutate(receipt)
                self.assert_invalid_receipt(receipt)

    def test_rejects_missing_fallback_or_experiment_boundary(self) -> None:
        for field in ("fallback", "experiment_boundary"):
            with self.subTest(field=field):
                receipt = valid_receipt()
                receipt.pop(field)
                self.assert_invalid_receipt(receipt)

    def test_rejects_implicit_not_applicable_values(self) -> None:
        mutations = (
            lambda receipt: receipt["samples"][0]["metrics"].update(cache=None),
            lambda receipt: receipt["samples"][0]["retry"].update(
                retry_of_attempt=None
            ),
            lambda receipt: receipt["fallback"].update(reason=None),
        )
        for mutate in mutations:
            with self.subTest():
                receipt = copy.deepcopy(valid_receipt())
                mutate(receipt)
                self.assert_invalid_receipt(receipt)

    def test_accepts_consistent_coordinated_action_counts(self) -> None:
        receipt = valid_receipt()
        receipt["action_counts"] = {
            "requested": 6,
            "executed": 2,
            "followers": 2,
            "coalesced": 2,
            "reused": 1,
            "cancelled": 1,
            "underlying_actions": 2,
        }
        self.module.validate_receipt(receipt)

    def test_rejects_inconsistent_action_count_relationships(self) -> None:
        mutations = (
            lambda counts: counts.update(requested=7),
            lambda counts: counts.update(coalesced=3),
            lambda counts: counts.update(underlying_actions=3),
            lambda counts: counts.update(underlying_actions=0),
        )
        for mutate in mutations:
            with self.subTest():
                receipt = valid_receipt()
                receipt["action_counts"] = {
                    "requested": 6,
                    "executed": 2,
                    "followers": 2,
                    "coalesced": 2,
                    "reused": 1,
                    "cancelled": 1,
                    "underlying_actions": 2,
                }
                mutate(receipt["action_counts"])
                self.assert_invalid_receipt(receipt)


class AggregateContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.module = load_benchmark_module()

    def assert_invalid_receipt(self, receipt: dict[str, object]) -> None:
        with self.assertRaises((ValueError, TypeError)):
            self.module.validate_receipt(receipt)

    def test_reproduces_odd_and_even_p50_from_retained_samples(self) -> None:
        fixtures = (
            ([100, 300, 500], 300),
            ([100, 300, 500, 700], 400),
        )
        for wall_times, expected_p50 in fixtures:
            with self.subTest(wall_times=wall_times):
                receipt = receipt_with_wall_times(wall_times)
                self.assertEqual(
                    expected_p50,
                    receipt["aggregates"]["focused_test"]["p50_wall_time_ms"],
                )
                self.module.validate_receipt(receipt)
                receipt["aggregates"]["focused_test"]["p50_wall_time_ms"] += 1
                self.assert_invalid_receipt(receipt)

    def test_uses_nearest_rank_p95(self) -> None:
        receipt = receipt_with_wall_times([10, 20, 30, 40, 50, 60, 70, 80, 90, 100])
        self.assertEqual(
            100,
            receipt["aggregates"]["focused_test"]["p95_wall_time_ms"],
        )
        self.module.validate_receipt(receipt)
        receipt["aggregates"]["focused_test"]["p95_wall_time_ms"] = 95
        self.assert_invalid_receipt(receipt)

    def test_excludes_invalid_samples_and_accounts_for_retries(self) -> None:
        receipt = valid_receipt()
        receipt["samples"] = [
            benchmark_sample(
                9_000,
                attempt=1,
                valid=False,
                validity_reason="invalidated_by_host_interference",
            ),
            benchmark_sample(100, attempt=2, retry_of_attempt=1),
            benchmark_sample(300, attempt=3),
            benchmark_sample(500, attempt=4),
        ]
        receipt["aggregates"] = self.module.aggregate_samples(receipt["samples"])
        self.module.validate_receipt(receipt)
        self.assertEqual(
            "invalidated_by_host_interference",
            receipt["samples"][0]["validity_reason"],
        )

        for field, incorrect_value in (
            ("valid_sample_count", 4),
            ("invalid_sample_count", 0),
            ("retry_sample_count", 0),
            ("p50_wall_time_ms", 500),
            ("p95_wall_time_ms", 9_000),
        ):
            with self.subTest(field=field):
                invalid = copy.deepcopy(receipt)
                invalid["aggregates"]["focused_test"][field] = incorrect_value
                self.assert_invalid_receipt(invalid)

    def test_rejects_incomplete_reproducible_aggregates(self) -> None:
        receipt = receipt_with_wall_times([100, 200, 300])
        del receipt["aggregates"]["focused_test"]["p95_execution_time_ms"]

        self.assert_invalid_receipt(receipt)


class RunnerContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.module = load_benchmark_module()
        for name in ("run_scenario", "is_complete_run"):
            if not callable(getattr(self.module, name, None)):
                self.fail(f"benchmark runner must define callable {name}()")

    def run_fixture(
        self,
        fixture: dict[str, object],
        code: str,
        **kwargs: object,
    ) -> list[dict[str, object]]:
        with tempfile.TemporaryDirectory() as temp_dir:
            command = [sys.executable, "-c", code]
            fixture["commands"] = [command]
            return self.module.run_scenario(
                fixture,
                command,
                cwd=Path(temp_dir),
                **kwargs,
            )

    def test_serializes_success_timings_resources_cache_and_exit_status(self) -> None:
        samples = self.run_fixture(
            runner_scenario(),
            "import time; time.sleep(0.02)",
            queue_time_ms=13,
            gate_time_ms=8,
            cache_metadata={"enabled": True, "hit": False, "backend": "fixture"},
            resource_probe=lambda _pid: {
                "peak_rss_bytes": 123_456,
                "swap_bytes": 4_096,
            },
        )

        self.assertEqual(1, len(samples))
        sample = samples[0]
        self.assertTrue(sample["valid"])
        metrics = sample["metrics"]
        self.assertGreater(metrics["wall_time_ms"], 0)
        self.assertGreater(metrics["execution_time_ms"], 0)
        self.assertGreaterEqual(metrics["wall_time_ms"], metrics["execution_time_ms"])
        self.assertEqual(13, metrics["queue_time_ms"])
        self.assertEqual(8, metrics["gate_time_ms"])
        self.assertEqual(123_456, metrics["peak_rss_bytes"])
        self.assertEqual(4_096, metrics["swap_bytes"])
        self.assertEqual(
            {"enabled": True, "hit": False, "backend": "fixture"},
            metrics["cache"],
        )
        self.assertEqual(0, metrics["exit_status"])
        self.assertEqual(0, metrics["retry_count"])

    def test_accepts_the_declared_nonzero_exit_outcome(self) -> None:
        samples = self.run_fixture(
            runner_scenario("invalid_zero_test"),
            "raise SystemExit(7)",
            cache_metadata=not_applicable("cache disabled for fixture"),
            resource_probe=lambda _pid: {
                "peak_rss_bytes": 10,
                "swap_bytes": 0,
            },
        )

        self.assertEqual(1, len(samples))
        self.assertTrue(samples[0]["valid"])
        self.assertEqual(7, samples[0]["metrics"]["exit_status"])

    def test_timeout_retains_terminal_invalid_evidence(self) -> None:
        samples = self.run_fixture(
            runner_scenario(timeout_seconds=0.05),
            "import time; time.sleep(2)",
            cache_metadata=not_applicable("cache disabled for fixture"),
            resource_probe=lambda _pid: {
                "peak_rss_bytes": 10,
                "swap_bytes": 0,
            },
        )

        self.assertEqual(1, len(samples))
        sample = samples[0]
        self.assertFalse(sample["valid"])
        self.assertIn("timeout", sample["validity_reason"])
        self.assertIsInstance(sample["metrics"]["exit_status"], int)

    def test_scenario_without_a_declared_command_is_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "declare command"):
            self.module.run_scenario(
                runner_scenario(),
                [sys.executable, "-c", "pass"],
                cwd=Path.cwd(),
            )

    @unittest.skipUnless(sys.platform != "win32", "process groups require POSIX")
    def test_timeout_reaps_descendant_process_group(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            child_pid_path = Path(temp_dir) / "child.pid"
            code = (
                "import pathlib, subprocess, time; "
                "child=subprocess.Popen(['sleep', '60']); "
                f"pathlib.Path({str(child_pid_path)!r}).write_text(str(child.pid)); "
                "time.sleep(60)"
            )
            fixture = runner_scenario(timeout_seconds=0.1)
            command = [sys.executable, "-c", code]
            fixture["commands"] = [command]
            self.module.run_scenario(fixture, command, cwd=Path(temp_dir))
            child_pid = int(child_pid_path.read_text())
            for _ in range(50):
                try:
                    os.kill(child_pid, 0)
                except ProcessLookupError:
                    break
                time.sleep(0.01)
            else:
                self.fail(f"timed-out descendant process {child_pid} survived")

    def test_planned_valid_samples_are_not_reported_as_retries(self) -> None:
        samples = self.run_fixture(
            runner_scenario(minimum_valid_samples=3, maximum_attempts=3),
            "pass",
        )
        self.assertEqual([False, False, False], [s["retry"]["is_retry"] for s in samples])
        self.assertEqual([0, 0, 0], [s["metrics"]["retry_count"] for s in samples])

    def test_invalid_attempt_is_retained_and_retried(self) -> None:
        fixture = runner_scenario(minimum_valid_samples=1, maximum_attempts=2)
        with tempfile.TemporaryDirectory() as temp_dir:
            counter = Path(temp_dir) / "attempt-count"
            code = (
                "from pathlib import Path; import sys; "
                f"p=Path({str(counter)!r}); "
                "attempt=int(p.read_text())+1 if p.exists() else 1; "
                "p.write_text(str(attempt)); sys.exit(9 if attempt == 1 else 0)"
            )
            command = [sys.executable, "-c", code]
            fixture["commands"] = [command]
            samples = self.module.run_scenario(
                fixture,
                command,
                cwd=Path(temp_dir),
                cache_metadata=not_applicable("cache disabled for fixture"),
                resource_probe=lambda _pid: {
                    "peak_rss_bytes": 10,
                    "swap_bytes": 0,
                },
            )

        self.assertEqual(2, len(samples))
        self.assertFalse(samples[0]["valid"])
        self.assertTrue(samples[1]["valid"])
        self.assertEqual(9, samples[0]["metrics"]["exit_status"])
        self.assertEqual(0, samples[1]["metrics"]["exit_status"])
        self.assertEqual({"is_retry": True, "retry_of_attempt": 1}, samples[1]["retry"])
        self.assertEqual(1, samples[1]["metrics"]["retry_count"])

    def test_environmental_interference_invalidates_an_otherwise_successful_sample(
        self,
    ) -> None:
        samples = self.run_fixture(
            runner_scenario(),
            "pass",
            cache_metadata=not_applicable("cache disabled for fixture"),
            resource_probe=lambda _pid: {
                "peak_rss_bytes": 10,
                "swap_bytes": 0,
            },
            interference_probe=lambda: "external_cargo_activity",
        )

        self.assertEqual(1, len(samples))
        self.assertFalse(samples[0]["valid"])
        self.assertIn("external_cargo_activity", samples[0]["validity_reason"])
        self.assertEqual(0, samples[0]["metrics"]["exit_status"])

    def test_unsupported_linux_metrics_are_explicitly_not_applicable(self) -> None:
        samples = self.run_fixture(
            runner_scenario(),
            "pass",
            cache_metadata=not_applicable("cache disabled for fixture"),
            platform_name="linux",
            proc_root=Path("/definitely/missing/jcode-proc-fixture"),
        )

        metrics = samples[0]["metrics"]
        for field in ("peak_rss_bytes", "swap_bytes"):
            with self.subTest(field=field):
                self.assertEqual("not_applicable", metrics[field]["status"])
                self.assertIn("unavailable", metrics[field]["reason"])

    def test_missing_or_invalid_scenario_samples_prevent_complete_run(self) -> None:
        matrix = single_sample_matrix()
        samples = []
        for fixture in matrix["scenarios"]:
            sample = benchmark_sample(100, attempt=1)
            sample["scenario_id"] = fixture["id"]
            samples.append(sample)

        self.assertTrue(self.module.is_complete_run(matrix, samples))

        missing = samples[:-1]
        self.assertFalse(self.module.is_complete_run(matrix, missing))

        invalid = copy.deepcopy(samples)
        invalid[0]["valid"] = False
        invalid[0]["validity_reason"] = "invalidated_by_host_interference"
        self.assertFalse(self.module.is_complete_run(matrix, invalid))


class CoordinatedDuplicateContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.module = load_benchmark_module()

    @staticmethod
    def coordinated_outcomes() -> list[dict[str, object]]:
        return [
            {
                "request_id": "producer",
                "outcome": "producer",
                "underlying_action_id": "cargo-action-1",
            },
            {
                "request_id": "follower",
                "outcome": "follower",
                "coalesced": True,
                "underlying_action_id": "cargo-action-1",
            },
            {
                "request_id": "late-reuse",
                "outcome": "reused",
                "underlying_action_id": "cargo-action-1",
            },
            {
                "request_id": "cancelled",
                "outcome": "cancelled",
                "underlying_action_id": not_applicable(
                    "cancelled before coordinator-authorized execution"
                ),
            },
        ]

    def collect_fixture(self, outcomes: list[dict[str, object]]) -> dict[str, object]:
        collect = getattr(self.module, "collect_coordinated_duplicate_receipt", None)
        if not callable(collect):
            self.fail(
                "coordinated duplicate runner must define callable "
                "collect_coordinated_duplicate_receipt()"
            )

        submissions = iter(copy.deepcopy(outcomes))
        submitted_request_ids: list[str] = []

        def submit_request(request: dict[str, object]) -> dict[str, object]:
            submitted_request_ids.append(str(request["request_id"]))
            return next(submissions)

        requests = [{"request_id": outcome["request_id"]} for outcome in outcomes]
        with mock.patch.object(
            self.module,
            "run_scenario",
            side_effect=AssertionError(
                "duplicate fixtures must use the server-owned coordinator, not a "
                "direct Cargo scenario"
            ),
        ):
            receipt = collect(
                runner_scenario("coordinated_duplicate"),
                requests,
                submit_request=submit_request,
                coordinator_identity="selfdev-server-fixture",
            )

        self.assertEqual(
            ["producer", "follower", "late-reuse", "cancelled"],
            submitted_request_ids,
        )
        return receipt

    def test_collects_producer_follower_late_reuse_and_cancellation(self) -> None:
        receipt = self.collect_fixture(self.coordinated_outcomes())

        self.assertEqual("selfdev-server-fixture", receipt["coordinator_identity"])
        self.assertEqual(
            ["producer", "follower", "reused", "cancelled"],
            [outcome["outcome"] for outcome in receipt["request_outcomes"]],
        )
        self.assertEqual(
            {
                "requested": 4,
                "executed": 1,
                "followers": 1,
                "coalesced": 1,
                "reused": 1,
                "cancelled": 1,
                "underlying_actions": 1,
            },
            receipt["action_counts"],
        )

    def test_rejects_more_than_one_coordinator_authorized_underlying_action(
        self,
    ) -> None:
        outcomes = self.coordinated_outcomes()
        outcomes[1] = {
            "request_id": "second-producer",
            "outcome": "producer",
            "underlying_action_id": "cargo-action-2",
        }

        with self.assertRaises((ValueError, TypeError)):
            self.collect_fixture(outcomes)


class ComparisonContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.module = load_benchmark_module()

    def test_rejects_undisclosed_provenance_or_configuration_differences(self) -> None:
        mutations = (
            (
                "source_revision",
                lambda identity: identity.update(source_revision="fedcba9876543210"),
            ),
            (
                "lockfile_fingerprint",
                lambda identity: identity.update(lockfile_fingerprint="sha256:other"),
            ),
            ("toolchain", lambda identity: identity.update(toolchain="rustc 1.90.0")),
            (
                "matrix_version",
                lambda identity: identity.update(matrix_version="2.0.0"),
            ),
            (
                "host_conditions",
                lambda identity: identity["host_conditions"].update(cpu_count=8),
            ),
            (
                "effective_configuration",
                lambda identity: identity["effective_configuration"].update(jobs=8),
            ),
        )
        for field, mutate in mutations:
            with self.subTest(field=field):
                baseline = valid_receipt()
                candidate = copy.deepcopy(baseline)
                mutate(candidate["run_identity"])
                if field == "matrix_version":
                    candidate["schema_version"] = "2.0.0"
                with self.assertRaises((ValueError, TypeError)):
                    self.module.validate_receipts_compatible(baseline, candidate)

    def test_accepts_explicitly_disclosed_incompatibility(self) -> None:
        baseline = valid_receipt()
        candidate = copy.deepcopy(baseline)
        candidate["run_identity"]["effective_configuration"]["jobs"] = 8
        candidate["environment_compatibility"] = {
            "status": "incompatible",
            "differences": ["effective_configuration"],
            "reason": "candidate intentionally measures the explicit eight-job lane",
        }
        self.module.validate_receipts_compatible(baseline, candidate)

    def test_controlled_experiment_configuration_difference_is_comparable(self) -> None:
        baseline = receipt_with_wall_times([120, 130, 140])
        candidate = receipt_with_wall_times([100, 110, 115])
        baseline["run_identity"]["effective_configuration"]["jobs"] = "adaptive"
        candidate["run_identity"]["effective_configuration"]["jobs"] = 6
        for receipt in (baseline, candidate):
            receipt["experiment_boundary"]["controlled_configuration_fields"] = ["jobs"]

        report = self.module.comparison_report(baseline, candidate)

        self.assertTrue(report["adoption"]["complete"], report["adoption"]["reasons"])
        self.assertTrue(report["adoption"]["adoptable"], report["adoption"]["reasons"])


if __name__ == "__main__":
    unittest.main()
