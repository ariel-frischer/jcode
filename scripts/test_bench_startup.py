#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


MODULE_PATH = Path(__file__).with_name("bench_startup.py")
SPEC = importlib.util.spec_from_file_location("bench_startup", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"unable to load daemon startup collector from {MODULE_PATH}")
collector = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = collector
SPEC.loader.exec_module(collector)


def valid_attempt(sequence: int, binary: Path, socket_path: Path) -> dict[str, object]:
    return {
        "status": "valid",
        "elapsed_ms": float(sequence),
        "daemon": {
            "pid": 10_000 + sequence,
            "resolved_path": str(binary),
            "socket": str(socket_path),
        },
        "exit_code": None,
        "failure": None,
    }


def invalid_attempt(kind: str, *, exit_code: int | None = None) -> dict[str, object]:
    return {
        "status": "invalid",
        "elapsed_ms": None,
        "daemon": None,
        "exit_code": exit_code,
        "failure": {"kind": kind, "message": f"{kind} failure"},
    }


class StructuredDaemonReadinessTests(unittest.TestCase):
    def test_readiness_elapsed_excludes_identity_verification_overhead(self) -> None:
        runtime = mock.Mock(socket_path=Path("/tmp/private-runtime.sock"))
        runtime.environment.return_value = {}
        process = mock.Mock(pid=4242)
        process.poll.return_value = None
        cleanup = mock.Mock(all_stopped=True, diagnostics=[])
        candidate = mock.Mock(resolved_path="/candidate/jcode")
        with (
            mock.patch.object(collector.PrivateRuntime, "create", return_value=runtime),
            mock.patch.object(collector.subprocess, "Popen", return_value=process),
            mock.patch.object(collector.OwnedProcess, "capture", return_value=mock.Mock()),
            mock.patch.object(collector, "wait_for_socket", return_value=True),
            mock.patch.object(collector, "resolve_socket_owner", return_value=4242),
            mock.patch.object(collector, "verify_daemon_executable"),
            mock.patch.object(collector, "cleanup_owned_processes", return_value=cleanup),
            mock.patch.object(
                collector.time, "perf_counter", side_effect=[0.0, 0.01, 0.05]
            ),
        ):
            result = collector.measure_server_startup_attempt(
                candidate=candidate, timeout_s=5.0
            )

        self.assertEqual(result["status"], "valid")
        self.assertAlmostEqual(result["elapsed_ms"], 50.0)

    def test_socket_owner_lookup_retries_transient_procfs_races(self) -> None:
        resolve_owner = getattr(collector, "resolve_socket_owner", None)
        self.assertIsNotNone(resolve_owner)
        socket_path = Path("/tmp/private-runtime.sock")
        with (
            mock.patch.object(
                collector,
                "socket_owner_pid",
                side_effect=[collector.IsolationError("not visible yet"), 4242],
            ) as socket_owner_pid,
            mock.patch.object(collector.time, "sleep") as sleep,
        ):
            owner = resolve_owner(socket_path, deadline=collector.time.perf_counter() + 1.0)

        self.assertEqual(owner, 4242)
        self.assertEqual(socket_owner_pid.call_count, 2)
        sleep.assert_called_once_with(0.005)

    def collect(self, attempts: list[dict[str, object]]) -> dict[str, object]:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            binary = root / "candidate-jcode"
            binary.touch(mode=0o755)
            identity = mock.Mock()
            identity.to_dict.return_value = {
                "requested_path": str(binary),
                "resolved_path": str(binary),
                "version_revision": "jcode test-revision",
                "sha256": "a" * 64,
            }
            for attempt in attempts:
                daemon = attempt.get("daemon")
                if isinstance(daemon, dict) and daemon.get("resolved_path"):
                    daemon["resolved_path"] = str(binary)
            collect_server_readiness = getattr(
                collector, "collect_server_readiness", None
            )
            self.assertIsNotNone(
                collect_server_readiness,
                "collector must expose structured private daemon readiness collection",
            )

            with (
                mock.patch.object(
                    collector,
                    "inspect_executable",
                    return_value=identity,
                    create=True,
                ) as inspect_executable,
                mock.patch.object(
                    collector,
                    "measure_server_startup_attempt",
                    side_effect=attempts,
                    create=True,
                ) as measure_attempt,
            ):
                result = collect_server_readiness(binary=binary)

            inspect_executable.assert_called_once_with(binary)
            self.assertEqual(measure_attempt.call_count, 5)
            for call in measure_attempt.call_args_list:
                self.assertIs(call.kwargs["candidate"], identity)
                self.assertEqual(call.kwargs["timeout_s"], 5.0)
            self.assertEqual(
                result["candidate"],
                identity.to_dict.return_value,
            )
            return result

    def complete_attempts(self) -> list[dict[str, object]]:
        binary = Path("/tmp/candidate-jcode")
        return [
            valid_attempt(index, binary, Path(f"/tmp/private-{index}.sock"))
            for index in range(1, 6)
        ]

    def test_emits_five_complete_private_daemon_readiness_samples(self) -> None:
        result = self.collect(self.complete_attempts())

        self.assertEqual(result["status"], "valid")
        self.assertEqual(
            result["sampling"], {"recorded_count": 5, "timeout_s": 5.0}
        )
        self.assertEqual(len(result["samples"]), 5)
        self.assertEqual(
            [sample["elapsed_ms"] for sample in result["samples"]],
            [1.0, 2.0, 3.0, 4.0, 5.0],
        )
        self.assertTrue(
            all(sample["daemon"]["pid"] for sample in result["samples"])
        )
        self.assertTrue(
            all(
                sample["daemon"]["resolved_path"]
                == result["candidate"]["resolved_path"]
                for sample in result["samples"]
            )
        )
        self.assertTrue(
            all(sample["daemon"]["socket"] for sample in result["samples"])
        )
        self.assertEqual(result["failures"], [])

    def test_failed_launch_is_invalid(self) -> None:
        attempts = self.complete_attempts()
        attempts[0] = invalid_attempt("launch")

        result = self.collect(attempts)

        self.assertEqual(result["status"], "invalid")
        self.assertEqual(result["failures"][0]["kind"], "launch")

    def test_wrong_socket_owner_is_invalid(self) -> None:
        attempts = self.complete_attempts()
        attempts[1] = invalid_attempt("socket_owner")

        result = self.collect(attempts)

        self.assertEqual(result["status"], "invalid")
        self.assertEqual(result["failures"][0]["kind"], "socket_owner")

    def test_timeout_is_invalid(self) -> None:
        attempts = self.complete_attempts()
        attempts[2] = invalid_attempt("timeout")

        result = self.collect(attempts)

        self.assertEqual(result["status"], "invalid")
        self.assertEqual(result["failures"][0]["kind"], "timeout")

    def test_nonzero_exit_is_invalid(self) -> None:
        attempts = self.complete_attempts()
        attempts[3] = invalid_attempt("nonzero_exit", exit_code=17)

        result = self.collect(attempts)

        self.assertEqual(result["status"], "invalid")
        self.assertEqual(result["failures"][0]["kind"], "nonzero_exit")
        self.assertEqual(result["failures"][0]["exit_code"], 17)

    def test_partial_or_identity_incomplete_evidence_is_invalid(self) -> None:
        attempts = self.complete_attempts()
        attempts[4] = {
            "status": "valid",
            "elapsed_ms": None,
            "daemon": {"pid": 10_005, "resolved_path": None, "socket": None},
            "exit_code": None,
            "failure": None,
        }

        result = self.collect(attempts)

        self.assertEqual(result["status"], "invalid")
        self.assertEqual(len(result["samples"]), 5)
        self.assertEqual(result["failures"][0]["kind"], "incomplete")
        self.assertEqual(result["failures"][0]["recorded_run"], 5)


if __name__ == "__main__":
    unittest.main()
