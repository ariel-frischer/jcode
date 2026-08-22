#!/usr/bin/env python3
"""Validate Rust development-feedback benchmark matrices and receipts.

This module intentionally uses only the Python standard library so benchmark
artifacts remain reviewable and runnable in a fresh repository checkout.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections.abc import Mapping, Sequence
from pathlib import Path
from typing import Any, NoReturn


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
REQUIRED_SCENARIO_FIELDS = {
    "id",
    "scenario_class",
    "source_preparation",
    "temperature",
    "command_intent",
    "sample_policy",
    "timeout_seconds",
    "validity_rules",
    "required_metrics",
    "expected_evidence",
}
REQUIRED_VARIANT_FIELDS = {
    "id",
    "eligibility",
    "experiment_boundary",
    "fallback",
    "resource_constraints",
}
REQUIRED_METRICS = {
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
}
REQUIRED_PROVENANCE_FIELDS = {
    "label",
    "source_revision",
    "dirty_fingerprint",
    "lockfile_fingerprint",
    "toolchain",
    "host_conditions",
    "effective_configuration",
    "timestamp",
    "matrix_version",
}
REQUIRED_SAMPLE_METRICS = REQUIRED_METRICS - {"action_counts"}
ACTION_COUNT_FIELDS = {
    "requested",
    "executed",
    "followers",
    "coalesced",
    "reused",
    "cancelled",
    "underlying_actions",
}
SEMVER_RE = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?$")
SENSITIVE_KEY_TOKENS = {
    "api_key",
    "apikey",
    "authorization",
    "cookie",
    "credential",
    "password",
    "private_key",
    "secret",
    "token",
}
RAW_SOURCE_KEYS = {"file_contents", "raw_source", "source_contents"}
REDACTED_VALUES = {"<redacted>", "[redacted]", "redacted"}


def _fail(path: str, message: str) -> NoReturn:
    raise ValueError(f"{path}: {message}")


def _mapping(value: Any, path: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        raise TypeError(f"{path}: expected object, got {type(value).__name__}")
    return value


def _list(value: Any, path: str) -> list[Any]:
    if not isinstance(value, list):
        raise TypeError(f"{path}: expected array, got {type(value).__name__}")
    return value


def _required(value: Mapping[str, Any], fields: set[str], path: str) -> None:
    missing = sorted(fields - value.keys())
    if missing:
        _fail(path, f"missing required field(s): {', '.join(missing)}")


def _nonempty_string(value: Any, path: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise TypeError(f"{path}: expected non-empty string")
    return value


def _nonnegative_number(value: Any, path: str) -> int | float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise TypeError(f"{path}: expected number")
    if value < 0:
        _fail(path, "must be non-negative")
    return value


def _nonnegative_integer(value: Any, path: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise TypeError(f"{path}: expected integer")
    if value < 0:
        _fail(path, "must be non-negative")
    return value


def _validate_not_applicable(value: Any, path: str) -> None:
    item = _mapping(value, path)
    if set(item) != {"status", "reason"}:
        _fail(path, "not-applicable value must contain exactly status and reason")
    if item["status"] != "not_applicable":
        _fail(f"{path}.status", "must equal 'not_applicable'")
    _nonempty_string(item["reason"], f"{path}.reason")


def _is_not_applicable(value: Any) -> bool:
    return isinstance(value, Mapping) and value.get("status") == "not_applicable"


def _is_sensitive_key(key: str) -> bool:
    return key in SENSITIVE_KEY_TOKENS or any(
        key.endswith(f"_{token}") for token in SENSITIVE_KEY_TOKENS
    )


def _is_raw_source_key(key: str) -> bool:
    return key in RAW_SOURCE_KEYS or any(
        key.endswith(f"_{token}") for token in RAW_SOURCE_KEYS
    )


def _validate_safe_artifact(value: Any, path: str = "receipt") -> None:
    if value is None:
        _fail(path, "null is not a valid implicit not-applicable value")
    if isinstance(value, Mapping):
        for raw_key, child in value.items():
            key = str(raw_key).lower()
            child_path = f"{path}.{raw_key}"
            if _is_raw_source_key(key):
                _fail(child_path, "raw source contents must not be recorded")
            if _is_sensitive_key(key):
                if not isinstance(child, str) or child.strip().lower() not in REDACTED_VALUES:
                    _fail(child_path, "sensitive value must be excluded or explicitly redacted")
            _validate_safe_artifact(child, child_path)
    elif isinstance(value, list):
        for index, child in enumerate(value):
            _validate_safe_artifact(child, f"{path}[{index}]")


def validate_matrix(matrix: Mapping[str, Any]) -> None:
    """Raise ``ValueError`` or ``TypeError`` when a matrix violates v1."""
    root = _mapping(matrix, "matrix")
    _required(root, {"schema_version", "scenarios", "experiment_variants"}, "matrix")
    version = _nonempty_string(root["schema_version"], "matrix.schema_version")
    if not SEMVER_RE.fullmatch(version):
        _fail("matrix.schema_version", "expected a semantic version such as 1.0.0")

    scenarios = _list(root["scenarios"], "matrix.scenarios")
    seen_ids: set[str] = set()
    seen_classes: set[str] = set()
    for index, raw_scenario in enumerate(scenarios):
        path = f"matrix.scenarios[{index}]"
        scenario = _mapping(raw_scenario, path)
        _required(scenario, REQUIRED_SCENARIO_FIELDS, path)
        scenario_id = _nonempty_string(scenario["id"], f"{path}.id")
        scenario_class = _nonempty_string(
            scenario["scenario_class"], f"{path}.scenario_class"
        )
        if scenario_id in seen_ids:
            _fail(f"{path}.id", f"duplicate scenario id {scenario_id!r}")
        if scenario_class in seen_classes:
            _fail(f"{path}.scenario_class", f"duplicate class {scenario_class!r}")
        seen_ids.add(scenario_id)
        seen_classes.add(scenario_class)
        _mapping(scenario["source_preparation"], f"{path}.source_preparation")
        _nonempty_string(scenario["temperature"], f"{path}.temperature")
        _nonempty_string(scenario["command_intent"], f"{path}.command_intent")
        sample_policy = _mapping(scenario["sample_policy"], f"{path}.sample_policy")
        _required(sample_policy, {"minimum_valid_samples", "maximum_attempts"}, f"{path}.sample_policy")
        minimum = _nonnegative_integer(sample_policy["minimum_valid_samples"], f"{path}.sample_policy.minimum_valid_samples")
        maximum = _nonnegative_integer(sample_policy["maximum_attempts"], f"{path}.sample_policy.maximum_attempts")
        if minimum < 1 or maximum < minimum:
            _fail(f"{path}.sample_policy", "maximum_attempts must be at least a positive minimum_valid_samples")
        if _nonnegative_number(scenario["timeout_seconds"], f"{path}.timeout_seconds") == 0:
            _fail(f"{path}.timeout_seconds", "must be greater than zero")
        _mapping(scenario["validity_rules"], f"{path}.validity_rules")
        metrics = _list(scenario["required_metrics"], f"{path}.required_metrics")
        if any(not isinstance(metric, str) or not metric for metric in metrics):
            _fail(f"{path}.required_metrics", "must contain non-empty metric names")
        missing_metrics = sorted(REQUIRED_METRICS - set(metrics))
        if missing_metrics:
            _fail(f"{path}.required_metrics", f"missing required metric(s): {', '.join(missing_metrics)}")
        evidence = _list(scenario["expected_evidence"], f"{path}.expected_evidence")
        if not evidence or any(not isinstance(item, str) or not item for item in evidence):
            _fail(f"{path}.expected_evidence", "must contain non-empty evidence names")

    missing_classes = sorted(REQUIRED_SCENARIO_CLASSES - seen_classes)
    if missing_classes:
        _fail("matrix.scenarios", f"missing required scenario class(es): {', '.join(missing_classes)}")

    variants = _list(root["experiment_variants"], "matrix.experiment_variants")
    seen_variants: set[str] = set()
    for index, raw_variant in enumerate(variants):
        path = f"matrix.experiment_variants[{index}]"
        variant = _mapping(raw_variant, path)
        _required(variant, REQUIRED_VARIANT_FIELDS, path)
        variant_id = _nonempty_string(variant["id"], f"{path}.id")
        if variant_id in seen_variants:
            _fail(f"{path}.id", f"duplicate variant id {variant_id!r}")
        seen_variants.add(variant_id)
        for field in ("eligibility", "experiment_boundary", "fallback"):
            value = variant[field]
            if not isinstance(value, (str, Mapping)) or not value:
                _fail(f"{path}.{field}", "must be a non-empty string or object")
        constraints = _mapping(variant["resource_constraints"], f"{path}.resource_constraints")
        if not constraints:
            _fail(f"{path}.resource_constraints", "must not be empty")

    missing_variants = sorted(REQUIRED_VARIANTS - seen_variants)
    if missing_variants:
        _fail("matrix.experiment_variants", f"missing required variant(s): {', '.join(missing_variants)}")


def _validate_action_counts(raw_counts: Any, path: str) -> None:
    counts = _mapping(raw_counts, path)
    _required(counts, ACTION_COUNT_FIELDS, path)
    values = {field: _nonnegative_integer(counts[field], f"{path}.{field}") for field in ACTION_COUNT_FIELDS}
    accounted_requests = values["executed"] + values["followers"] + values["reused"] + values["cancelled"]
    if values["requested"] != accounted_requests:
        _fail(path, "requested must equal executed + followers + reused + cancelled")
    if values["coalesced"] > values["followers"]:
        _fail(path, "coalesced cannot exceed followers")
    if values["underlying_actions"] > values["executed"]:
        _fail(path, "underlying_actions cannot exceed executed")


def validate_receipt(receipt: Mapping[str, Any]) -> None:
    """Raise ``ValueError`` or ``TypeError`` when a receipt violates v1."""
    root = _mapping(receipt, "receipt")
    _validate_safe_artifact(root)
    _required(
        root,
        {"schema_version", "run_identity", "samples", "aggregates", "action_counts", "fallback", "experiment_boundary"},
        "receipt",
    )
    version = _nonempty_string(root["schema_version"], "receipt.schema_version")
    if not SEMVER_RE.fullmatch(version):
        _fail("receipt.schema_version", "expected a semantic version such as 1.0.0")

    identity = _mapping(root["run_identity"], "receipt.run_identity")
    _required(identity, REQUIRED_PROVENANCE_FIELDS, "receipt.run_identity")
    for field in ("label", "source_revision", "dirty_fingerprint", "lockfile_fingerprint", "toolchain", "timestamp", "matrix_version"):
        _nonempty_string(identity[field], f"receipt.run_identity.{field}")
    if identity["matrix_version"] != version:
        _fail("receipt.run_identity.matrix_version", "must match receipt.schema_version")
    if not _mapping(identity["host_conditions"], "receipt.run_identity.host_conditions"):
        _fail("receipt.run_identity.host_conditions", "must not be empty")
    if not _mapping(identity["effective_configuration"], "receipt.run_identity.effective_configuration"):
        _fail("receipt.run_identity.effective_configuration", "must not be empty")

    samples = _list(root["samples"], "receipt.samples")
    if not samples:
        _fail("receipt.samples", "must contain at least one raw sample")
    for index, raw_sample in enumerate(samples):
        path = f"receipt.samples[{index}]"
        sample = _mapping(raw_sample, path)
        _required(sample, {"scenario_id", "attempt", "valid", "validity_reason", "retry", "metrics"}, path)
        _nonempty_string(sample["scenario_id"], f"{path}.scenario_id")
        attempt = _nonnegative_integer(sample["attempt"], f"{path}.attempt")
        if attempt < 1:
            _fail(f"{path}.attempt", "must be at least 1")
        if not isinstance(sample["valid"], bool):
            raise TypeError(f"{path}.valid: expected boolean")
        _nonempty_string(sample["validity_reason"], f"{path}.validity_reason")

        retry = _mapping(sample["retry"], f"{path}.retry")
        _required(retry, {"is_retry", "retry_of_attempt"}, f"{path}.retry")
        if not isinstance(retry["is_retry"], bool):
            raise TypeError(f"{path}.retry.is_retry: expected boolean")
        if retry["is_retry"]:
            previous = _nonnegative_integer(retry["retry_of_attempt"], f"{path}.retry.retry_of_attempt")
            if previous < 1 or previous >= attempt:
                _fail(f"{path}.retry.retry_of_attempt", "must refer to an earlier positive attempt")
        else:
            _validate_not_applicable(retry["retry_of_attempt"], f"{path}.retry.retry_of_attempt")

        metrics = _mapping(sample["metrics"], f"{path}.metrics")
        _required(metrics, REQUIRED_SAMPLE_METRICS, f"{path}.metrics")
        for field in REQUIRED_SAMPLE_METRICS - {"cache", "exit_status"}:
            _nonnegative_number(metrics[field], f"{path}.metrics.{field}")
        if isinstance(metrics["exit_status"], bool) or not isinstance(metrics["exit_status"], int):
            raise TypeError(f"{path}.metrics.exit_status: expected integer")
        cache = metrics["cache"]
        if _is_not_applicable(cache):
            _validate_not_applicable(cache, f"{path}.metrics.cache")
        elif not isinstance(cache, (str, Mapping)) or not cache:
            _fail(f"{path}.metrics.cache", "must be non-empty metadata or explicit not-applicable")

    _mapping(root["aggregates"], "receipt.aggregates")
    _validate_action_counts(root["action_counts"], "receipt.action_counts")

    fallback = _mapping(root["fallback"], "receipt.fallback")
    _required(fallback, {"used", "reason"}, "receipt.fallback")
    if not isinstance(fallback["used"], bool):
        raise TypeError("receipt.fallback.used: expected boolean")
    if fallback["used"]:
        _nonempty_string(fallback["reason"], "receipt.fallback.reason")
    else:
        _validate_not_applicable(fallback["reason"], "receipt.fallback.reason")

    boundary = _mapping(root["experiment_boundary"], "receipt.experiment_boundary")
    _required(boundary, {"variant", "bounded", "default_changed"}, "receipt.experiment_boundary")
    _nonempty_string(boundary["variant"], "receipt.experiment_boundary.variant")
    for field in ("bounded", "default_changed"):
        if not isinstance(boundary[field], bool):
            raise TypeError(f"receipt.experiment_boundary.{field}: expected boolean")


def validate_receipts_compatible(baseline: Mapping[str, Any], candidate: Mapping[str, Any]) -> None:
    """Reject undisclosed environment differences between two valid receipts."""
    validate_receipt(baseline)
    validate_receipt(candidate)
    baseline_identity = _mapping(baseline["run_identity"], "baseline.run_identity")
    candidate_identity = _mapping(candidate["run_identity"], "candidate.run_identity")
    compared_fields = ("source_revision", "dirty_fingerprint", "lockfile_fingerprint", "toolchain", "host_conditions", "matrix_version")
    differences = [field for field in compared_fields if baseline_identity[field] != candidate_identity[field]]
    if not differences:
        return
    disclosure = candidate.get("environment_compatibility")
    if not isinstance(disclosure, Mapping) or disclosure.get("status") != "incompatible":
        _fail("candidate.environment_compatibility", f"undisclosed incompatible field(s): {', '.join(differences)}")
    disclosed = disclosure.get("differences")
    if not isinstance(disclosed, list) or not set(differences).issubset(disclosed):
        _fail("candidate.environment_compatibility.differences", f"must disclose: {', '.join(differences)}")
    _nonempty_string(disclosure.get("reason"), "candidate.environment_compatibility.reason")


def load_json(path: str | Path) -> Any:
    """Load one UTF-8 JSON artifact with an actionable path-prefixed failure."""
    artifact_path = Path(path)
    try:
        with artifact_path.open(encoding="utf-8") as artifact_file:
            return json.load(artifact_file)
    except OSError as error:
        raise ValueError(f"{artifact_path}: unable to read JSON: {error}") from error
    except json.JSONDecodeError as error:
        raise ValueError(f"{artifact_path}:{error.lineno}:{error.colno}: invalid JSON: {error.msg}") from error


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    matrix_parser = subparsers.add_parser("validate-matrix", help="validate a benchmark matrix JSON file")
    matrix_parser.add_argument("path")
    receipt_parser = subparsers.add_parser("validate-receipt", help="validate a benchmark receipt JSON file")
    receipt_parser.add_argument("path")
    compare_parser = subparsers.add_parser("validate-comparison", help="validate two receipts and their environment compatibility")
    compare_parser.add_argument("baseline")
    compare_parser.add_argument("candidate")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        if args.command == "validate-matrix":
            validate_matrix(load_json(args.path))
        elif args.command == "validate-receipt":
            validate_receipt(load_json(args.path))
        else:
            validate_receipts_compatible(load_json(args.baseline), load_json(args.candidate))
    except (TypeError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    print("valid")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
