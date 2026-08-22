#!/usr/bin/env python3
from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import math
import os
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
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

CLI_PATH = Path(__file__).with_name("bench_runtime_budgets.py")


def copy_executable(directory: Path, name: str = "jcode-fixture") -> Path:
    candidate = directory / name
    shutil.copy2(sys.executable, candidate)
    candidate.chmod(0o755)
    return candidate


def stop_process(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is not None:
        return
    os.killpg(process.pid, signal.SIGKILL)
    process.wait(timeout=2.0)


def load_cli_module():
    spec = importlib.util.spec_from_file_location("bench_runtime_budgets", CLI_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"unable to load runtime budget CLI from {CLI_PATH}")
    sys.modules["runtime_budget"] = budget
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def sample_report() -> budget.RuntimeReport:
    definition = budget.MetricDefinition(
        id="daemon_ready",
        unit="ms",
        sampling={"recorded_count": 1},
        aggregation=["median"],
        policy={"kind": "deterministic", "ceiling": 80.0},
    )
    result = budget.MetricResult(
        samples=[12.0],
        aggregates={"median": 12.0},
        classification=budget.Classification.PASS,
        diagnostics=[],
    )
    return budget.RuntimeReport(
        schema_version="1.0.0",
        executable={"resolved_path": "/tmp/jcode", "sha256": "a" * 64},
        daemon={"resolved_path": "/tmp/jcode", "pid": 123},
        environment=budget.EnvironmentProvenance(
            platform={"os": "linux", "architecture": "x86_64"},
            toolchain_profile={"python": "3.11"},
            command_parameters={"recorded_count": 1},
        ),
        command={"argv": ["collect"]},
        definitions={definition.id: definition},
        metrics={definition.id: result},
        baseline=None,
        cleanup={"all_stopped": True},
    )


def canonical_report() -> budget.RuntimeReport:
    definitions = budget.canonical_metric_definitions()
    metrics = {
        metric_id: metric_result(definition, default_metric_value(metric_id))
        for metric_id, definition in definitions.items()
    }
    return budget.RuntimeReport(
        schema_version=budget.SCHEMA_VERSION,
        executable={
            "requested_path": "/tmp/jcode",
            "resolved_path": "/tmp/jcode",
            "version_revision": "jcode fixture",
            "sha256": "a" * 64,
        },
        daemon={
            "resolved_path": "/tmp/jcode",
            "pid": 123,
            "socket": "/tmp/jcode-runtime-budget.sock",
        },
        environment=budget.EnvironmentProvenance(
            platform={"os": "linux", "architecture": "x86_64"},
            toolchain_profile={"python": "3.11", "profile": "selfdev"},
            command_parameters={"suite": "runtime-budget-fixture"},
        ),
        command={"argv": ["collect"], "started_at": "2026-08-21T00:00:00Z"},
        definitions=definitions,
        metrics=metrics,
        baseline=None,
        cleanup={"owned_processes_stopped": True, "private_paths_removed": True},
    )


def default_metric_value(metric_id: str) -> float:
    if metric_id == "daemon_ready_ms":
        return 12.0
    if metric_id == "frame_update_work_count":
        return 0.0
    return 10.0


def metric_result(
    definition: budget.MetricDefinition,
    value: float,
    *,
    classification: budget.Classification = budget.Classification.PASS,
) -> budget.MetricResult:
    recorded_count = definition.sampling["recorded_count"]
    samples = [value] * recorded_count
    aggregates: dict[str, float] = {}
    for aggregation in definition.aggregation:
        if aggregation == "median":
            aggregates[aggregation] = budget.median(samples)
        elif aggregation == "nearest_rank_p95":
            aggregates[aggregation] = budget.nearest_rank_percentile(samples, 95)
        elif aggregation == "exact":
            aggregates[aggregation] = value
        else:
            raise AssertionError(f"unsupported fixture aggregation: {aggregation}")
    return budget.MetricResult(
        samples=samples,
        aggregates=aggregates,
        classification=classification,
        diagnostics=[],
    )


def metric_result_with_target_aggregate(
    definition: budget.MetricDefinition,
    *,
    baseline_value: float,
    aggregation: str,
    target_value: float,
) -> budget.MetricResult:
    recorded_count = definition.sampling["recorded_count"]
    if aggregation == "median":
        samples = [target_value] * recorded_count
    else:
        rank = math.ceil(0.95 * recorded_count)
        target_count = recorded_count - rank + 1
        samples = [baseline_value] * (recorded_count - target_count)
        samples.extend([target_value] * target_count)
    aggregates = {
        name: (
            budget.median(samples)
            if name == "median"
            else budget.nearest_rank_percentile(samples, 95)
        )
        for name in definition.aggregation
    }
    return budget.MetricResult(
        samples=samples,
        aggregates=aggregates,
        classification=budget.Classification.PASS,
        diagnostics=[],
    )


def baseline_for(report: budget.RuntimeReport) -> budget.RuntimeBaseline:
    return budget.RuntimeBaseline(
        schema_version=report.schema_version,
        collected_at="2026-08-21T00:00:00Z",
        reason="accepted fixture baseline",
        environment=report.environment,
        definitions=report.definitions,
        metrics=report.metrics,
    )


def write_json(path: Path, value: object) -> None:
    serializable = value.to_dict() if hasattr(value, "to_dict") else value
    path.write_text(
        json.dumps(serializable, indent=2, sort_keys=True), encoding="utf-8"
    )


class CommandShellTests(unittest.TestCase):
    def run_cli(self, *arguments: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(CLI_PATH), *arguments],
            capture_output=True,
            text=True,
            check=False,
        )

    def test_subcommands_document_required_inputs_and_exit_categories(self) -> None:
        top_level = self.run_cli("--help")
        self.assertEqual(top_level.returncode, 0, top_level.stderr)
        for category in (
            "0 pass",
            "2 usage",
            "3 review_required",
            "4 deterministic_failure",
            "5 invalid",
            "6 unsupported",
        ):
            self.assertIn(category, top_level.stdout)

        expected_options = {
            "collect": ("--binary", "--output"),
            "compare": ("--report", "--baseline"),
            "baseline": ("--report", "--output", "--reason", "--overwrite"),
        }
        for command, options in expected_options.items():
            with self.subTest(command=command):
                help_result = self.run_cli(command, "--help")
                self.assertEqual(help_result.returncode, 0, help_result.stderr)
                for option in options:
                    self.assertIn(option, help_result.stdout)
                missing = self.run_cli(command)
                self.assertEqual(missing.returncode, 2)

        empty_reason = self.run_cli(
            "baseline",
            "--report",
            "report.json",
            "--output",
            "baseline.json",
            "--reason",
            "",
        )
        self.assertEqual(empty_reason.returncode, 2)

    def test_json_and_human_output_share_one_validated_serialization(self) -> None:
        cli = load_cli_module()
        serialized = cli.serialize_report(sample_report())
        parsed = json.loads(serialized)

        self.assertEqual(parsed["metrics"]["daemon_ready"]["classification"], "pass")
        summary = cli.render_human_report(serialized)
        self.assertIn("daemon_ready: pass", summary)
        self.assertIn("overall: pass", summary)

    def test_private_daemon_exit_is_reported_before_followup_collection(self) -> None:
        cli = load_cli_module()
        process = mock.Mock()
        process.poll.return_value = 9

        with self.assertRaises(subprocess.CalledProcessError) as caught:
            cli.ensure_process_running(process, phase="local round trips")

        self.assertEqual(caught.exception.returncode, 9)


class ExecutableIdentityTests(unittest.TestCase):
    def test_missing_executable_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            missing = Path(temp_dir) / "missing-jcode"

            with self.assertRaises(budget.IdentityError):
                budget.inspect_executable(missing)

    def test_non_executable_candidate_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            candidate = copy_executable(Path(temp_dir))
            candidate.chmod(0o644)

            with self.assertRaises(budget.IdentityError):
                budget.inspect_executable(candidate)

    def test_symlink_alias_is_rejected_as_a_candidate_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            candidate = copy_executable(root)
            alias = root / "jcode-alias"
            alias.symlink_to(candidate)

            with self.assertRaises(budget.IdentityError):
                budget.inspect_executable(alias)

    def test_candidate_older_than_the_declared_build_start_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            candidate = copy_executable(Path(temp_dir))
            build_started_ns = candidate.stat().st_mtime_ns + 1

            with self.assertRaises(budget.IdentityError):
                budget.inspect_executable(
                    candidate, not_before_mtime_ns=build_started_ns
                )

    def test_candidate_changed_after_preflight_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            candidate = copy_executable(Path(temp_dir))
            identity = budget.inspect_executable(candidate)
            with candidate.open("ab") as executable:
                executable.write(b"changed-after-preflight")

            with self.assertRaises(budget.IdentityError):
                budget.verify_executable_unchanged(identity)

    def test_running_daemon_executable_must_match_the_candidate(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            candidate = copy_executable(Path(temp_dir))
            identity = budget.inspect_executable(candidate)
            daemon = subprocess.Popen(
                ["/bin/sleep", "60"],
                start_new_session=True,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            self.addCleanup(stop_process, daemon)

            with self.assertRaises(budget.IdentityError):
                budget.verify_daemon_executable(identity, daemon.pid)


class PrivateRuntimeTests(unittest.TestCase):
    def test_private_runtime_paths_are_unique_and_absent_before_launch(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            first = budget.PrivateRuntime.create(parent_dir=Path(temp_dir))
            second = budget.PrivateRuntime.create(parent_dir=Path(temp_dir))
            self.addCleanup(first.cleanup)
            self.addCleanup(second.cleanup)

            self.assertNotEqual(first.root, second.root)
            self.assertNotEqual(first.home_dir, second.home_dir)
            self.assertNotEqual(first.runtime_dir, second.runtime_dir)
            self.assertNotEqual(first.socket_path, second.socket_path)
            self.assertFalse(first.socket_path.exists())
            self.assertFalse(second.socket_path.exists())

    def test_private_runtime_enables_only_its_local_debug_control(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            runtime = budget.PrivateRuntime.create(parent_dir=Path(temp_dir))
            self.addCleanup(runtime.cleanup)

            environment = runtime.environment()

            self.assertEqual(environment["JCODE_DEBUG_CONTROL"], "1")
            self.assertEqual(environment["JCODE_SOCKET"], str(runtime.socket_path))

    def test_pre_existing_socket_is_rejected_and_preserved(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            socket_path = Path(temp_dir) / "existing.sock"
            server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            self.addCleanup(server.close)
            server.bind(str(socket_path))

            with self.assertRaises(budget.IsolationError):
                budget.PrivateRuntime.create(
                    parent_dir=Path(temp_dir), socket_path=socket_path
                )

            self.assertTrue(socket_path.exists())

    def test_socket_must_be_owned_by_the_tracked_daemon(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            socket_path = Path(temp_dir) / "owned.sock"
            server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            self.addCleanup(server.close)
            server.bind(str(socket_path))
            unrelated = subprocess.Popen(
                ["/bin/sleep", "60"],
                start_new_session=True,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            self.addCleanup(stop_process, unrelated)

            budget.verify_socket_owner(socket_path, os.getpid())
            socket_owner_pid = getattr(budget, "socket_owner_pid", None)
            self.assertIsNotNone(
                socket_owner_pid,
                "runtime ownership must expose the actual socket-owning pid",
            )
            self.assertEqual(socket_owner_pid(socket_path), os.getpid())
            with self.assertRaises(budget.IsolationError):
                budget.verify_socket_owner(socket_path, unrelated.pid)

    def test_cleanup_is_bounded_and_preserves_unrelated_process_groups(self) -> None:
        owned = subprocess.Popen(
            [
                sys.executable,
                "-c",
                "import signal,time; "
                "signal.signal(signal.SIGTERM, signal.SIG_IGN); "
                "print('ready', flush=True); time.sleep(60)",
            ],
            start_new_session=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
        unrelated = subprocess.Popen(
            ["/bin/sleep", "60"],
            start_new_session=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        self.addCleanup(stop_process, owned)
        self.addCleanup(stop_process, unrelated)
        self.assertIsNotNone(owned.stdout)
        self.addCleanup(owned.stdout.close)
        self.assertEqual(owned.stdout.readline(), b"ready\n")
        record = budget.OwnedProcess.capture(owned.pid)

        started = time.monotonic()
        result = budget.cleanup_owned_processes(
            [record], term_timeout_s=0.05, kill_timeout_s=0.5
        )

        self.assertLess(time.monotonic() - started, 1.0)
        self.assertTrue(result.all_stopped)
        self.assertIsNotNone(owned.poll())
        self.assertIsNone(unrelated.poll())


class CollectOrchestrationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.cli = load_cli_module()

    def require_local_round_trip_collector(self):
        collector = getattr(self.cli, "collect_local_round_trips", None)
        self.assertIsNotNone(
            collector,
            "collect orchestration must expose private local round-trip collection",
        )
        return collector

    def operation_fixtures(
        self,
        runtime: budget.PrivateRuntime,
        *,
        protocol_results: list[object] | None = None,
        tool_results: list[object] | None = None,
    ) -> tuple[mock.Mock, mock.Mock]:
        expected_tool_input = {"command": "printf jcode-runtime-budget"}

        def protocol_operation(
            *, socket_path: Path, protocol_version: int, timeout_s: float
        ) -> object:
            self.assertEqual(socket_path, runtime.socket_path)
            self.assertEqual(protocol_version, 1)
            self.assertEqual(timeout_s, 1.0)
            assert protocol_results is not None
            result = protocol_results[protocol.call_count - 1]
            if isinstance(result, BaseException):
                raise result
            return result

        def tool_operation(
            *,
            socket_path: Path,
            session_id: str,
            tool_name: str,
            tool_input: dict[str, str],
            timeout_s: float,
        ) -> object:
            self.assertEqual(socket_path, runtime.socket_path.with_name("jcode-debug.sock"))
            self.assertEqual(session_id, "session-runtime-budget")
            self.assertEqual(tool_name, "bash")
            self.assertEqual(tool_input, expected_tool_input)
            self.assertEqual(timeout_s, 1.0)
            assert tool_results is not None
            result = tool.call_count - 1
            fixture = tool_results[result]
            if isinstance(fixture, BaseException):
                raise fixture
            return fixture

        protocol = mock.Mock(side_effect=protocol_operation)
        tool = mock.Mock(side_effect=tool_operation)
        return protocol, tool

    @staticmethod
    def elapsed_results(offset: float = 0.0) -> list[dict[str, float]]:
        return [{"elapsed_ms": offset + float(index)} for index in range(35)]

    def collect_round_trips(
        self,
        runtime: budget.PrivateRuntime,
        protocol_results: list[object],
        tool_results: list[object],
    ) -> tuple[dict[str, object], mock.Mock, mock.Mock]:
        collect_local_round_trips = self.require_local_round_trip_collector()
        protocol, tool = self.operation_fixtures(
            runtime,
            protocol_results=protocol_results,
            tool_results=tool_results,
        )
        with (
            mock.patch.object(
                self.cli,
                "run_versioned_protocol_operation",
                protocol,
                create=True,
            ),
            mock.patch.object(
                self.cli,
                "run_deterministic_local_tool_operation",
                tool,
                create=True,
            ),
            mock.patch.object(
                self.cli,
                "prepare_debug_tool_session",
                return_value="session-runtime-budget",
                create=True,
            ) as prepare_session,
        ):
            result = collect_local_round_trips(runtime=runtime, timeout_s=1.0)
        prepare_session.assert_called_once_with(runtime=runtime, timeout_s=1.0)
        return result, protocol, tool

    def test_local_round_trips_exclude_five_warmups_and_retain_thirty_samples(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            runtime = budget.PrivateRuntime.create(parent_dir=Path(temp_dir))
            self.addCleanup(runtime.cleanup)

            result, protocol, tool = self.collect_round_trips(
                runtime,
                self.elapsed_results(),
                self.elapsed_results(100.0),
            )

        self.assertEqual(protocol.call_count, 35)
        self.assertEqual(tool.call_count, 35)
        for metric_id, expected_samples in (
            ("protocol_round_trip_ms", [float(index) for index in range(5, 35)]),
            ("tool_round_trip_ms", [100.0 + index for index in range(5, 35)]),
        ):
            with self.subTest(metric_id=metric_id):
                metric = result[metric_id]
                self.assertEqual(metric["status"], "valid")
                self.assertEqual(
                    metric["sampling"],
                    {"warm_up_count": 5, "recorded_count": 30},
                )
                self.assertEqual(metric["samples"], expected_samples)
                self.assertEqual(len(metric["samples"]), 30)
                self.assertEqual(
                    metric["aggregates"],
                    {
                        "median": budget.median(expected_samples),
                        "nearest_rank_p95": budget.nearest_rank_percentile(
                            expected_samples, 95
                        ),
                    },
                )

    def test_timeout_nonzero_and_partial_round_trips_are_invalid(self) -> None:
        failures = (
            (
                subprocess.TimeoutExpired("local-operation", 1.0),
                "timeout",
            ),
            (
                subprocess.CalledProcessError(17, "local-operation"),
                "nonzero_exit",
            ),
            ({"elapsed_ms": None}, "incomplete"),
        )
        for failure, expected_kind in failures:
            with (
                self.subTest(kind=expected_kind),
                tempfile.TemporaryDirectory() as temp_dir,
            ):
                runtime = budget.PrivateRuntime.create(parent_dir=Path(temp_dir))
                self.addCleanup(runtime.cleanup)
                protocol_results: list[object] = self.elapsed_results()
                protocol_results[8] = failure

                result, _protocol, _tool = self.collect_round_trips(
                    runtime,
                    protocol_results,
                    self.elapsed_results(100.0),
                )

                metric = result["protocol_round_trip_ms"]
                self.assertEqual(metric["status"], "invalid")
                self.assertEqual(metric["failures"][0]["kind"], expected_kind)
                self.assertEqual(metric["failures"][0]["recorded_run"], 4)
                self.assertEqual(metric["samples"], [5.0, 6.0, 7.0])

    def test_local_round_trips_use_only_the_private_socket(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            active_socket = root / "active-shared.sock"
            active_socket.write_text("active-shared-runtime", encoding="utf-8")
            private_parent = root / "private"
            private_parent.mkdir()
            runtime = budget.PrivateRuntime.create(parent_dir=private_parent)
            self.addCleanup(runtime.cleanup)

            with mock.patch.dict(
                os.environ,
                {
                    "JCODE_SOCKET": str(active_socket),
                    "OPENAI_API_KEY": "must-not-be-used",
                    "HTTPS_PROXY": "must-not-be-used",
                },
            ):
                result, _protocol, _tool = self.collect_round_trips(
                    runtime,
                    self.elapsed_results(),
                    self.elapsed_results(100.0),
                )

            self.assertEqual(result["protocol_round_trip_ms"]["status"], "valid")
            self.assertEqual(result["tool_round_trip_ms"]["status"], "valid")
            self.assertEqual(
                active_socket.read_text(encoding="utf-8"),
                "active-shared-runtime",
            )

    def test_report_output_is_replaced_atomically(self) -> None:
        write_report_atomic = getattr(self.cli, "write_report_atomic", None)
        self.assertIsNotNone(
            write_report_atomic,
            "collect orchestration must expose atomic report publication",
        )
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            output = root / "report.json"
            output.write_text("previous-report", encoding="utf-8")

            with mock.patch.object(
                self.cli.os,
                "replace",
                side_effect=OSError("atomic replace failed"),
            ):
                with self.assertRaisesRegex(OSError, "atomic replace failed"):
                    write_report_atomic(output=output, serialized="new-report")

            self.assertEqual(
                output.read_text(encoding="utf-8"),
                "previous-report",
            )
            self.assertEqual(
                [path for path in root.iterdir() if path != output],
                [],
                "failed atomic publication must clean its owned temporary file",
            )

    def test_frame_evidence_uses_the_repository_cargo_gate(self) -> None:
        completed = subprocess.CompletedProcess(args=[], returncode=0)
        with mock.patch.object(
            self.cli.subprocess,
            "run",
            return_value=completed,
        ) as run:
            result = self.cli._frame_work_evidence(cwd=Path.cwd())

        command = run.call_args.args[0]
        self.assertEqual(command[0], str(Path.cwd() / "scripts/dev_cargo.sh"))
        self.assertEqual(command[1], "test")
        self.assertEqual(result["status"], "valid")
        self.assertEqual(result["command"], command)

    def test_collect_command_propagates_collector_failures_as_invalid(self) -> None:
        failures = (
            (subprocess.TimeoutExpired("collector", 1.0), "timed out"),
            (subprocess.CalledProcessError(17, "collector"), "exit status 17"),
            (
                budget.ValidationError("collector returned partial samples"),
                "partial samples",
            ),
        )
        for failure, expected_diagnostic in failures:
            with (
                self.subTest(diagnostic=expected_diagnostic),
                tempfile.TemporaryDirectory() as temp_dir,
            ):
                root = Path(temp_dir)
                binary = copy_executable(root)
                output = root / "report.json"
                collect_runtime_report = mock.Mock(side_effect=failure)
                stderr = io.StringIO()

                with (
                    mock.patch.object(
                        self.cli,
                        "collect_runtime_report",
                        collect_runtime_report,
                        create=True,
                    ),
                    contextlib.redirect_stderr(stderr),
                ):
                    exit_code = self.cli.main(
                        [
                            "collect",
                            "--binary",
                            str(binary),
                            "--output",
                            str(output),
                        ]
                    )

                collect_runtime_report.assert_called_once_with(
                    binary=binary,
                    output=output,
                )
                self.assertEqual(exit_code, self.cli.ExitCode.INVALID)
                self.assertIn(expected_diagnostic, stderr.getvalue())
                self.assertFalse(output.exists())


class ComparisonAndRatchetTests(unittest.TestCase):
    def setUp(self) -> None:
        self.cli = load_cli_module()

    def invoke(self, *arguments: str) -> tuple[int, str, str]:
        stdout = io.StringIO()
        stderr = io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            exit_code = self.cli.main(list(arguments))
        return int(exit_code), stdout.getvalue(), stderr.getvalue()

    def compare(
        self,
        root: Path,
        report: budget.RuntimeReport,
        baseline: budget.RuntimeBaseline,
    ) -> tuple[int, str, str, Path, Path]:
        report_path = root / "report.json"
        baseline_path = root / "baseline.json"
        write_json(report_path, report)
        write_json(baseline_path, baseline)
        exit_code, stdout, stderr = self.invoke(
            "compare",
            "--report",
            str(report_path),
            "--baseline",
            str(baseline_path),
        )
        return exit_code, stdout, stderr, report_path, baseline_path

    def assert_comparison(
        self,
        *,
        metric_id: str,
        candidate_result: budget.MetricResult,
        expected: budget.Classification,
    ) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            reference = canonical_report()
            candidate = canonical_report()
            candidate.metrics[metric_id] = candidate_result

            exit_code, stdout, stderr, _report_path, _baseline_path = self.compare(
                Path(temp_dir), candidate, baseline_for(reference)
            )

        expected_exit = {
            budget.Classification.PASS: self.cli.ExitCode.PASS,
            budget.Classification.REVIEW_REQUIRED: self.cli.ExitCode.REVIEW_REQUIRED,
            budget.Classification.DETERMINISTIC_FAILURE: (
                self.cli.ExitCode.DETERMINISTIC_FAILURE
            ),
            budget.Classification.INVALID: self.cli.ExitCode.INVALID,
            budget.Classification.UNSUPPORTED: self.cli.ExitCode.UNSUPPORTED,
        }[expected]
        self.assertEqual(exit_code, expected_exit, stderr)
        self.assertIn(f"{metric_id}: {expected.value}", stdout)
        self.assertIn(f"overall: {expected.value}", stdout)

    def test_daemon_readiness_uses_a_strict_80_ms_deterministic_gate(self) -> None:
        definition = canonical_report().definitions["daemon_ready_ms"]
        for value, expected in (
            (79.9, budget.Classification.PASS),
            (80.0, budget.Classification.PASS),
            (80.1, budget.Classification.DETERMINISTIC_FAILURE),
        ):
            with self.subTest(value=value):
                self.assert_comparison(
                    metric_id=definition.id,
                    candidate_result=metric_result(definition, value),
                    expected=expected,
                )

    def test_frame_work_exact_invariant_uses_strict_above_behavior(self) -> None:
        definition = canonical_report().definitions["frame_update_work_count"]
        for value, expected in (
            (-1.0, budget.Classification.PASS),
            (0.0, budget.Classification.PASS),
            (1.0, budget.Classification.DETERMINISTIC_FAILURE),
        ):
            with self.subTest(value=value):
                self.assert_comparison(
                    metric_id=definition.id,
                    candidate_result=metric_result(definition, value),
                    expected=expected,
                )

    def test_every_noisy_aggregate_uses_its_strict_same_environment_tolerance(
        self,
    ) -> None:
        reference = canonical_report()
        noisy_definitions = (
            definition
            for definition in reference.definitions.values()
            if definition.policy["kind"] == "noisy_comparison"
        )
        for definition in noisy_definitions:
            baseline_value = default_metric_value(definition.id)
            tolerance = max(
                abs(baseline_value) * definition.policy["relative_tolerance"],
                definition.policy["absolute_tolerance"],
            )
            boundary = baseline_value + tolerance
            for aggregation in definition.aggregation:
                for value, expected in (
                    (boundary - 0.01, budget.Classification.PASS),
                    (boundary, budget.Classification.PASS),
                    (boundary + 0.01, budget.Classification.REVIEW_REQUIRED),
                ):
                    with self.subTest(
                        metric=definition.id,
                        aggregation=aggregation,
                        value=value,
                    ):
                        self.assert_comparison(
                            metric_id=definition.id,
                            candidate_result=metric_result_with_target_aggregate(
                                definition,
                                baseline_value=baseline_value,
                                aggregation=aggregation,
                                target_value=value,
                            ),
                            expected=expected,
                        )

    def test_compare_has_distinct_invalid_and_unsupported_exit_behavior(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            reference = canonical_report()
            incomplete = canonical_report()
            incomplete.metrics.pop("tool_round_trip_ms")
            exit_code, _stdout, stderr, _report_path, _baseline_path = self.compare(
                root, incomplete, baseline_for(reference)
            )
            self.assertEqual(exit_code, self.cli.ExitCode.INVALID)
            self.assertIn("invalid", stderr)

        unsupported = canonical_report()
        unsupported.metrics["idle_rss_mib"] = budget.MetricResult(
            samples=[],
            aggregates={},
            classification=budget.Classification.UNSUPPORTED,
            diagnostics=["procfs unavailable"],
        )
        self.assert_comparison(
            metric_id="idle_rss_mib",
            candidate_result=unsupported.metrics["idle_rss_mib"],
            expected=budget.Classification.UNSUPPORTED,
        )

    def test_compare_rejects_incompatible_schema_and_environment(self) -> None:
        cases = []
        schema_baseline = replace(
            baseline_for(canonical_report()), schema_version="2.0.0"
        )
        cases.append(("schema", schema_baseline))

        environment_baseline = baseline_for(canonical_report())
        environment_baseline = replace(
            environment_baseline,
            environment=budget.EnvironmentProvenance(
                platform={"os": "linux", "architecture": "aarch64"},
                toolchain_profile=environment_baseline.environment.toolchain_profile,
                command_parameters=environment_baseline.environment.command_parameters,
            ),
        )
        cases.append(("environment", environment_baseline))

        for name, incompatible in cases:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temp_dir:
                exit_code, _stdout, stderr, _report_path, _baseline_path = self.compare(
                    Path(temp_dir), canonical_report(), incompatible
                )
                self.assertEqual(exit_code, self.cli.ExitCode.INVALID)
                self.assertIn("incompatible", stderr)

    def test_compare_never_mutates_the_report_or_baseline(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            candidate = canonical_report()
            reference = baseline_for(canonical_report())
            report_path = root / "report.json"
            baseline_path = root / "baseline.json"
            write_json(report_path, candidate)
            write_json(baseline_path, reference)
            report_before = report_path.read_bytes()
            baseline_before = baseline_path.read_bytes()

            exit_code, _stdout, stderr = self.invoke(
                "compare",
                "--report",
                str(report_path),
                "--baseline",
                str(baseline_path),
            )

            self.assertEqual(exit_code, self.cli.ExitCode.PASS, stderr)
            self.assertEqual(report_path.read_bytes(), report_before)
            self.assertEqual(baseline_path.read_bytes(), baseline_before)

    def test_baseline_creation_requires_complete_valid_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            report = canonical_report()
            report.metrics.pop("tool_round_trip_ms")
            report_path = root / "incomplete-report.json"
            output = root / "baseline.json"
            write_json(report_path, report)

            exit_code, _stdout, stderr = self.invoke(
                "baseline",
                "--report",
                str(report_path),
                "--output",
                str(output),
                "--reason",
                "incomplete evidence must be rejected",
            )

            self.assertEqual(exit_code, self.cli.ExitCode.INVALID)
            self.assertIn("invalid", stderr)
            self.assertFalse(output.exists())

    def test_baseline_creation_is_explicit_attributable_and_overwrite_safe(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            report_path = root / "report.json"
            output = root / "baseline.json"
            write_json(report_path, canonical_report())

            exit_code, _stdout, stderr = self.invoke(
                "baseline",
                "--report",
                str(report_path),
                "--output",
                str(output),
                "--reason",
                "reviewed healthy fixture",
            )
            self.assertEqual(exit_code, self.cli.ExitCode.PASS, stderr)
            created = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(created["reason"], "reviewed healthy fixture")
            self.assertEqual(set(created["metrics"]), set(canonical_report().metrics))

            accepted_content = output.read_bytes()
            exit_code, _stdout, _stderr = self.invoke(
                "baseline",
                "--report",
                str(report_path),
                "--output",
                str(output),
                "--reason",
                "replacement without approval",
            )
            self.assertEqual(exit_code, self.cli.ExitCode.INVALID)
            self.assertEqual(output.read_bytes(), accepted_content)

            exit_code, _stdout, stderr = self.invoke(
                "baseline",
                "--report",
                str(report_path),
                "--output",
                str(output),
                "--reason",
                "explicit reviewed replacement",
                "--overwrite",
            )
            self.assertEqual(exit_code, self.cli.ExitCode.PASS, stderr)
            replaced = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(replaced["reason"], "explicit reviewed replacement")
            self.assertNotEqual(output.read_bytes(), accepted_content)


if __name__ == "__main__":
    unittest.main()
