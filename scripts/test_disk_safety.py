#!/usr/bin/env python3
import importlib.util
import os
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
            candidate = DISK_SAFETY.CleanupCandidate(repo, worktree, target, artifact.stat().st_size)

            self.assertEqual(DISK_SAFETY.remove_candidates([candidate], apply=False), len(b"artifact-bytes"))
            self.assertTrue(target.exists())
            self.assertEqual(DISK_SAFETY.remove_candidates([candidate], apply=True), len(b"artifact-bytes"))
            self.assertFalse(target.exists())

    def test_make_disk_help_and_dry_run_commands_are_exposed(self):
        makefile = Path(__file__).parents[1] / "Makefile"
        text = makefile.read_text()
        for target in ("disk-report", "disk-check", "disk-clean", "disk-clean-apply", "disk-help"):
            self.assertIn(target, text)

    def test_guardrails_runs_disk_preflight_before_cargo(self):
        guardrails = Path(__file__).with_name("check_guardrails.sh").read_text()
        preflight = guardrails.index("disk_safety.py check")
        first_cargo = guardrails.index("cargo fmt")
        self.assertLess(preflight, first_cargo)


if __name__ == "__main__":
    unittest.main(verbosity=2)
