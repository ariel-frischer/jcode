#!/usr/bin/env python3
"""Validate Rust development-feedback benchmark matrices and receipts.

This module intentionally uses only the Python standard library so benchmark
artifacts remain reviewable and runnable in a fresh repository checkout.
"""

from __future__ import annotations

import argparse
import json
import math
import re
import subprocess
import sys
import threading
import time
from collections.abc import Mapping, Sequence
from pathlib import Path
from typing import Any, Callable, NoReturn


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
AGGREGATED_METRICS = (
    "wall_time_ms",
    "execution_time_ms",
    "queue_time_ms",
    "gate_time_ms",
    "peak_rss_bytes",
    "swap_bytes",
)
_CARGO_SCENARIO_LOCK = threading.Lock()


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


def _not_applicable(reason: str) -> dict[str, str]:
    return {"status": "not_applicable", "reason": reason}


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
    if values["underlying_actions"] != values["executed"]:
        _fail(path, "underlying_actions must equal executed")


def _percentile(values: Sequence[int | float], percentile: int) -> int | float:
    """Return median p50 or a deterministic nearest-rank percentile."""
    if not values:
        raise ValueError("percentile requires at least one value")
    ordered = sorted(values)
    if percentile == 50:
        midpoint = len(ordered) // 2
        if len(ordered) % 2:
            return ordered[midpoint]
        return (ordered[midpoint - 1] + ordered[midpoint]) / 2
    return ordered[max(1, math.ceil(percentile * len(ordered) / 100)) - 1]


def _read_linux_status(proc_root: Path, pid: int) -> tuple[int, int]:
    rss_bytes = 0
    swap_bytes = 0
    status = (proc_root / str(pid) / "status").read_text(encoding="utf-8")
    for line in status.splitlines():
        if line.startswith("VmRSS:"):
            rss_bytes = int(line.split()[1]) * 1024
        elif line.startswith("VmSwap:"):
            swap_bytes = int(line.split()[1]) * 1024
    return rss_bytes, swap_bytes


def _linux_process_tree(proc_root: Path, root_pid: int) -> set[int]:
    pending = [root_pid]
    discovered: set[int] = set()
    while pending:
        pid = pending.pop()
        if pid in discovered:
            continue
        discovered.add(pid)
        children_path = proc_root / str(pid) / "task" / str(pid) / "children"
        try:
            pending.extend(int(child) for child in children_path.read_text().split())
        except (FileNotFoundError, PermissionError, ProcessLookupError, ValueError):
            continue
    return discovered


def _linux_resource_probe(proc_root: Path, pid: int) -> dict[str, Any]:
    if not proc_root.is_dir():
        reason = f"Linux process metrics unavailable: {proc_root} is not readable"
        return {
            "peak_rss_bytes": _not_applicable(reason),
            "swap_bytes": _not_applicable(reason),
        }

    rss_bytes = 0
    swap_bytes = 0
    observed = False
    for process_pid in _linux_process_tree(proc_root, pid):
        try:
            process_rss, process_swap = _read_linux_status(proc_root, process_pid)
        except (FileNotFoundError, PermissionError, ProcessLookupError, ValueError):
            continue
        observed = True
        rss_bytes += process_rss
        swap_bytes += process_swap
    if observed:
        return {"peak_rss_bytes": rss_bytes, "swap_bytes": swap_bytes}

    reason = f"Linux process metrics unavailable for process tree {pid}"
    return {
        "peak_rss_bytes": _not_applicable(reason),
        "swap_bytes": _not_applicable(reason),
    }


def _resource_probe_for(
    *,
    platform_name: str,
    proc_root: Path,
    resource_probe: Callable[[int], Mapping[str, Any]] | None,
) -> Callable[[int], Mapping[str, Any]]:
    if resource_probe is not None:
        return resource_probe
    if platform_name == "linux":
        return lambda pid: _linux_resource_probe(proc_root, pid)
    reason = f"process RSS and swap metrics unavailable on {platform_name}"
    unsupported = {
        "peak_rss_bytes": _not_applicable(reason),
        "swap_bytes": _not_applicable(reason),
    }
    return lambda _pid: unsupported


def _merge_resource_observation(
    peaks: dict[str, Any], observation: Mapping[str, Any]
) -> None:
    for field in ("peak_rss_bytes", "swap_bytes"):
        value = observation.get(field, _not_applicable(f"{field} unavailable"))
        if isinstance(value, bool):
            raise TypeError(f"resource observation {field}: expected number")
        if isinstance(value, (int, float)):
            if value < 0:
                raise ValueError(f"resource observation {field}: must be non-negative")
            previous = peaks.get(field)
            peaks[field] = (
                max(previous, value) if isinstance(previous, (int, float)) else value
            )
        elif field not in peaks:
            _validate_not_applicable(value, f"resource observation {field}")
            peaks[field] = value


def _validate_declared_command(
    scenario: Mapping[str, Any], command: Sequence[str]
) -> list[str]:
    argv = list(command)
    if not argv or any(
        not isinstance(argument, str) or not argument for argument in argv
    ):
        raise TypeError("command must contain at least one non-empty string")

    declared = scenario.get("commands")
    if declared is None and "command" in scenario:
        declared = [scenario["command"]]
    if declared is not None:
        allowed = [list(item) for item in _list(declared, "scenario.commands")]
        if argv not in allowed:
            _fail("command", "is not declared by the selected matrix scenario")
    return argv


def run_scenario(
    raw_scenario: Mapping[str, Any],
    command: Sequence[str],
    *,
    cwd: Path,
    queue_time_ms: int | float = 0,
    gate_time_ms: int | float = 0,
    cache_metadata: Any = None,
    resource_probe: Callable[[int], Mapping[str, Any]] | None = None,
    interference_probe: Callable[[], str | None] | None = None,
    platform_name: str = sys.platform,
    proc_root: Path = Path("/proc"),
    env: Mapping[str, str] | None = None,
) -> list[dict[str, Any]]:
    """Execute one matrix scenario within its declared timeout and sample bounds."""
    scenario = _mapping(raw_scenario, "scenario")
    _required(scenario, REQUIRED_SCENARIO_FIELDS, "scenario")
    scenario_id = _nonempty_string(scenario["id"], "scenario.id")
    argv = _validate_declared_command(scenario, command)
    timeout_seconds = _nonnegative_number(
        scenario["timeout_seconds"], "scenario.timeout_seconds"
    )
    if timeout_seconds == 0:
        _fail("scenario.timeout_seconds", "must be greater than zero")
    queue_ms = _nonnegative_number(queue_time_ms, "queue_time_ms")
    gate_ms = _nonnegative_number(gate_time_ms, "gate_time_ms")

    policy = _mapping(scenario["sample_policy"], "scenario.sample_policy")
    _required(
        policy, {"minimum_valid_samples", "maximum_attempts"}, "scenario.sample_policy"
    )
    minimum_valid = _nonnegative_integer(
        policy["minimum_valid_samples"], "scenario.sample_policy.minimum_valid_samples"
    )
    maximum_attempts = _nonnegative_integer(
        policy["maximum_attempts"], "scenario.sample_policy.maximum_attempts"
    )
    if minimum_valid < 1 or maximum_attempts < minimum_valid:
        _fail(
            "scenario.sample_policy", "maximum_attempts must cover the positive minimum"
        )
    retry_invalid = bool(
        policy.get("retry_invalid_samples", maximum_attempts > minimum_valid)
    )

    validity_rules = _mapping(scenario["validity_rules"], "scenario.validity_rules")
    expected_exit = validity_rules.get("expected_exit", "zero")
    if expected_exit not in {"zero", "nonzero"}:
        _fail("scenario.validity_rules.expected_exit", "must be 'zero' or 'nonzero'")
    cache = cache_metadata or _not_applicable("cache metadata was not supplied")
    probe = _resource_probe_for(
        platform_name=platform_name,
        proc_root=proc_root,
        resource_probe=resource_probe,
    )

    samples: list[dict[str, Any]] = []
    valid_samples = 0
    with _CARGO_SCENARIO_LOCK:
        for attempt in range(1, maximum_attempts + 1):
            execution_started = time.monotonic()
            peaks: dict[str, Any] = {}
            timed_out = False
            interrupted = False
            spawn_error: OSError | None = None
            process: subprocess.Popen[bytes] | None = None
            exit_status = 127
            try:
                process = subprocess.Popen(
                    argv,
                    cwd=cwd,
                    env=dict(env) if env is not None else None,
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    start_new_session=True,
                )
                while process.poll() is None:
                    _merge_resource_observation(peaks, probe(process.pid))
                    if time.monotonic() - execution_started >= timeout_seconds:
                        timed_out = True
                        process.terminate()
                        try:
                            process.wait(timeout=0.2)
                        except subprocess.TimeoutExpired:
                            process.kill()
                        break
                    time.sleep(0.01)
                _merge_resource_observation(peaks, probe(process.pid))
                exit_status = 124 if timed_out else process.wait()
            except KeyboardInterrupt:
                interrupted = True
                if process is not None and process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=0.2)
                    except subprocess.TimeoutExpired:
                        process.kill()
                exit_status = 130
            except OSError as error:
                spawn_error = error

            execution_time_ms = max(
                0, round((time.monotonic() - execution_started) * 1000)
            )
            interference = (
                interference_probe() if interference_probe is not None else None
            )
            valid = False
            if interrupted:
                validity_reason = "interrupted_before_completion"
            elif timed_out:
                validity_reason = f"timeout_after_{timeout_seconds}_seconds"
            elif spawn_error is not None:
                validity_reason = f"execution_error_{type(spawn_error).__name__}"
            elif interference and validity_rules.get(
                "invalidate_on_interference", True
            ):
                validity_reason = f"invalidated_by_{interference}"
            elif expected_exit == "zero" and exit_status != 0:
                validity_reason = f"unexpected_nonzero_exit_{exit_status}"
            elif expected_exit == "nonzero" and exit_status == 0:
                validity_reason = "unexpected_zero_exit"
            else:
                valid = True
                validity_reason = "completed_without_interference"

            is_retry = attempt > 1
            samples.append(
                {
                    "scenario_id": scenario_id,
                    "attempt": attempt,
                    "valid": valid,
                    "validity_reason": validity_reason,
                    "retry": {
                        "is_retry": is_retry,
                        "retry_of_attempt": attempt - 1
                        if is_retry
                        else _not_applicable("first attempt"),
                    },
                    "metrics": {
                        "wall_time_ms": execution_time_ms + queue_ms + gate_ms,
                        "execution_time_ms": execution_time_ms,
                        "queue_time_ms": queue_ms,
                        "gate_time_ms": gate_ms,
                        "peak_rss_bytes": peaks.get(
                            "peak_rss_bytes", _not_applicable("peak RSS unavailable")
                        ),
                        "swap_bytes": peaks.get(
                            "swap_bytes", _not_applicable("swap unavailable")
                        ),
                        "cache": cache,
                        "exit_status": exit_status,
                        "retry_count": attempt - 1,
                    },
                }
            )
            if valid:
                valid_samples += 1
                if valid_samples >= minimum_valid:
                    break
            elif interrupted or not retry_invalid:
                break
    return samples


def is_complete_run(
    matrix: Mapping[str, Any], samples: Sequence[Mapping[str, Any]]
) -> bool:
    """Return whether every declared scenario has its required valid samples."""
    validate_matrix(matrix)
    valid_counts: dict[str, int] = {}
    for raw_sample in samples:
        sample = _mapping(raw_sample, "sample")
        if sample.get("valid") is True:
            scenario_id = _nonempty_string(
                sample.get("scenario_id"), "sample.scenario_id"
            )
            valid_counts[scenario_id] = valid_counts.get(scenario_id, 0) + 1
    return all(
        valid_counts.get(_nonempty_string(scenario["id"], "scenario.id"), 0)
        >= _nonnegative_integer(
            _mapping(scenario["sample_policy"], "scenario.sample_policy")[
                "minimum_valid_samples"
            ],
            "scenario.sample_policy.minimum_valid_samples",
        )
        for scenario in _list(matrix["scenarios"], "matrix.scenarios")
    )


def aggregate_samples(samples: Sequence[Mapping[str, Any]]) -> dict[str, dict[str, Any]]:
    """Derive reproducible per-scenario aggregates from retained raw samples."""
    grouped: dict[str, list[Mapping[str, Any]]] = {}
    for raw_sample in samples:
        sample = _mapping(raw_sample, "sample")
        scenario_id = _nonempty_string(sample.get("scenario_id"), "sample.scenario_id")
        grouped.setdefault(scenario_id, []).append(sample)

    aggregates: dict[str, dict[str, Any]] = {}
    for scenario_id, scenario_samples in grouped.items():
        valid = [sample for sample in scenario_samples if sample.get("valid") is True]
        invalid = [sample for sample in scenario_samples if sample.get("valid") is False]
        retries = [sample for sample in scenario_samples if isinstance(sample.get("retry"), Mapping) and sample["retry"].get("is_retry") is True]
        aggregate: dict[str, Any] = {
            "valid_sample_count": len(valid),
            "invalid_sample_count": len(invalid),
            "retry_sample_count": len(retries),
        }
        for metric in AGGREGATED_METRICS:
            values = [
                _nonnegative_number(sample["metrics"][metric], f"sample.metrics.{metric}")
                for sample in valid
                if not _is_not_applicable(sample["metrics"][metric])
            ]
            if values:
                aggregate[f"p50_{metric}"] = _percentile(values, 50)
                aggregate[f"p95_{metric}"] = _percentile(values, 95)
        aggregate["exit_statuses"] = [sample["metrics"]["exit_status"] for sample in scenario_samples]
        aggregate["cache_observations"] = [sample["metrics"]["cache"] for sample in scenario_samples]
        aggregates[scenario_id] = aggregate
    return aggregates


def _validate_aggregates(raw_aggregates: Any, samples: Sequence[Mapping[str, Any]]) -> None:
    aggregates = _mapping(raw_aggregates, "receipt.aggregates")
    expected = aggregate_samples(samples)
    if set(aggregates) != set(expected):
        _fail("receipt.aggregates", "must contain exactly the scenarios present in retained samples")
    for scenario_id, raw_aggregate in aggregates.items():
        path = f"receipt.aggregates.{scenario_id}"
        aggregate = _mapping(raw_aggregate, path)
        _required(aggregate, {"valid_sample_count", "p50_wall_time_ms", "p95_wall_time_ms"}, path)
        for field, value in aggregate.items():
            if field not in expected[scenario_id]:
                _fail(f"{path}.{field}", "unknown or non-reproducible aggregate field")
            if value != expected[scenario_id][field]:
                _fail(f"{path}.{field}", f"does not match retained raw samples; expected {expected[scenario_id][field]!r}")


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
            value = metrics[field]
            if field in {"peak_rss_bytes", "swap_bytes"} and _is_not_applicable(value):
                _validate_not_applicable(value, f"{path}.metrics.{field}")
            else:
                _nonnegative_number(value, f"{path}.metrics.{field}")
        if isinstance(metrics["exit_status"], bool) or not isinstance(metrics["exit_status"], int):
            raise TypeError(f"{path}.metrics.exit_status: expected integer")
        cache = metrics["cache"]
        if _is_not_applicable(cache):
            _validate_not_applicable(cache, f"{path}.metrics.cache")
        elif not isinstance(cache, (str, Mapping)) or not cache:
            _fail(f"{path}.metrics.cache", "must be non-empty metadata or explicit not-applicable")

    _validate_aggregates(root["aggregates"], samples)
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
    compared_fields = ("source_revision", "dirty_fingerprint", "lockfile_fingerprint", "toolchain", "host_conditions", "effective_configuration", "matrix_version")
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


def comparison_report(baseline: Mapping[str, Any], candidate: Mapping[str, Any]) -> dict[str, Any]:
    """Build a deterministic comparison with separated timing and resource observations."""
    validate_receipts_compatible(baseline, candidate)
    baseline_aggregates = _mapping(baseline["aggregates"], "baseline.aggregates")
    candidate_aggregates = _mapping(candidate["aggregates"], "candidate.aggregates")
    common_scenarios = sorted(set(baseline_aggregates) & set(candidate_aggregates))
    reasons: list[str] = []
    if set(baseline_aggregates) != set(candidate_aggregates):
        reasons.append("baseline and candidate scenario sets differ")
    disclosure = candidate.get("environment_compatibility")
    if isinstance(disclosure, Mapping) and disclosure.get("status") == "incompatible":
        reasons.append("environment or effective configuration is explicitly incompatible")

    observations: dict[str, Any] = {}
    for scenario_id in common_scenarios:
        baseline_scenario = _mapping(baseline_aggregates[scenario_id], f"baseline.aggregates.{scenario_id}")
        candidate_scenario = _mapping(candidate_aggregates[scenario_id], f"candidate.aggregates.{scenario_id}")
        scenario_observations: dict[str, Any] = {}
        for metric in AGGREGATED_METRICS:
            p50_key = f"p50_{metric}"
            p95_key = f"p95_{metric}"
            if p50_key not in baseline_scenario or p95_key not in baseline_scenario:
                reasons.append(f"baseline {scenario_id} lacks {metric} p50/p95")
                continue
            if p50_key not in candidate_scenario or p95_key not in candidate_scenario:
                reasons.append(f"candidate {scenario_id} lacks {metric} p50/p95")
                continue
            baseline_values = {"p50": baseline_scenario[p50_key], "p95": baseline_scenario[p95_key]}
            candidate_values = {"p50": candidate_scenario[p50_key], "p95": candidate_scenario[p95_key]}
            scenario_observations[metric] = {
                "baseline": baseline_values,
                "candidate": candidate_values,
                "delta": {"p50": candidate_values["p50"] - baseline_values["p50"], "p95": candidate_values["p95"] - baseline_values["p95"]},
            }
        scenario_observations["exit"] = {"baseline": baseline_scenario.get("exit_statuses", []), "candidate": candidate_scenario.get("exit_statuses", [])}
        scenario_observations["retry"] = {"baseline": baseline_scenario.get("retry_sample_count", 0), "candidate": candidate_scenario.get("retry_sample_count", 0)}
        scenario_observations["cache"] = {"baseline": baseline_scenario.get("cache_observations", []), "candidate": candidate_scenario.get("cache_observations", [])}
        observations[scenario_id] = scenario_observations

    for scenario_id, scenario in observations.items():
        wall = scenario.get("wall_time_ms")
        if not wall or wall["delta"]["p50"] >= 0 or wall["delta"]["p95"] >= 0:
            reasons.append(f"candidate {scenario_id} does not improve both wall-time p50 and p95")
        if scenario["retry"]["candidate"] > scenario["retry"]["baseline"]:
            reasons.append(f"candidate {scenario_id} increases retry observations")

    return {
        "schema_version": baseline["schema_version"],
        "baseline_label": baseline["run_identity"]["label"],
        "candidate_label": candidate["run_identity"]["label"],
        "observations": observations,
        "action_counts": {"baseline": dict(baseline["action_counts"]), "candidate": dict(candidate["action_counts"])},
        "adoption": {"complete": not reasons, "adoptable": not reasons, "reasons": reasons},
    }


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
    report_parser = subparsers.add_parser("compare", help="emit a deterministic baseline/candidate comparison report")
    report_parser.add_argument("baseline")
    report_parser.add_argument("candidate")
    report_parser.add_argument("--output")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        if args.command == "validate-matrix":
            validate_matrix(load_json(args.path))
        elif args.command == "validate-receipt":
            validate_receipt(load_json(args.path))
        elif args.command == "validate-comparison":
            validate_receipts_compatible(load_json(args.baseline), load_json(args.candidate))
        else:
            report = comparison_report(load_json(args.baseline), load_json(args.candidate))
            rendered = json.dumps(report, indent=2, sort_keys=True) + "\n"
            if args.output:
                Path(args.output).write_text(rendered, encoding="utf-8")
            else:
                print(rendered, end="")
            if not report["adoption"]["complete"]:
                return 2
    except (TypeError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    if args.command != "compare":
        print("valid")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
