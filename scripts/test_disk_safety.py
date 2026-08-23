#!/usr/bin/env python3
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


SCRIPT = Path(__file__).with_name("disk_safety.py")
SPEC = importlib.util.spec_from_file_location("disk_safety", SCRIPT)
assert SPEC and SPEC.loader
DISK_SAFETY = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = DISK_SAFETY
SPEC.loader.exec_module(DISK_SAFETY)


class DiskSafetyTests(unittest.TestCase):
    def test_threshold_boundary_is_inclusive(self):
        threshold = 100
        self.assertFalse(DISK_SAFETY.preflight_ok(99, threshold))
        self.assertTrue(DISK_SAFETY.preflight_ok(100, threshold))
        self.assertTrue(DISK_SAFETY.preflight_ok(101, threshold))

    def test_invalid_configuration_is_rejected(self):
        for raw in ("", "-1", "1.5", "bytes"):
            with self.subTest(raw=raw):
                with self.assertRaises(DISK_SAFETY.DiskSafetyError):
                    DISK_SAFETY.parse_non_negative_int(raw, "threshold")
        with self.assertRaises(DISK_SAFETY.DiskSafetyError):
            DISK_SAFETY.configured_value(-1, "JCODE_DISK_MIN_FREE_BYTES", 100)
        with self.assertRaises(DISK_SAFETY.DiskSafetyError):
            DISK_SAFETY.configured_value(0, "JCODE_DISK_MAX_WORKTREES", 25, DISK_SAFETY.parse_positive_int)

    def test_configuration_precedence_is_cli_then_environment_then_default(self):
        with patch.dict(os.environ, {"JCODE_DISK_MIN_FREE_BYTES": "200"}, clear=False):
            self.assertEqual(
                DISK_SAFETY.configured_value(None, "JCODE_DISK_MIN_FREE_BYTES", 100), 200
            )
            self.assertEqual(
                DISK_SAFETY.configured_value(300, "JCODE_DISK_MIN_FREE_BYTES", 100), 300
            )
        with patch.dict(os.environ, {}, clear=False):
            os.environ.pop("JCODE_DISK_MIN_FREE_BYTES", None)
            self.assertEqual(
                DISK_SAFETY.configured_value(None, "JCODE_DISK_MIN_FREE_BYTES", 100), 100
            )

    def test_target_must_be_contained_direct_non_symlink_directory(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp) / "repo"
            worktree = repo / ".worktrees" / "agent"
            target = worktree / "target"
            target.mkdir(parents=True)
            self.assertTrue(DISK_SAFETY.safe_target_path(repo, worktree, target)[0])

            outside = Path(temp) / "outside-target"
            outside.mkdir()
            self.assertFalse(DISK_SAFETY.safe_target_path(repo, worktree, outside)[0])

            target.rmdir()
            target.symlink_to(outside, target_is_directory=True)
            self.assertFalse(DISK_SAFETY.safe_target_path(repo, worktree, target)[0])

    def test_cleanup_excludes_main_active_dirty_and_recent_worktrees(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp) / "repo"
            repo.mkdir()
            now = 1_000_000.0
            old = now - (8 * 24 * 60 * 60)
            snapshots = []
            for name, flags in (
                ("main", {"is_main": True}),
                ("active", {"active": True}),
                ("dirty", {"dirty": True}),
                ("recent", {"target_mtime": now - 60}),
                ("old", {"target_mtime": old}),
            ):
                worktree = repo / (Path(".worktrees") / name if name != "main" else "main")
                target = worktree / "target"
                target.mkdir(parents=True)
                (target / "artifact").write_bytes(b"artifact")
                snapshots.append(
                    DISK_SAFETY.WorktreeSnapshot(
                        path=worktree,
                        target=target,
                        target_bytes=8,
                        target_mtime=flags.pop("target_mtime", old),
                        **flags,
                    )
                )

            candidates = DISK_SAFETY.select_cleanup_candidates(
                snapshots, repo, now=now, min_age_seconds=7 * 24 * 60 * 60
            )
            self.assertEqual([candidate.worktree_path.name for candidate in candidates], ["old"])

    def test_cleanup_dry_run_preserves_target_and_apply_reports_bytes(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp) / "repo"
            worktree = repo / ".worktrees" / "old"
            target = worktree / "target"
            target.mkdir(parents=True)
            artifact = target / "artifact"
            artifact.write_bytes(b"artifact-bytes")
            expected_bytes = DISK_SAFETY.directory_usage(target)[0]
            candidate = DISK_SAFETY.CleanupCandidate(
                repo, worktree, target, expected_bytes
            )

            safety_state = (
                patch.object(
                    DISK_SAFETY,
                    "discover_worktrees",
                    return_value=(repo, [repo, worktree]),
                ),
                patch.object(
                    DISK_SAFETY, "active_session_paths", return_value=(set(), [])
                ),
                patch.object(DISK_SAFETY, "worktree_is_active", return_value=False),
                patch.object(
                    DISK_SAFETY, "worktree_is_dirty", return_value=(False, False)
                ),
            )
            with safety_state[0], safety_state[1], safety_state[2], safety_state[3]:
                self.assertEqual(
                    DISK_SAFETY.remove_candidates(
                        [candidate], apply=False, min_age_seconds=0
                    ),
                    expected_bytes,
                )
                self.assertTrue(target.exists())
                self.assertEqual(
                    DISK_SAFETY.remove_candidates(
                        [candidate], apply=True, min_age_seconds=0
                    ),
                    expected_bytes,
                )
            self.assertFalse(target.exists())

    def test_cleanup_apply_revalidates_dirty_active_and_recent_state(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp) / "repo"
            worktree = repo / ".worktrees" / "candidate"
            target = worktree / "target"
            target.mkdir(parents=True)
            artifact = target / "artifact"
            artifact.write_bytes(b"artifact-bytes")
            candidate = DISK_SAFETY.CleanupCandidate(
                repo, worktree, target, artifact.stat().st_size
            )

            cases = (
                ("dirty", patch.object(DISK_SAFETY, "worktree_is_dirty", return_value=(True, False))),
                ("active", patch.object(DISK_SAFETY, "worktree_is_active", return_value=True)),
                (
                    "recent",
                    patch.object(
                        DISK_SAFETY,
                        "directory_usage",
                        return_value=(artifact.stat().st_size, 999.0),
                    ),
                ),
            )
            for name, state_patch in cases:
                with self.subTest(name=name):
                    with patch.object(
                        DISK_SAFETY,
                        "active_session_paths",
                        return_value=(set(), []),
                    ), patch.object(
                        DISK_SAFETY,
                        "discover_worktrees",
                        return_value=(repo, [repo, worktree]),
                    ), patch.object(
                        DISK_SAFETY, "worktree_is_active", return_value=False
                    ), patch.object(
                        DISK_SAFETY,
                        "worktree_is_dirty",
                        return_value=(False, False),
                    ), state_patch:
                        with self.assertRaises(DISK_SAFETY.DiskSafetyError):
                            DISK_SAFETY.remove_candidates(
                                [candidate],
                                apply=True,
                                min_age_seconds=100,
                                now=1_000.0,
                            )
                    self.assertTrue(target.exists())

    def test_cleanup_apply_refuses_an_unregistered_worktree(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp) / "repo"
            worktree = repo / ".worktrees" / "unregistered"
            target = worktree / "target"
            target.mkdir(parents=True)
            artifact = target / "artifact"
            artifact.write_bytes(b"artifact")
            candidate = DISK_SAFETY.CleanupCandidate(repo, worktree, target, 8)

            with patch.object(
                DISK_SAFETY, "discover_worktrees", return_value=(repo, [repo])
            ):
                with self.assertRaises(DISK_SAFETY.DiskSafetyError):
                    DISK_SAFETY.remove_candidates(
                        [candidate], apply=True, min_age_seconds=0
                    )
            self.assertTrue(target.exists())

    def test_revalidation_rechecks_activity_after_the_usage_scan(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp) / "repo"
            worktree = repo / ".worktrees" / "candidate"
            target = worktree / "target"
            target.mkdir(parents=True)
            (target / "artifact").write_bytes(b"artifact")
            candidate = DISK_SAFETY.CleanupCandidate(repo, worktree, target, 0)

            with patch.object(
                DISK_SAFETY,
                "discover_worktrees",
                return_value=(repo, [repo, worktree]),
            ), patch.object(
                DISK_SAFETY, "active_session_paths", return_value=(set(), [])
            ), patch.object(
                DISK_SAFETY,
                "worktree_is_active",
                side_effect=(False, True),
            ), patch.object(
                DISK_SAFETY, "worktree_is_dirty", return_value=(False, False)
            ):
                with self.assertRaises(DISK_SAFETY.DiskSafetyError):
                    DISK_SAFETY._revalidate_candidate(
                        candidate, now=1_000.0, min_age_seconds=0
                    )
            self.assertTrue(target.exists())

    def test_target_age_uses_newest_nested_artifact(self):
        with tempfile.TemporaryDirectory() as temp:
            target = Path(temp) / "target"
            nested = target / "debug" / "deps"
            nested.mkdir(parents=True)
            artifact = nested / "recent.rlib"
            artifact.write_bytes(b"artifact")
            os.utime(target, (100.0, 100.0))
            os.utime(nested.parent, (100.0, 100.0))
            os.utime(nested, (100.0, 100.0))
            os.utime(artifact, (900.0, 900.0))

            size, newest_mtime = DISK_SAFETY.directory_usage(target)
            self.assertGreater(size, 0)
            self.assertEqual(newest_mtime, 900.0)

    def test_live_session_with_unreadable_metadata_fails_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            home = Path(temp)
            active_dir = home / "active_pids"
            active_dir.mkdir()
            (active_dir / "session-1").write_text(str(os.getpid()))

            paths, warnings = DISK_SAFETY.active_session_paths(home)
            self.assertEqual(paths, set())
            self.assertTrue(warnings)

    def test_live_session_path_is_discovered_from_persisted_metadata(self):
        with tempfile.TemporaryDirectory() as temp:
            home = Path(temp)
            worktree = home / "repo" / ".worktrees" / "active"
            active_dir = home / "active_pids"
            sessions_dir = home / "sessions"
            active_dir.mkdir()
            sessions_dir.mkdir()
            (active_dir / "session-1").write_text(str(os.getpid()))
            (sessions_dir / "session-1.json").write_text(
                json.dumps({"status": "Active", "working_dir": str(worktree)})
            )

            paths, warnings = DISK_SAFETY.active_session_paths(home)
            self.assertEqual(paths, {worktree})
            self.assertEqual(warnings, [])

    def test_make_disk_help_and_dry_run_commands_are_exposed(self):
        makefile = Path(__file__).parents[1] / "Makefile"
        text = makefile.read_text()
        for target in ("disk-report", "disk-check", "disk-clean", "disk-clean-apply", "disk-help"):
            self.assertIn(target, text)

    def test_guardrails_low_space_exits_before_invoking_cargo(self):
        with tempfile.TemporaryDirectory() as temp:
            fake_bin = Path(temp) / "bin"
            fake_bin.mkdir()
            cargo_marker = Path(temp) / "cargo-invoked"
            fake_cargo = fake_bin / "cargo"
            fake_cargo.write_text(
                f"#!/usr/bin/env bash\ntouch {cargo_marker}\nexit 99\n"
            )
            fake_cargo.chmod(0o755)
            env = os.environ.copy()
            env["PATH"] = str(fake_bin) + os.pathsep + env["PATH"]
            env["JCODE_DISK_MIN_FREE_BYTES"] = str(10**30)
            guardrails = Path(__file__).with_name("check_guardrails.sh")

            result = subprocess.run(
                ["bash", str(guardrails), "--skip-slow"],
                cwd=guardrails.parents[1],
                env=env,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=30,
                check=False,
            )

            self.assertEqual(result.returncode, 1)
            self.assertFalse(cargo_marker.exists())
            self.assertIn("Guardrails stopped before Cargo", result.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
