#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from collections import Counter
from pathlib import Path
from unittest import mock


MODULE_PATH = Path(__file__).with_name("bench_memory_cli.py")
SPEC = importlib.util.spec_from_file_location("bench_memory_cli", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"unable to load memory collector from {MODULE_PATH}")
collector = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = collector
SPEC.loader.exec_module(collector)


def population_trial(
    population: int,
    trial: int,
    *,
    observed_population: int | None = None,
) -> dict[str, object]:
    slope_mib = float(9 + trial)
    return {
        "status": "valid",
        "population": population,
        "observed_population": (
            population if observed_population is None else observed_population
        ),
        "trial": trial,
        "daemon_rss_mib": 100.0 + population * slope_mib,
        "attributed_session_mib": population * slope_mib,
        "cleanup": {
            "status": "complete",
            "owned_processes_terminated": population + 1,
        },
        "failure": None,
    }


class IdleResourceCollectionTests(unittest.TestCase):
    def collect(self) -> tuple[dict[str, object], mock.Mock, mock.Mock]:
        collect_idle_resources = getattr(collector, "collect_idle_resources", None)
        self.assertIsNotNone(
            collect_idle_resources,
            "collector must expose structured idle CPU/RSS collection",
        )
        sampler = mock.Mock(
            side_effect=[
                {"cpu_percent": 0.1, "rss_mib": 100.0},
                {"cpu_percent": 0.3, "rss_mib": 104.0},
                {"cpu_percent": 0.2, "rss_mib": 102.0},
                {"cpu_percent": 0.5, "rss_mib": 108.0},
                {"cpu_percent": 0.4, "rss_mib": 106.0},
            ]
        )
        sleeper = mock.Mock()

        with (
            mock.patch.object(
                collector,
                "sample_process_resources",
                sampler,
                create=True,
            ),
            mock.patch.object(collector.time, "sleep", sleeper),
        ):
            result = collect_idle_resources(daemon_pid=4242)
        return result, sampler, sleeper

    def test_waits_five_seconds_then_collects_five_one_second_samples(self) -> None:
        result, sampler, sleeper = self.collect()

        self.assertEqual(result["status"], "valid")
        self.assertEqual(
            result["sampling"],
            {
                "settle_seconds": 5.0,
                "sample_interval_seconds": 1.0,
                "recorded_count": 5,
            },
        )
        self.assertEqual(sampler.call_count, 5)
        sampler.assert_has_calls([mock.call(4242)] * 5)
        self.assertEqual(
            sleeper.call_args_list,
            [mock.call(5.0)] + [mock.call(1.0)] * 4,
        )
        self.assertEqual(len(result["samples"]), 5)
        self.assertEqual(result["aggregates"]["median_cpu_percent"], 0.3)
        self.assertEqual(result["aggregates"]["median_rss_mib"], 104.0)

    def test_unsupported_platform_is_explicit_evidence(self) -> None:
        collect_idle_resources = getattr(collector, "collect_idle_resources", None)
        self.assertIsNotNone(
            collect_idle_resources,
            "collector must expose structured idle CPU/RSS collection",
        )

        result = collect_idle_resources(daemon_pid=4242, platform_name="Darwin")

        self.assertEqual(result["status"], "unsupported")
        self.assertEqual(result["platform"], "Darwin")
        self.assertEqual(result["samples"], [])
        self.assertIn("procfs", result["diagnostic"].lower())


class SessionScalingCollectionTests(unittest.TestCase):
    def test_scaling_environment_uses_supported_attribution_intervals(self) -> None:
        env = collector.scaling_environment("/tmp/private-runtime")

        self.assertEqual(
            env["JCODE_RUNTIME_MEMORY_LOG_ATTRIBUTION_INTERVAL_SECS"], "60"
        )
        self.assertEqual(
            env["JCODE_RUNTIME_MEMORY_LOG_ATTRIBUTION_MIN_SPACING_SECS"], "5"
        )

    def test_population_trial_spaces_session_events_for_complete_attribution(self) -> None:
        self.assertEqual(collector.ATTRIBUTION_SETTLE_S, 5.1)
        process = mock.Mock(pid=4242)
        process.poll.return_value = None
        launch = collector.SessionLaunch(
            5001, 5001, 31, True, False, "connecting", 0.1, None, None
        )
        with (
            mock.patch.object(
                collector.subprocess, "Popen", return_value=process
            ) as popen,
            mock.patch.object(collector.os, "getpgid", side_effect=lambda pid: pid),
            mock.patch.object(collector, "wait_for_socket", return_value=True),
            mock.patch.object(
                collector, "create_debug_session", side_effect=[f"session-{n}" for n in range(4)]
            ) as create_session,
            mock.patch.object(
                collector, "launch_interactive", return_value=launch
            ) as attach_client,
            mock.patch.object(collector, "_read_proc_rss_mib", return_value=100.0),
            mock.patch.object(
                collector, "_runtime_memory_attribution", return_value=(4, 40.0)
            ),
            mock.patch.object(collector, "terminate_pgroup"),
            mock.patch.object(collector.os, "close"),
            mock.patch.object(collector.shutil, "rmtree"),
            mock.patch.object(collector.time, "sleep") as sleep,
        ):
            result = collector.run_population_trial(
                binary=Path("/candidate/jcode"),
                cwd=Path.cwd(),
                population=4,
                trial=1,
            )

        self.assertEqual(result["status"], "valid")
        server_env = popen.call_args.kwargs["env"]
        self.assertEqual(
            server_env["JCODE_RUNTIME_MEMORY_LOG_PROCESS_INTERVAL_SECS"], "15"
        )
        self.assertEqual(server_env["JCODE_DEBUG_CONTROL"], "1")
        self.assertEqual(create_session.call_count, 4)
        self.assertEqual(attach_client.call_count, 4)
        self.assertEqual(
            sleep.call_args_list,
            [mock.call(collector.ATTRIBUTION_SETTLE_S)] * 5
            + [mock.call(collector.IDLE_SETTLE_S)],
        )

    def collect(
        self, trials: list[dict[str, object]]
    ) -> tuple[dict[str, object], mock.Mock]:
        collect_session_scaling = getattr(collector, "collect_session_scaling", None)
        self.assertIsNotNone(
            collect_session_scaling,
            "collector must expose structured 1/4/8 session scaling collection",
        )
        run_trial = mock.Mock(side_effect=trials)
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            binary = root / "candidate-jcode"
            binary.touch(mode=0o755)
            with mock.patch.object(
                collector,
                "run_population_trial",
                run_trial,
                create=True,
            ):
                result = collect_session_scaling(binary=binary, cwd=root)
        return result, run_trial

    def complete_trials(self) -> list[dict[str, object]]:
        return [
            population_trial(population, trial)
            for trial in range(1, 4)
            for population in (1, 4, 8)
        ]

    def test_collects_three_trials_for_each_required_population(self) -> None:
        result, run_trial = self.collect(self.complete_trials())

        self.assertEqual(result["status"], "valid")
        self.assertEqual(
            result["sampling"],
            {"populations": [1, 4, 8], "trials_per_population": 3},
        )
        self.assertEqual(run_trial.call_count, 9)
        self.assertEqual(
            Counter(call.kwargs["population"] for call in run_trial.call_args_list),
            Counter({1: 3, 4: 3, 8: 3}),
        )
        self.assertEqual(len(result["trials"]), 9)

    def test_reports_daemon_rss_attribution_and_median_incremental_slope(self) -> None:
        result, _run_trial = self.collect(self.complete_trials())

        for trial in result["trials"]:
            self.assertIsInstance(trial["daemon_rss_mib"], float)
            self.assertIsInstance(trial["attributed_session_mib"], float)
            self.assertEqual(trial["population"], trial["observed_population"])
            self.assertEqual(trial["cleanup"]["status"], "complete")
        self.assertEqual(
            result["incremental_mib_per_session_samples"],
            [10.0, 11.0, 12.0],
        )
        self.assertEqual(result["median_incremental_mib_per_session"], 11.0)

    def test_population_mismatch_is_invalid_evidence(self) -> None:
        trials = self.complete_trials()
        trials[4] = population_trial(4, 2, observed_population=3)

        result, _run_trial = self.collect(trials)

        self.assertEqual(result["status"], "invalid")
        self.assertEqual(result["failures"][0]["kind"], "population_mismatch")
        self.assertEqual(result["failures"][0]["requested_population"], 4)
        self.assertEqual(result["failures"][0]["observed_population"], 3)

    def test_attribution_waits_for_the_requested_population_to_be_logged(self) -> None:
        wait_for_population = getattr(
            collector, "wait_for_population_attribution", None
        )
        self.assertIsNotNone(wait_for_population)
        with (
            mock.patch.object(
                collector,
                "_runtime_memory_attribution",
                side_effect=[(3, 12.0), (4, 16.0)],
            ) as attribution,
            mock.patch.object(collector.time, "sleep") as sleep,
        ):
            observed = wait_for_population(
                Path("/tmp/private-home"),
                expected_population=4,
                timeout_s=1.0,
                poll_interval_s=0.01,
            )

        self.assertEqual(observed, (4, 16.0))
        self.assertEqual(attribution.call_count, 2)
        attribution.assert_has_calls(
            [
                mock.call(Path("/tmp/private-home"), expected_population=4),
                mock.call(Path("/tmp/private-home"), expected_population=4),
            ]
        )
        sleep.assert_called_once_with(0.01)


class OwnedCleanupTests(unittest.TestCase):
    def test_failed_collection_terminates_only_launched_process_groups(self) -> None:
        spec = collector.ToolSpec(
            name="example",
            argv=["example"],
            version_argv=["example", "--version"],
        )
        launches = [
            collector.SessionLaunch(101, 2001, 31, True, True, None, 0.1, 0.2, None),
            collector.SessionLaunch(102, 2002, 32, True, True, None, 0.1, 0.2, None),
        ]

        with (
            mock.patch.object(collector, "version_for", return_value="example 1"),
            mock.patch.object(collector, "launch_interactive", side_effect=launches),
            mock.patch.object(
                collector,
                "sum_tree_pss",
                side_effect=RuntimeError("sample failed"),
            ),
            mock.patch.object(collector, "terminate_pgroup") as terminate,
            mock.patch.object(collector.os, "close"),
        ):
            with self.assertRaisesRegex(RuntimeError, "sample failed"):
                collector.run_tool(spec, 2, Path.cwd(), 5.0, 1.0)

        self.assertEqual(
            terminate.call_args_list,
            [mock.call(2002), mock.call(2001)],
        )


if __name__ == "__main__":
    unittest.main()
