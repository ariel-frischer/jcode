#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import sys
import unittest
from dataclasses import replace
from pathlib import Path
from unittest import mock

MODULE_PATH = Path(__file__).with_name("runtime_budget.py")
SPEC = importlib.util.spec_from_file_location("runtime_budget", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"unable to load runtime budget module from {MODULE_PATH}")
budget = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = budget
SPEC.loader.exec_module(budget)


def definition(
    *, metric_id: str = "first_visible_ms", recorded_count: int = 10
) -> budget.MetricDefinition:
    return budget.MetricDefinition(
        id=metric_id,
        unit="milliseconds",
        sampling={"warm_up_count": 1, "recorded_count": recorded_count},
        aggregation=["median", "nearest_rank_p95"],
        policy={
            "kind": "noisy_comparison",
            "relative_tolerance": 0.15,
            "absolute_tolerance": 20.0,
        },
    )


def result(
    *,
    classification: budget.Classification | None = None,
    samples: list[float] | None = None,
) -> budget.MetricResult:
    values = (
        samples if samples is not None else [float(value) for value in range(1, 11)]
    )
    return budget.MetricResult(
        samples=values,
        aggregates={
            "median": budget.median(values),
            "nearest_rank_p95": budget.nearest_rank_percentile(values, 95),
        },
        classification=classification or budget.Classification.PASS,
        diagnostics=[],
    )


def environment(*, recorded_count: int = 10) -> budget.EnvironmentProvenance:
    return budget.EnvironmentProvenance(
        platform={
            "os": "linux",
            "architecture": "x86_64",
            "kernel": "test-kernel",
            "cpu": "test-cpu",
            "logical_cpu_count": 8,
        },
        toolchain_profile={
            "rust": "stable-test",
            "cargo_profile": "selfdev",
            "python": "3.11-test",
            "collectors": {"runtime_budget": "1"},
        },
        command_parameters={
            "command": "python3 scripts/bench_runtime_budgets.py collect",
            "metrics": {
                "first_visible_ms": {
                    "warm_up_count": 1,
                    "recorded_count": recorded_count,
                }
            },
        },
    )


def report(
    *,
    classification: budget.Classification | None = None,
    provenance: budget.EnvironmentProvenance | None = None,
    classifications: list[budget.Classification] | None = None,
) -> budget.RuntimeReport:
    selected_classifications = classifications or [
        classification or budget.Classification.PASS
    ]
    definitions = {
        f"metric_{index}": definition(metric_id=f"metric_{index}")
        for index in range(len(selected_classifications))
    }
    metrics = {
        metric_id: result(classification=selected_classifications[index])
        for index, metric_id in enumerate(definitions)
    }
    return budget.RuntimeReport(
        schema_version="1.0.0",
        executable={
            "requested_path": "/tmp/jcode",
            "resolved_path": "/tmp/jcode",
            "version_revision": "0.78.0-test",
            "sha256": "a" * 64,
        },
        daemon={"pid": 123, "resolved_path": "/tmp/jcode", "socket": "/tmp/jcode.sock"},
        environment=provenance or environment(),
        command={"argv": ["collect"], "started_at": "2026-08-21T00:00:00Z"},
        definitions=definitions,
        metrics=metrics,
        baseline=None,
        cleanup={"owned_processes_stopped": True, "private_paths_removed": True},
    )


def baseline(
    *,
    provenance: budget.EnvironmentProvenance | None = None,
    schema_version: str = "1.0.0",
) -> budget.RuntimeBaseline:
    metric_definition = definition()
    return budget.RuntimeBaseline(
        schema_version=schema_version,
        collected_at="2026-08-20T00:00:00Z",
        reason="Establish the reviewed test baseline",
        environment=provenance or environment(),
        definitions={metric_definition.id: metric_definition},
        metrics={metric_definition.id: result()},
    )


CANONICAL_GROUPS = {
    "first_visible": ("first_visible_ms",),
    "input_ready": ("input_ready_ms",),
    "daemon_ready": ("daemon_ready_ms",),
    "idle_cpu_rss": ("idle_cpu_percent", "idle_rss_mib"),
    "session_scaling": ("session_scaling_mib_per_session",),
    "frame_update_work": ("frame_update_work_count",),
    "protocol_tool_round_trip": (
        "protocol_round_trip_ms",
        "tool_round_trip_ms",
    ),
}


def canonical_definitions() -> dict[str, budget.MetricDefinition]:
    return budget.canonical_metric_definitions()


def canonical_report() -> budget.RuntimeReport:
    definitions = canonical_definitions()
    metrics = {}
    for metric_id, metric_definition in definitions.items():
        samples = (
            [0.0] * metric_definition.sampling["recorded_count"]
            if "exact" in metric_definition.aggregation
            else [
                float(value)
                for value in range(
                    1, metric_definition.sampling["recorded_count"] + 1
                )
            ]
        )
        aggregates = {}
        if "median" in metric_definition.aggregation:
            aggregates["median"] = budget.median(samples)
        if "nearest_rank_p95" in metric_definition.aggregation:
            aggregates["nearest_rank_p95"] = budget.nearest_rank_percentile(samples, 95)
        if "exact" in metric_definition.aggregation:
            aggregates["exact"] = samples[0]
        metrics[metric_id] = budget.MetricResult(
            samples=samples,
            aggregates=aggregates,
            classification=budget.Classification.PASS,
            diagnostics=[],
        )
    return replace(report(), definitions=definitions, metrics=metrics)


class AggregationContractTests(unittest.TestCase):
    def test_median_supports_odd_and_even_sample_counts(self) -> None:
        self.assertEqual(budget.median([9.0, 1.0, 5.0]), 5.0)
        self.assertEqual(budget.median([4.0, 1.0, 3.0, 2.0]), 2.5)

    def test_nearest_rank_p95_uses_ceil_rank_without_interpolation(self) -> None:
        samples = [float(value) for value in range(1, 21)]

        self.assertEqual(budget.nearest_rank_percentile(samples, 95), 19.0)
        self.assertEqual(
            budget.nearest_rank_percentile(list(reversed(samples)), 95), 19.0
        )


class SocketOwnershipTests(unittest.TestCase):
    def test_any_matching_kernel_socket_inode_may_be_owned_by_the_daemon(self) -> None:
        with (
            mock.patch.object(budget, "_socket_inodes", return_value={101, 202}),
            mock.patch.object(budget, "_pid_socket_inodes", return_value={202}),
        ):
            budget.verify_socket_owner(Path("/tmp/private-jcode.sock"), 4242)

    def test_noisy_tolerance_is_strictly_above_the_larger_boundary(self) -> None:
        self.assertEqual(
            budget.classify_tolerance(
                candidate=120.0,
                baseline=100.0,
                relative_tolerance=0.15,
                absolute_tolerance=20.0,
            ),
            budget.Classification.PASS,
        )
        self.assertEqual(
            budget.classify_tolerance(
                candidate=120.000_001,
                baseline=100.0,
                relative_tolerance=0.15,
                absolute_tolerance=20.0,
            ),
            budget.Classification.REVIEW_REQUIRED,
        )


class ModelContractTests(unittest.TestCase):
    def test_classification_has_the_five_stable_wire_values(self) -> None:
        self.assertEqual(
            {classification.value for classification in budget.Classification},
            {
                "pass",
                "review_required",
                "deterministic_failure",
                "invalid",
                "unsupported",
            },
        )

    def test_report_round_trip_preserves_versioned_models_and_classifications(
        self,
    ) -> None:
        classifications = list(budget.Classification)
        candidate = report(classifications=classifications)

        restored = budget.RuntimeReport.from_dict(candidate.to_dict())

        self.assertEqual(restored.schema_version, "1.0.0")
        self.assertIsInstance(
            next(iter(restored.definitions.values())), budget.MetricDefinition
        )
        self.assertIsInstance(
            next(iter(restored.metrics.values())), budget.MetricResult
        )
        self.assertIsInstance(restored.environment, budget.EnvironmentProvenance)
        self.assertEqual(
            {metric.classification for metric in restored.metrics.values()},
            set(classifications),
        )

    def test_baseline_round_trip_preserves_schema_and_provenance(self) -> None:
        candidate = baseline()

        restored = budget.RuntimeBaseline.from_dict(candidate.to_dict())

        self.assertEqual(restored.schema_version, "1.0.0")
        self.assertEqual(restored.reason, candidate.reason)
        self.assertEqual(restored.environment, candidate.environment)
        self.assertIsInstance(restored.environment, budget.EnvironmentProvenance)
        self.assertIsInstance(
            next(iter(restored.definitions.values())), budget.MetricDefinition
        )
        self.assertIsInstance(
            next(iter(restored.metrics.values())), budget.MetricResult
        )


class ValidationContractTests(unittest.TestCase):
    def validate_report(self, candidate: budget.RuntimeReport) -> None:
        self.assertTrue(
            hasattr(budget, "validate_report"),
            "runtime_budget.validate_report must enforce the canonical inventory",
        )
        budget.validate_report(candidate)

    def test_complete_report_requires_all_seven_metric_groups(self) -> None:
        self.validate_report(canonical_report())

        for group, metric_ids in CANONICAL_GROUPS.items():
            with self.subTest(group=group):
                candidate = canonical_report()
                for metric_id in metric_ids:
                    candidate.definitions.pop(metric_id)
                    candidate.metrics.pop(metric_id)

                with self.assertRaises(budget.ValidationError):
                    self.validate_report(candidate)

    def test_report_rejects_a_canonical_metric_with_the_wrong_unit(self) -> None:
        candidate = canonical_report()
        metric_id = "idle_rss_mib"
        candidate.definitions[metric_id] = replace(
            candidate.definitions[metric_id], unit="bytes"
        )

        with self.assertRaises(budget.ValidationError):
            self.validate_report(candidate)

    def test_report_rejects_wrong_canonical_sample_parameters(self) -> None:
        candidate = canonical_report()
        metric_id = "protocol_round_trip_ms"
        candidate.definitions[metric_id] = replace(
            candidate.definitions[metric_id],
            sampling={"warm_up_count": 4, "recorded_count": 30},
        )

        with self.assertRaises(budget.ValidationError):
            self.validate_report(candidate)

    def test_report_rejects_missing_raw_samples(self) -> None:
        candidate = canonical_report()
        candidate.metrics["frame_update_work_count"] = budget.MetricResult(
            samples=[],
            aggregates={},
            classification=budget.Classification.PASS,
            diagnostics=[],
        )

        with self.assertRaises(budget.ValidationError):
            self.validate_report(candidate)

    def test_report_rejects_an_absent_review_action(self) -> None:
        candidate = canonical_report()
        metric_id = "first_visible_ms"
        policy = dict(candidate.definitions[metric_id].policy)
        policy.pop("review_action")
        candidate.definitions[metric_id] = replace(
            candidate.definitions[metric_id], policy=policy
        )

        with self.assertRaises(budget.ValidationError):
            self.validate_report(candidate)

    def test_incomplete_samples_are_invalid(self) -> None:
        metric_definition = definition(recorded_count=10)
        incomplete = result(samples=[1.0, 2.0, 3.0])

        with self.assertRaises(budget.ValidationError):
            budget.validate_metric_result(metric_definition, incomplete)

    def test_unsupported_metric_is_explicit_and_does_not_fabricate_samples(
        self,
    ) -> None:
        unsupported = budget.MetricResult(
            samples=[],
            aggregates={},
            classification=budget.Classification.UNSUPPORTED,
            diagnostics=["procfs is unavailable on this platform"],
        )

        budget.validate_metric_result(definition(), unsupported)
        restored = budget.MetricResult.from_dict(unsupported.to_dict())

        self.assertEqual(restored.classification, budget.Classification.UNSUPPORTED)
        self.assertEqual(restored.samples, [])
        self.assertTrue(restored.diagnostics)

    def test_empty_samples_cannot_be_reported_as_pass(self) -> None:
        fabricated_pass = budget.MetricResult(
            samples=[],
            aggregates={},
            classification=budget.Classification.PASS,
            diagnostics=[],
        )

        with self.assertRaises(budget.ValidationError):
            budget.validate_metric_result(definition(), fabricated_pass)

    def test_incompatible_baseline_command_parameters_are_rejected(self) -> None:
        incompatible_environment = environment(recorded_count=9)

        with self.assertRaises(budget.CompatibilityError):
            budget.validate_baseline_compatibility(
                report(),
                baseline(provenance=incompatible_environment),
            )

    def test_incompatible_schema_versions_are_rejected(self) -> None:
        with self.assertRaises(budget.CompatibilityError):
            budget.validate_baseline_compatibility(
                report(),
                baseline(schema_version="2.0.0"),
            )


if __name__ == "__main__":
    unittest.main()
