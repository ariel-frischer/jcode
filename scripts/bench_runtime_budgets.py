#!/usr/bin/env python3
"""Canonical CLI shell for Jcode runtime budget reports.

Exit codes are stable workflow categories:
  0 pass
  2 usage
  3 review_required
  4 deterministic_failure
  5 invalid
  6 unsupported
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import socket
import subprocess
import sys
import tempfile
import time
from dataclasses import replace
from datetime import datetime, timezone
from enum import IntEnum
from pathlib import Path
from typing import Any, Callable, Sequence

import runtime_budget as budget


ROUND_TRIP_WARM_UPS = 5
ROUND_TRIP_RECORDED_RUNS = 30
ROUND_TRIP_TIMEOUT_S = 1.0
PRIVATE_DAEMON_TIMEOUT_S = 5.0
PROTOCOL_VERSION = 1


def _load_collectors() -> tuple[Any, Any, Any]:
    try:
        import bench_memory_cli as memory_collector
        import bench_startup as daemon_collector
        import bench_startup_visible_ready as startup_collector
    except ModuleNotFoundError:  # Imported as scripts.bench_runtime_budgets.
        from scripts import bench_memory_cli as memory_collector
        from scripts import bench_startup as daemon_collector
        from scripts import bench_startup_visible_ready as startup_collector
    return memory_collector, daemon_collector, startup_collector


class ExitCode(IntEnum):
    PASS = 0
    USAGE = 2
    REVIEW_REQUIRED = 3
    DETERMINISTIC_FAILURE = 4
    INVALID = 5
    UNSUPPORTED = 6


EXIT_CODE_HELP = """exit codes:
  0 pass
  2 usage
  3 review_required
  4 deterministic_failure
  5 invalid
  6 unsupported
"""


def _path(value: str) -> Path:
    return Path(value).expanduser()


def _non_empty(value: str) -> str:
    if not value.strip():
        raise argparse.ArgumentTypeError("value must not be empty")
    return value


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Collect and evaluate Jcode runtime performance budgets.",
        epilog=EXIT_CODE_HELP,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    collect = subparsers.add_parser(
        "collect",
        help="collect a candidate report using isolated runtimes",
    )
    collect.add_argument(
        "--binary",
        type=_path,
        required=True,
        help="absolute candidate executable to measure",
    )
    collect.add_argument(
        "--output",
        type=_path,
        required=True,
        help="path for the versioned JSON report",
    )

    compare = subparsers.add_parser(
        "compare",
        help="compare an existing report with a reviewed baseline",
    )
    compare.add_argument("--report", type=_path, required=True)
    compare.add_argument("--baseline", type=_path, required=True)

    baseline = subparsers.add_parser(
        "baseline",
        help="explicitly create a proposed baseline from a valid report",
    )
    baseline.add_argument("--report", type=_path, required=True)
    baseline.add_argument("--output", type=_path, required=True)
    baseline.add_argument(
        "--reason",
        type=_non_empty,
        required=True,
        help="non-empty attributable reason for the proposed baseline",
    )
    baseline.add_argument(
        "--overwrite",
        action="store_true",
        help="explicitly permit replacing the output path",
    )
    return parser


def _validate_report(report: budget.RuntimeReport) -> None:
    definition_ids = set(report.definitions)
    metric_ids = set(report.metrics)
    if definition_ids != metric_ids:
        raise budget.ValidationError(
            "report metric definitions and results must have identical ids"
        )
    for metric_id in sorted(metric_ids):
        definition = report.definitions[metric_id]
        if definition.id != metric_id:
            raise budget.ValidationError(
                f"metric definition id does not match its key: {metric_id}"
            )
        budget.validate_metric_result(definition, report.metrics[metric_id])


def serialize_report(report: budget.RuntimeReport) -> str:
    """Validate and serialize the sole model used by JSON and human output."""
    _validate_report(report)
    serialized = json.dumps(report.to_dict(), indent=2, sort_keys=True)
    reparsed = budget.RuntimeReport.from_dict(json.loads(serialized))
    _validate_report(reparsed)
    return serialized


def _overall_classification(report: budget.RuntimeReport) -> budget.Classification:
    classifications = {result.classification for result in report.metrics.values()}
    precedence = (
        budget.Classification.INVALID,
        budget.Classification.DETERMINISTIC_FAILURE,
        budget.Classification.UNSUPPORTED,
        budget.Classification.REVIEW_REQUIRED,
        budget.Classification.PASS,
    )
    for classification in precedence:
        if classification in classifications:
            return classification
    raise budget.ValidationError("report contains no metric results")


def render_human_report(serialized_report: str) -> str:
    """Render a summary only after parsing the validated JSON representation."""
    try:
        raw_report = json.loads(serialized_report)
    except json.JSONDecodeError as error:
        raise budget.ValidationError("report JSON is malformed") from error
    report = budget.RuntimeReport.from_dict(raw_report)
    _validate_report(report)
    lines = [
        f"{metric_id}: {report.metrics[metric_id].classification.value}"
        for metric_id in sorted(report.metrics)
    ]
    lines.append(f"overall: {_overall_classification(report).value}")
    return "\n".join(lines)


def _socket_request(
    *, socket_path: Path, request: dict[str, Any], response_type: str, timeout_s: float
) -> dict[str, Any]:
    request_id = request["id"]
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(timeout_s)
        connection.connect(str(socket_path))
        connection.sendall((json.dumps(request, separators=(",", ":")) + "\n").encode())
        received = b""
        while True:
            chunk = connection.recv(64 * 1024)
            if not chunk:
                raise budget.ValidationError(
                    f"local {response_type} operation ended without a response"
                )
            received += chunk
            while b"\n" in received:
                line, received = received.split(b"\n", 1)
                if not line:
                    continue
                try:
                    response = json.loads(line)
                except json.JSONDecodeError as error:
                    raise budget.ValidationError(
                        f"local {response_type} operation returned malformed JSON"
                    ) from error
                if response.get("id") != request_id:
                    continue
                if response.get("type") == "error":
                    raise budget.ValidationError(
                        str(response.get("message") or "local operation failed")
                    )
                if response.get("type") == response_type:
                    return response


def run_versioned_protocol_operation(
    *, socket_path: Path, protocol_version: int, timeout_s: float
) -> dict[str, float]:
    if protocol_version != PROTOCOL_VERSION:
        raise budget.ValidationError(
            f"unsupported local protocol version: {protocol_version}"
        )
    started = time.perf_counter()
    _socket_request(
        socket_path=socket_path,
        request={"type": "ping", "id": 1},
        response_type="pong",
        timeout_s=timeout_s,
    )
    return {"elapsed_ms": (time.perf_counter() - started) * 1000.0}


def run_deterministic_local_tool_operation(
    *,
    socket_path: Path,
    session_id: str,
    tool_name: str,
    tool_input: dict[str, str],
    timeout_s: float,
) -> dict[str, float]:
    command = f"tool:{tool_name} {json.dumps(tool_input, separators=(',', ':'))}"
    started = time.perf_counter()
    response = _socket_request(
        socket_path=socket_path,
        request={
            "type": "debug_command",
            "id": 1,
            "command": command,
            "session_id": session_id,
        },
        response_type="debug_response",
        timeout_s=timeout_s,
    )
    if response.get("ok") is not True:
        raise budget.ValidationError(
            str(response.get("output") or "deterministic local tool failed")
        )
    return {"elapsed_ms": (time.perf_counter() - started) * 1000.0}


def _debug_socket_path(runtime: budget.PrivateRuntime) -> Path:
    filename = runtime.socket_path.name
    if filename.endswith(".sock"):
        filename = f"{filename[:-5]}-debug.sock"
    else:
        filename = f"{filename}-debug.sock"
    return runtime.socket_path.with_name(filename)


def prepare_debug_tool_session(
    *, runtime: budget.PrivateRuntime, timeout_s: float
) -> str:
    response = _socket_request(
        socket_path=_debug_socket_path(runtime),
        request={
            "type": "debug_command",
            "id": 1,
            "command": f"create_session:{Path.cwd()}",
        },
        response_type="debug_response",
        timeout_s=timeout_s,
    )
    if response.get("ok") is not True:
        raise budget.ValidationError(
            str(response.get("output") or "private debug session creation failed")
        )
    try:
        session = json.loads(str(response.get("output") or ""))
    except json.JSONDecodeError as error:
        raise budget.ValidationError(
            "private debug session returned malformed metadata"
        ) from error
    session_id = session.get("session_id") if isinstance(session, dict) else None
    if not isinstance(session_id, str) or not session_id:
        raise budget.ValidationError("private debug session omitted session_id")
    return session_id


def _failure_kind(error: BaseException) -> str:
    if isinstance(error, (TimeoutError, socket.timeout, subprocess.TimeoutExpired)):
        return "timeout"
    if isinstance(error, subprocess.CalledProcessError):
        return "nonzero_exit"
    return "collection_failed"


def _collect_round_trip_metric(
    operation: Callable[[], object],
) -> dict[str, object]:
    samples: list[float] = []
    failures: list[dict[str, object]] = []
    total_runs = ROUND_TRIP_WARM_UPS + ROUND_TRIP_RECORDED_RUNS
    for run_index in range(total_runs):
        recorded_run = run_index - ROUND_TRIP_WARM_UPS + 1
        try:
            result = operation()
            elapsed_ms = result.get("elapsed_ms") if isinstance(result, dict) else None
            if not isinstance(elapsed_ms, (int, float)):
                raise budget.ValidationError(
                    "operation returned incomplete elapsed time"
                )
        except (
            OSError,
            subprocess.SubprocessError,
            budget.RuntimeBudgetError,
        ) as error:
            failure = {
                "kind": (
                    "incomplete"
                    if isinstance(error, budget.ValidationError)
                    and "incomplete" in str(error)
                    else _failure_kind(error)
                ),
                "diagnostic": str(error),
            }
            if run_index < ROUND_TRIP_WARM_UPS:
                failure["warm_up_run"] = run_index + 1
            else:
                failure["recorded_run"] = recorded_run
            failures.append(failure)
            break
        if run_index >= ROUND_TRIP_WARM_UPS:
            samples.append(float(elapsed_ms))

    aggregates = {}
    if not failures and len(samples) == ROUND_TRIP_RECORDED_RUNS:
        aggregates = {
            "median": budget.median(samples),
            "nearest_rank_p95": budget.nearest_rank_percentile(samples, 95),
        }
    elif not failures:
        failures.append(
            {
                "kind": "incomplete",
                "recorded_run": len(samples) + 1,
                "diagnostic": "collector returned partial samples",
            }
        )
    return {
        "status": "valid" if not failures else "invalid",
        "sampling": {
            "warm_up_count": ROUND_TRIP_WARM_UPS,
            "recorded_count": ROUND_TRIP_RECORDED_RUNS,
        },
        "samples": samples,
        "aggregates": aggregates,
        "failures": failures,
    }


def collect_local_round_trips(
    *, runtime: budget.PrivateRuntime, timeout_s: float = ROUND_TRIP_TIMEOUT_S
) -> dict[str, object]:
    tool_session_id = prepare_debug_tool_session(runtime=runtime, timeout_s=timeout_s)
    return {
        "protocol_round_trip_ms": _collect_round_trip_metric(
            lambda: run_versioned_protocol_operation(
                socket_path=runtime.socket_path,
                protocol_version=PROTOCOL_VERSION,
                timeout_s=timeout_s,
            )
        ),
        "tool_round_trip_ms": _collect_round_trip_metric(
            lambda: run_deterministic_local_tool_operation(
                socket_path=_debug_socket_path(runtime),
                session_id=tool_session_id,
                tool_name="bash",
                tool_input={"command": "printf jcode-runtime-budget"},
                timeout_s=timeout_s,
            )
        ),
    }


def write_report_atomic(*, output: Path, serialized: str) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary_path: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            dir=output.parent,
            prefix=f".{output.name}.",
            suffix=".tmp",
            delete=False,
        ) as temporary:
            temporary_path = Path(temporary.name)
            temporary.write(serialized)
            temporary.write("\n")
            temporary.flush()
            os.fsync(temporary.fileno())
        os.replace(temporary_path, output)
        temporary_path = None
    finally:
        if temporary_path is not None:
            temporary_path.unlink(missing_ok=True)


def _metric_result(
    samples: Sequence[float], *, exact: bool = False
) -> budget.MetricResult:
    values = [float(value) for value in samples]
    aggregates = {"exact": values[0]} if exact else {"median": budget.median(values)}
    if not exact and len(values) in (10, 30):
        aggregates["nearest_rank_p95"] = budget.nearest_rank_percentile(values, 95)
    return budget.MetricResult(
        samples=values,
        aggregates=aggregates,
        classification=budget.Classification.PASS,
        diagnostics=[],
    )


def _invalid_metric(
    diagnostics: Sequence[object], *, samples: Sequence[float] = ()
) -> budget.MetricResult:
    messages = [str(item) for item in diagnostics if str(item)]
    return budget.MetricResult(
        samples=[float(sample) for sample in samples],
        aggregates={},
        classification=budget.Classification.INVALID,
        diagnostics=messages or ["collector returned invalid evidence"],
    )


def _collector_metric(
    evidence: dict[str, object], samples: Sequence[float], *, exact: bool = False
) -> budget.MetricResult:
    if evidence.get("status") == "valid":
        return _metric_result(samples, exact=exact)
    diagnostics: list[object] = list(evidence.get("failures") or [])
    if evidence.get("diagnostic"):
        diagnostics.append(evidence["diagnostic"])
    return _invalid_metric(diagnostics, samples=samples)


def _wait_for_private_daemon(
    *,
    runtime: budget.PrivateRuntime,
    process: subprocess.Popen[bytes],
    timeout_s: float,
) -> None:
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        exit_code = process.poll()
        if exit_code is not None:
            raise subprocess.CalledProcessError(exit_code, process.args)
        try:
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
                connection.settimeout(0.02)
                connection.connect(str(runtime.socket_path))
                return
        except OSError:
            time.sleep(0.005)
    raise subprocess.TimeoutExpired(process.args, timeout_s)


def ensure_process_running(
    process: subprocess.Popen[bytes], *, phase: str
) -> None:
    exit_code = process.poll()
    if exit_code is not None:
        raise subprocess.CalledProcessError(
            exit_code, process.args, output=f"private daemon exited before {phase}"
        )


def _frame_work_evidence(*, cwd: Path) -> dict[str, object]:
    command = [
        str(cwd / "scripts/dev_cargo.sh"),
        "test",
        "--profile",
        "selfdev",
        "-p",
        "jcode-desktop2",
        "profile::",
        "--",
        "--test-threads=1",
    ]
    try:
        subprocess.run(
            command,
            cwd=cwd,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
    except (OSError, subprocess.SubprocessError) as error:
        return {"status": "invalid", "failures": [str(error)], "command": command}
    return {"status": "valid", "samples": [0.0], "command": command}


def _environment_provenance(
    *, definitions: dict[str, budget.MetricDefinition]
) -> budget.EnvironmentProvenance:
    rust = subprocess.run(
        ["rustc", "--version"],
        capture_output=True,
        text=True,
        check=False,
        timeout=15,
    )
    return budget.EnvironmentProvenance(
        platform={
            "os": platform.system().lower(),
            "architecture": platform.machine(),
            "kernel": platform.release(),
            "logical_cpu_count": os.cpu_count(),
        },
        toolchain_profile={
            "python": platform.python_version(),
            "rust": (rust.stdout or rust.stderr).strip() or "unknown",
            "cargo_profile": "selfdev",
            "collectors": {"runtime_budget": budget.SCHEMA_VERSION},
        },
        command_parameters={
            "command": "python3 scripts/bench_runtime_budgets.py collect",
            "metrics": {
                metric_id: definition.sampling
                for metric_id, definition in definitions.items()
            },
        },
    )


def collect_runtime_report(*, binary: Path, output: Path) -> budget.RuntimeReport:
    if platform.system() != "Linux" or not Path("/proc").is_dir():
        raise budget.ValidationError("Linux procfs support is required for collection")

    started_at = datetime.now(timezone.utc).isoformat()
    cwd = Path.cwd().resolve()
    memory_collector, daemon_collector, startup_collector = _load_collectors()
    candidate = budget.inspect_executable(binary)
    definitions = budget.canonical_metric_definitions()
    runtime = budget.PrivateRuntime.create()
    process: subprocess.Popen[bytes] | None = None
    owned_processes: list[budget.OwnedProcess] = []
    cleanup = budget.CleanupResult(all_stopped=True, diagnostics=[])
    private_root = runtime.root

    try:
        process = subprocess.Popen(
            [candidate.resolved_path, "serve", "--socket", str(runtime.socket_path)],
            cwd=cwd,
            env=runtime.environment(),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
        owned_processes.append(budget.OwnedProcess.capture(process.pid))
        _wait_for_private_daemon(
            runtime=runtime, process=process, timeout_s=PRIVATE_DAEMON_TIMEOUT_S
        )
        daemon_pid = budget.socket_owner_pid(runtime.socket_path)
        budget.verify_daemon_executable(candidate, daemon_pid)
        if daemon_pid != process.pid:
            owned_processes.append(budget.OwnedProcess.capture(daemon_pid))
        daemon = {
            "pid": daemon_pid,
            "resolved_path": candidate.resolved_path,
            "socket": str(runtime.socket_path),
        }

        ensure_process_running(process, phase="idle resource collection")
        idle = memory_collector.collect_idle_resources(daemon_pid)
        ensure_process_running(process, phase="local round trips")
        round_trips = collect_local_round_trips(runtime=runtime)
        startup = startup_collector.collect_startup_samples(
            binary=Path(candidate.resolved_path),
            cwd=cwd,
            timeout_s=startup_collector.DEFAULT_TIMEOUT_S,
            environment=runtime.environment(),
        )
        readiness = daemon_collector.collect_server_readiness(
            binary=Path(candidate.resolved_path)
        )
        scaling = memory_collector.collect_session_scaling(
            binary=Path(candidate.resolved_path), cwd=cwd
        )
        frame = _frame_work_evidence(cwd=cwd)
        budget.verify_executable_unchanged(candidate)

        startup_samples = list(startup.get("samples") or [])
        readiness_samples = list(readiness.get("samples") or [])
        idle_samples = list(idle.get("samples") or [])
        metrics = {
            "first_visible_ms": _collector_metric(
                startup,
                [
                    sample["first_visible_ms"]
                    for sample in startup_samples
                    if sample.get("first_visible_ms") is not None
                ],
            ),
            "input_ready_ms": _collector_metric(
                startup,
                [
                    sample["input_ready_ms"]
                    for sample in startup_samples
                    if sample.get("input_ready_ms") is not None
                ],
            ),
            "daemon_ready_ms": _collector_metric(
                readiness,
                [
                    sample["elapsed_ms"]
                    for sample in readiness_samples
                    if sample.get("elapsed_ms") is not None
                ],
            ),
            "idle_cpu_percent": _collector_metric(
                idle,
                [
                    sample["cpu_percent"]
                    for sample in idle_samples
                    if sample.get("cpu_percent") is not None
                ],
            ),
            "idle_rss_mib": _collector_metric(
                idle,
                [
                    sample["rss_mib"]
                    for sample in idle_samples
                    if sample.get("rss_mib") is not None
                ],
            ),
            "session_scaling_mib_per_session": _collector_metric(
                scaling, list(scaling.get("incremental_mib_per_session_samples") or [])
            ),
            "frame_update_work_count": _collector_metric(
                frame, list(frame.get("samples") or []), exact=True
            ),
            "protocol_round_trip_ms": _collector_metric(
                round_trips["protocol_round_trip_ms"],
                list(round_trips["protocol_round_trip_ms"].get("samples") or []),
            ),
            "tool_round_trip_ms": _collector_metric(
                round_trips["tool_round_trip_ms"],
                list(round_trips["tool_round_trip_ms"].get("samples") or []),
            ),
        }
    finally:
        cleanup = budget.cleanup_owned_processes(owned_processes)
        if process is not None:
            try:
                process.wait(timeout=0.1)
            except subprocess.TimeoutExpired:
                pass
        runtime.cleanup()

    if not cleanup.all_stopped or private_root.exists():
        raise budget.IsolationError(
            "; ".join(cleanup.diagnostics) or "private runtime cleanup was incomplete"
        )
    report = budget.RuntimeReport(
        schema_version=budget.SCHEMA_VERSION,
        executable=candidate.to_dict(),
        daemon=daemon,
        environment=_environment_provenance(definitions=definitions),
        command={
            "argv": [
                "python3",
                "scripts/bench_runtime_budgets.py",
                "collect",
                "--binary",
                str(binary),
                "--output",
                str(output),
            ],
            "started_at": started_at,
        },
        definitions=definitions,
        metrics=metrics,
        baseline=None,
        cleanup={
            "owned_processes_stopped": cleanup.all_stopped,
            "private_paths_removed": not private_root.exists(),
            "diagnostics": cleanup.diagnostics,
        },
    )
    serialized = serialize_report(report)
    budget.validate_report(report)
    write_report_atomic(output=output, serialized=serialized)
    return report


def _load_report(path: Path) -> budget.RuntimeReport:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise budget.ValidationError(f"report JSON is malformed: {path}") from error
    try:
        report = budget.RuntimeReport.from_dict(value)
    except (KeyError, TypeError, ValueError) as error:
        raise budget.ValidationError(f"report schema is invalid: {path}") from error
    budget.validate_report(report)
    return report


def _load_baseline(path: Path) -> budget.RuntimeBaseline:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise budget.ValidationError(f"baseline JSON is malformed: {path}") from error
    try:
        return budget.RuntimeBaseline.from_dict(value)
    except (KeyError, TypeError, ValueError) as error:
        raise budget.ValidationError(f"baseline schema is invalid: {path}") from error


def _classify_metric(
    definition: budget.MetricDefinition,
    candidate: budget.MetricResult,
    baseline: budget.MetricResult,
) -> budget.MetricResult:
    if candidate.classification in (
        budget.Classification.INVALID,
        budget.Classification.UNSUPPORTED,
    ):
        return candidate

    policy_kind = definition.policy.get("kind")
    if policy_kind == "deterministic":
        limit = definition.policy.get(
            "ceiling", definition.policy.get("unchanged_transcript_layout_work")
        )
        if not isinstance(limit, (int, float)):
            raise budget.ValidationError(
                f"deterministic metric {definition.id} has no numeric limit"
            )
        classification = (
            budget.Classification.DETERMINISTIC_FAILURE
            if any(
                candidate.aggregates[name] > limit
                for name in definition.aggregation
            )
            else budget.Classification.PASS
        )
    elif policy_kind == "noisy_comparison":
        classifications = (
            budget.classify_tolerance(
                candidate=candidate.aggregates[name],
                baseline=baseline.aggregates[name],
                relative_tolerance=float(definition.policy["relative_tolerance"]),
                absolute_tolerance=float(definition.policy["absolute_tolerance"]),
            )
            for name in definition.aggregation
        )
        classification = (
            budget.Classification.REVIEW_REQUIRED
            if budget.Classification.REVIEW_REQUIRED in classifications
            else budget.Classification.PASS
        )
    else:
        raise budget.ValidationError(
            f"metric {definition.id} has unsupported policy kind: {policy_kind}"
        )
    return replace(candidate, classification=classification)


def compare_report(*, report_path: Path, baseline_path: Path) -> budget.RuntimeReport:
    report = _load_report(report_path)
    baseline = _load_baseline(baseline_path)
    try:
        budget.validate_baseline_compatibility(report, baseline)
    except budget.CompatibilityError as error:
        raise budget.CompatibilityError(f"incompatible baseline: {error}") from error
    if report.definitions != baseline.definitions or set(report.metrics) != set(
        baseline.metrics
    ):
        raise budget.CompatibilityError(
            "report and baseline metric contracts are incompatible"
        )
    for metric_id, definition in baseline.definitions.items():
        result = baseline.metrics[metric_id]
        budget.validate_metric_result(definition, result)
        if result.classification is not budget.Classification.PASS:
            raise budget.ValidationError(
                f"baseline metric {metric_id} is not valid passing evidence"
            )

    classified_metrics = {
        metric_id: _classify_metric(
            definition,
            report.metrics[metric_id],
            baseline.metrics[metric_id],
        )
        for metric_id, definition in report.definitions.items()
    }
    baseline_fingerprint = hashlib.sha256(baseline_path.read_bytes()).hexdigest()
    return replace(
        report,
        metrics=classified_metrics,
        baseline={
            "path": str(baseline_path),
            "sha256": baseline_fingerprint,
        },
    )


def create_baseline(
    *, report_path: Path, output: Path, reason: str, overwrite: bool
) -> budget.RuntimeBaseline:
    report = _load_report(report_path)
    if any(
        result.classification is not budget.Classification.PASS
        for result in report.metrics.values()
    ):
        raise budget.ValidationError(
            "baseline requires complete valid passing report evidence"
        )
    if output.exists() and not overwrite:
        raise budget.ValidationError(
            f"baseline output already exists; pass --overwrite to replace it: {output}"
        )
    baseline = budget.RuntimeBaseline(
        schema_version=report.schema_version,
        collected_at=str(report.command["started_at"]),
        reason=reason,
        environment=report.environment,
        definitions=report.definitions,
        metrics=report.metrics,
    )
    serialized = json.dumps(baseline.to_dict(), indent=2, sort_keys=True)
    write_report_atomic(output=output, serialized=serialized)
    return baseline


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        if args.command == "collect":
            report = collect_runtime_report(binary=args.binary, output=args.output)
        elif args.command == "compare":
            report = compare_report(
                report_path=args.report, baseline_path=args.baseline
            )
        else:
            baseline = create_baseline(
                report_path=args.report,
                output=args.output,
                reason=args.reason,
                overwrite=args.overwrite,
            )
            print(f"baseline written: {args.output} ({baseline.reason})")
            return ExitCode.PASS
        serialized = serialize_report(report)
        print(render_human_report(serialized))
        classification = _overall_classification(report)
        return {
            budget.Classification.PASS: ExitCode.PASS,
            budget.Classification.REVIEW_REQUIRED: ExitCode.REVIEW_REQUIRED,
            budget.Classification.DETERMINISTIC_FAILURE: ExitCode.DETERMINISTIC_FAILURE,
            budget.Classification.INVALID: ExitCode.INVALID,
            budget.Classification.UNSUPPORTED: ExitCode.UNSUPPORTED,
        }[classification]
    except (
        budget.RuntimeBudgetError,
        OSError,
        subprocess.SubprocessError,
        ValueError,
    ) as error:
        print(f"invalid: {error}", file=sys.stderr)
        return ExitCode.INVALID


if __name__ == "__main__":
    raise SystemExit(main())
