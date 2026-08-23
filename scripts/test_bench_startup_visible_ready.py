#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


MODULE_PATH = Path(__file__).with_name("bench_startup_visible_ready.py")
SPEC = importlib.util.spec_from_file_location("bench_startup_visible_ready", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"unable to load startup collector from {MODULE_PATH}")
collector = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = collector
SPEC.loader.exec_module(collector)


def completed_run(sequence: int) -> dict[str, object]:
    return {
        "first_visible_ms": float(sequence),
        "first_visible_excerpt": f"Jcode ready {sequence}",
        "input_ready_ms": float(sequence) + 0.5,
        "input_ready_source": "probe_echo",
        "timed_out": False,
        "failure": None,
    }


class RenderedReadinessTests(unittest.TestCase):
    def setUp(self) -> None:
        self.screen = collector.pyte.Screen(80, 24)
        self.stream = collector.pyte.Stream(self.screen)

    def test_terminal_control_traffic_is_not_meaningful_visible_content(self) -> None:
        self.stream.feed("\x1b[6n\x1b[c\x1b]10;?\x07\x1b[?2026$p")

        self.assertIsNone(collector.first_meaningful_line(self.screen))

        self.stream.feed("\x1b[2J\x1b[HJcode ready")
        self.assertEqual(collector.first_meaningful_line(self.screen), "Jcode ready")

    def test_probe_must_be_visible_in_rendered_content(self) -> None:
        self.stream.feed(f"\x1b]0;{collector.PROBE}\x07")
        probe_visibly_accepted = getattr(collector, "probe_visibly_accepted", None)
        self.assertIsNotNone(
            probe_visibly_accepted,
            "collector must expose rendered typed-probe acceptance",
        )
        self.assertFalse(
            probe_visibly_accepted(self.screen, collector.PROBE)
        )

        self.stream.feed(collector.PROBE)
        self.assertTrue(
            probe_visibly_accepted(self.screen, collector.PROBE)
        )

    def test_probe_is_retried_when_the_first_keystroke_precedes_input_readiness(self) -> None:
        probe_send_due = getattr(collector, "probe_send_due", None)
        self.assertIsNotNone(probe_send_due)
        self.assertTrue(probe_send_due(last_sent_at=None, now=1.0))
        self.assertFalse(probe_send_due(last_sent_at=1.0, now=1.05))
        self.assertTrue(probe_send_due(last_sent_at=1.0, now=1.11))


class StructuredCollectionTests(unittest.TestCase):
    def private_environment(self, root: Path) -> dict[str, str]:
        return {
            "JCODE_HOME": str(root / "home"),
            "JCODE_RUNTIME_DIR": str(root / "run"),
            "JCODE_SOCKET": str(root / "run" / "startup.sock"),
        }

    def collect(self, side_effect: list[dict[str, object]]) -> dict[str, object]:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            binary = root / "candidate-jcode"
            binary.touch(mode=0o755)
            environment = self.private_environment(root)
            collect_startup_samples = getattr(
                collector, "collect_startup_samples", None
            )
            self.assertIsNotNone(
                collect_startup_samples,
                "collector must expose structured startup collection",
            )
            with mock.patch.object(
                collector, "run_once", side_effect=side_effect
            ) as run_once:
                result = collect_startup_samples(
                    binary=binary,
                    cwd=root,
                    timeout_s=5.0,
                    environment=environment,
                )

            self.assertEqual(run_once.call_count, 11)
            for call in run_once.call_args_list:
                tool_spec, cwd, timeout_s, run_environment = call.args
                self.assertEqual(Path(tool_spec.argv[0]), binary)
                self.assertEqual(tool_spec.input_probe_delay_s, 0.05)
                self.assertEqual(cwd, root)
                self.assertEqual(timeout_s, 5.0)
                self.assertEqual(run_environment, environment)
            return result

    def test_excludes_one_warm_up_and_emits_ten_complete_pairs(self) -> None:
        result = self.collect([completed_run(index) for index in range(11)])

        self.assertEqual(result["status"], "valid")
        self.assertEqual(
            result["sampling"], {"warm_up_count": 1, "recorded_count": 10}
        )
        self.assertEqual(len(result["samples"]), 10)
        self.assertEqual(result["samples"][0]["first_visible_ms"], 1.0)
        self.assertEqual(result["samples"][-1]["input_ready_ms"], 10.5)
        self.assertEqual(result["failures"], [])

    def test_partial_recorded_run_is_structured_invalid_evidence(self) -> None:
        runs = [completed_run(index) for index in range(11)]
        runs[6]["input_ready_ms"] = None

        result = self.collect(runs)

        self.assertEqual(result["status"], "invalid")
        self.assertEqual(len(result["samples"]), 10)
        self.assertEqual(result["failures"][0]["kind"], "incomplete")
        self.assertEqual(result["failures"][0]["recorded_run"], 6)

    def test_timed_out_recorded_run_is_structured_invalid_evidence(self) -> None:
        runs = [completed_run(index) for index in range(11)]
        runs[4].update(
            {
                "first_visible_ms": None,
                "first_visible_excerpt": None,
                "input_ready_ms": None,
                "timed_out": True,
            }
        )

        result = self.collect(runs)

        self.assertEqual(result["status"], "invalid")
        self.assertEqual(result["failures"][0]["kind"], "timeout")
        self.assertEqual(result["failures"][0]["recorded_run"], 4)

    def test_probe_write_failure_is_structured_invalid_evidence(self) -> None:
        runs = [completed_run(index) for index in range(11)]
        runs[3].update(
            {
                "input_ready_ms": None,
                "timed_out": True,
                "failure": {"kind": "probe_write", "diagnostic": "pty closed"},
            }
        )

        result = self.collect(runs)

        self.assertEqual(result["status"], "invalid")
        self.assertEqual(result["failures"][0]["kind"], "probe_write")
        self.assertEqual(result["failures"][0]["diagnostic"], "pty closed")

    def test_private_environment_is_required(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            binary = root / "candidate-jcode"
            binary.touch(mode=0o755)
            collect_startup_samples = getattr(
                collector, "collect_startup_samples", None
            )
            self.assertIsNotNone(
                collect_startup_samples,
                "collector must expose structured startup collection",
            )

            with self.assertRaisesRegex(ValueError, "JCODE_SOCKET"):
                collect_startup_samples(
                    binary=binary,
                    cwd=root,
                    timeout_s=5.0,
                    environment={
                        "JCODE_HOME": str(root / "home"),
                        "JCODE_RUNTIME_DIR": str(root / "run"),
                    },
                )


if __name__ == "__main__":
    unittest.main()
