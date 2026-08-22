#!/usr/bin/env python3
"""Contract tests for the Rust development-feedback benchmark artifacts."""

from __future__ import annotations

import copy
import importlib.util
import json
from pathlib import Path
import unittest


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
            "warm" if scenario_class.startswith("warm_") else "cold"
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
                "retry": {"is_retry": False, "retry_of_attempt": not_applicable("first attempt")},
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
                "p50_wall_time_ms": 1250,
                "p95_wall_time_ms": 1250,
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
            ("validity reason", lambda receipt: receipt["samples"][0].pop("validity_reason")),
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
            lambda receipt: receipt["samples"][0]["retry"].update(retry_of_attempt=None),
            lambda receipt: receipt["fallback"].update(reason=None),
        )
        for mutate in mutations:
            with self.subTest():
                receipt = copy.deepcopy(valid_receipt())
                mutate(receipt)
                self.assert_invalid_receipt(receipt)


if __name__ == "__main__":
    unittest.main()
