#!/usr/bin/env python3
import importlib.util
import io
import inspect
import json
import os
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from types import SimpleNamespace
from contextlib import redirect_stdout
from unittest.mock import patch


SCRIPT = Path(__file__).with_name("disk_safety.py")
SPEC = importlib.util.spec_from_file_location("disk_safety", SCRIPT)
assert SPEC and SPEC.loader
DISK_SAFETY = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = DISK_SAFETY
SPEC.loader.exec_module(DISK_SAFETY)


class GitWorktreeFixture:
    """Disposable registered worktrees and safety metadata for apply tests."""

    def __init__(self):
        self._temp = tempfile.TemporaryDirectory()
        self.root = Path(self._temp.name)
        self.main = self.root / "repo"
        self.jcode_home = self.root / "jcode-home"
        self.external_sentinel = self.root / "outside-sentinel"
        self._git("init", "-b", "main", str(self.main), cwd=self.root)
        self._git("config", "user.email", "disk-safety@example.invalid")
        self._git("config", "user.name", "Disk Safety Test")
        (self.main / "source.txt").write_text("source")
        (self.main / ".gitignore").write_text("target/\n")
        self._git("add", "source.txt", ".gitignore")
        self._git("commit", "-m", "fixture")
        self.external_sentinel.write_text("outside")

    def close(self):
        self._temp.cleanup()

    def _git(self, *args: str, cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["git", *args],
            cwd=cwd or self.main,
            check=True,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

    def add_worktree(self, name: str) -> Path:
        path = self.main / ".worktrees" / name
        self._git("worktree", "add", "-b", f"fixture/{name}", str(path))
        return path

    def add_target(self, worktree: Path, *, age_days: int = 8) -> Path:
        target = worktree / "target"
        target.mkdir()
        artifact = target / "artifact"
        artifact.write_bytes(b"fixture-artifact")
        timestamp = time.time() - age_days * 24 * 60 * 60
        os.utime(artifact, (timestamp, timestamp))
        os.utime(target, (timestamp, timestamp))
        return target

    def make_dirty(self, worktree: Path, *, untracked: bool = True):
        path = worktree / ("untracked.txt" if untracked else "source.txt")
        path.write_text("dirty")

    def add_live_session(self, worktree: Path, session_id: str = "fixture-session"):
        active = self.jcode_home / "active_pids"
        sessions = self.jcode_home / "sessions"
        active.mkdir(parents=True, exist_ok=True)
        sessions.mkdir(parents=True, exist_ok=True)
        (active / session_id).write_text(str(os.getpid()))
        (sessions / f"{session_id}.json").write_text(
            json.dumps({"status": "Active", "working_dir": str(worktree)})
        )

    @staticmethod
    def statvfs_with_free_bytes(free_bytes: int):
        return SimpleNamespace(f_bavail=free_bytes, f_frsize=1)

    def assert_protected_paths_remain(self, worktree: Path):
        self.assert_path(self.main / "source.txt", "main source")
        self.assert_path(worktree / "source.txt", "worktree source")
        self.assert_path(worktree, "worktree root")
        self.assert_path(self.external_sentinel, "external sentinel")
        registrations = self._git("worktree", "list", "--porcelain").stdout
        if str(worktree) not in registrations:
            raise AssertionError("worktree registration was removed")

    @staticmethod
    def assert_path(path: Path, label: str):
        if not path.exists():
            raise AssertionError(f"{label} was removed: {path}")


class DiskSafetyTests(unittest.TestCase):
    def test_fixture_creates_registered_disposable_worktrees_and_safety_metadata(self):
        fixture = GitWorktreeFixture()
        self.addCleanup(fixture.close)
        worktree = fixture.add_worktree("fixture")
        target = fixture.add_target(worktree)
        fixture.make_dirty(worktree)
        fixture.add_live_session(worktree)

        repo_root, registered = DISK_SAFETY.discover_worktrees(fixture.main)

        self.assertEqual(repo_root, fixture.main)
        self.assertIn(worktree, registered)
        self.assertTrue(target.is_dir())
        self.assertEqual(fixture.statvfs_with_free_bytes(123).f_bavail, 123)
        fixture.assert_protected_paths_remain(worktree)

    def test_threshold_boundary_is_inclusive(self):
        threshold = 100
        self.assertFalse(DISK_SAFETY.preflight_ok(99, threshold))
        self.assertFalse(DISK_SAFETY.preflight_ok(100, threshold))
        self.assertTrue(DISK_SAFETY.preflight_ok(101, threshold))

    def test_check_rejects_malformed_environment_with_actionable_exit_2(self):
        env = os.environ.copy()
        env["JCODE_DISK_MIN_FREE_BYTES"] = "10GiB"

        result = subprocess.run(
            [sys.executable, str(SCRIPT), "check"],
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )

        self.assertEqual(result.returncode, 2)
        self.assertIn("JCODE_DISK_MIN_FREE_BYTES", result.stderr)
        self.assertIn("decimal integer", result.stderr)

    def test_cli_preserves_lexical_configuration_for_strict_validation(self):
        parser = DISK_SAFETY.build_parser()
        for argv, attribute in (
            (["check", "--min-free-bytes", "+1"], "min_free_bytes"),
            (["clean", "--min-age-days", "+1"], "min_age_days"),
        ):
            with self.subTest(argv=argv):
                args = parser.parse_args(argv)
                self.assertEqual(getattr(args, attribute), "+1")
                with self.assertRaises(DISK_SAFETY.DiskSafetyError):
                    DISK_SAFETY.configured_value(
                        getattr(args, attribute), "strict test setting", 0
                    )

    def test_check_reports_effective_reserve_value(self):
        result = subprocess.run(
            [sys.executable, str(SCRIPT), "check", "--min-free-bytes", "0"],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("effective reserve bytes=0", result.stdout)

    def test_guardrails_malformed_reserve_exits_before_invoking_cargo(self):
        with tempfile.TemporaryDirectory() as temp:
            fake_bin = Path(temp) / "bin"
            fake_bin.mkdir()
            cargo_marker = Path(temp) / "cargo-invoked"
            fake_cargo = fake_bin / "cargo"
            fake_cargo.write_text(f"#!/usr/bin/env bash\ntouch {cargo_marker}\nexit 99\n")
            fake_cargo.chmod(0o755)
            env = os.environ.copy()
            env["PATH"] = str(fake_bin) + os.pathsep + env["PATH"]
            env["JCODE_DISK_MIN_FREE_BYTES"] = "invalid"
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

            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(cargo_marker.exists())
            self.assertIn("JCODE_DISK_MIN_FREE_BYTES", result.stderr)

    def test_invalid_configuration_is_rejected(self):
        for raw in ("", "-1", "1.5", "10GiB", " 1", "1 ", "bytes"):
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

    def test_target_observation_deduplicates_inodes_and_ignores_symlinks(self):
        with tempfile.TemporaryDirectory() as temp:
            target = Path(temp) / "target"
            target.mkdir()
            artifact = target / "artifact"
            artifact.write_bytes(b"allocated")
            os.link(artifact, target / "hard-link")
            outside = Path(temp) / "outside"
            outside.write_bytes(b"outside")
            (target / "outside-link").symlink_to(outside)

            allocated, newest = DISK_SAFETY.directory_usage(target)
            target_blocks = target.stat().st_blocks * 512
            artifact_blocks = artifact.stat().st_blocks * 512

            self.assertEqual(allocated, target_blocks + artifact_blocks)
            self.assertEqual(newest, artifact.stat().st_mtime)

    def test_empty_target_has_unknown_artifact_age(self):
        with tempfile.TemporaryDirectory() as temp:
            target = Path(temp) / "target"
            target.mkdir()

            allocated, newest = DISK_SAFETY.directory_usage(target)

            self.assertGreaterEqual(allocated, 0)
            self.assertIsNone(newest)

    def test_report_has_one_estimate_record_for_every_registered_worktree(self):
        self.assertEqual(list(inspect.signature(DISK_SAFETY.run_report).parameters), ["start"])
        self.assertIn("target_state", DISK_SAFETY.WorktreeSnapshot.__dataclass_fields__)
        repo = Path("/fixture/repo")
        paths = [repo / f"worktree-{index}" for index in range(30)]
        snapshots = [
            DISK_SAFETY.WorktreeSnapshot(
                path=path,
                target=path / "target",
                target_bytes=index * 4096,
                target_mtime=100.0,
                target_state="readable",
            )
            for index, path in enumerate(paths)
        ]
        output = io.StringIO()

        with patch.object(
            DISK_SAFETY, "discover_worktrees", return_value=(repo, paths)
        ), patch.object(
            DISK_SAFETY, "inspect_worktrees", return_value=(snapshots, [])
        ) as inspect_mock, patch.object(
            DISK_SAFETY, "filesystem_free_bytes", return_value=123456
        ), redirect_stdout(output):
            result = DISK_SAFETY.run_report(repo)

        self.assertEqual(result, 0)
        inspect_mock.assert_called_once_with(repo, paths)
        text = output.getvalue()
        self.assertEqual(text.count("allocated-byte estimate="), len(paths))
        self.assertNotIn("omitted", text)

    def test_report_distinguishes_absent_and_unsafe_targets(self):
        self.assertIn("target_state", DISK_SAFETY.WorktreeSnapshot.__dataclass_fields__)
        repo = Path("/fixture/repo")
        absent = repo / "absent"
        unsafe = repo / "unsafe"
        snapshots = [
            DISK_SAFETY.WorktreeSnapshot(
                path=absent,
                target=absent / "target",
                target_state="absent",
                target_reason="direct target directory is absent",
            ),
            DISK_SAFETY.WorktreeSnapshot(
                path=unsafe,
                target=unsafe / "target",
                target_state="unknown",
                target_reason="target is a symlink",
            ),
        ]
        output = io.StringIO()

        with patch.object(
            DISK_SAFETY,
            "discover_worktrees",
            return_value=(repo, [absent, unsafe]),
        ), patch.object(
            DISK_SAFETY, "inspect_worktrees", return_value=(snapshots, [])
        ), patch.object(
            DISK_SAFETY, "filesystem_free_bytes", return_value=123456
        ), redirect_stdout(output):
            result = DISK_SAFETY.run_report(repo)

        self.assertEqual(result, 0)
        self.assertIn("target=absent", output.getvalue())
        self.assertIn("target=unknown (target is a symlink)", output.getvalue())

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

    def test_cleanup_age_requires_strictly_older_newest_artifact(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp) / "repo"
            worktree = repo / ".worktrees" / "boundary"
            target = worktree / "target"
            target.mkdir(parents=True)
            snapshot = DISK_SAFETY.WorktreeSnapshot(
                path=worktree,
                target=target,
                target_bytes=4096,
                target_mtime=900.0,
                target_state="readable",
            )

            candidates = DISK_SAFETY.select_cleanup_candidates(
                [snapshot], repo, now=1_000.0, min_age_seconds=100
            )

            self.assertEqual(candidates, [])

    def test_cleanup_dry_run_reports_every_exclusion_and_selected_estimate(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp) / "repo"
            repo.mkdir()
            now = 1_000_000.0
            old = now - 800_000
            snapshots = []
            for name, values in (
                ("main", {"is_main": True}),
                ("active", {"active": True}),
                ("dirty", {"dirty": True}),
                ("recent", {"target_mtime": now - 10}),
                ("empty", {"target_mtime": None}),
                ("eligible", {"target_mtime": old, "target_bytes": 4096}),
            ):
                worktree = repo if name == "main" else repo / ".worktrees" / name
                target = worktree / "target"
                target.mkdir(parents=True, exist_ok=True)
                snapshots.append(
                    DISK_SAFETY.WorktreeSnapshot(
                        path=worktree,
                        target=target,
                        target_state="readable",
                        target_mtime=values.pop("target_mtime", old),
                        **values,
                    )
                )
            output = io.StringIO()

            with patch.object(
                DISK_SAFETY,
                "discover_worktrees",
                return_value=(repo, [snapshot.path for snapshot in snapshots]),
            ), patch.object(
                DISK_SAFETY, "inspect_worktrees", return_value=(snapshots, [])
            ), patch.object(
                DISK_SAFETY, "remove_candidates", return_value=4096
            ), patch.object(DISK_SAFETY.time, "time", return_value=now), redirect_stdout(output):
                result = DISK_SAFETY.run_clean(repo, 7, False)

            self.assertEqual(result, 0)
            text = output.getvalue()
            for expected in (
                "excluded: main checkout",
                "excluded: active worktree",
                "excluded: dirty worktree",
                "excluded: target is not strictly older",
                "excluded: artifact age is unknown",
                "eligible: allocated-byte estimate=4.0KiB",
            ):
                self.assertIn(expected, text)

    def test_cleanup_dry_run_reports_uncertain_activity_without_deleting(self):
        repo = Path("/fixture/repo")
        output = io.StringIO()
        with patch.object(
            DISK_SAFETY, "discover_worktrees", return_value=(repo, [repo])
        ), patch.object(
            DISK_SAFETY,
            "inspect_worktrees",
            return_value=(
                [
                    DISK_SAFETY.WorktreeSnapshot(
                        path=repo,
                        target=repo / "target",
                        is_main=True,
                    )
                ],
                ["cannot trust active session metadata"],
            ),
        ), patch.object(DISK_SAFETY, "remove_candidates") as remove, redirect_stdout(output):
            result = DISK_SAFETY.run_clean(repo, 7, False)

        self.assertEqual(result, 0)
        remove.assert_not_called()
        self.assertIn("uncertain activity", output.getvalue())

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

    def test_apply_uses_only_disposable_git_worktrees_and_preserves_exclusions(self):
        fixture = GitWorktreeFixture()
        self.addCleanup(fixture.close)
        eligible = fixture.add_worktree("eligible")
        dirty = fixture.add_worktree("dirty")
        eligible_target = fixture.add_target(eligible)
        dirty_target = fixture.add_target(dirty)
        fixture.make_dirty(dirty, untracked=False)
        output = io.StringIO()

        with patch.dict(
            os.environ, {"JCODE_HOME": str(fixture.jcode_home)}, clear=False
        ), redirect_stdout(output):
            result = DISK_SAFETY.run_clean(fixture.main, 0, True)

        self.assertEqual(result, 0)
        self.assertFalse(eligible_target.exists())
        self.assertTrue(dirty_target.exists())
        fixture.assert_protected_paths_remain(eligible)
        fixture.assert_protected_paths_remain(dirty)
        self.assertIn("removed: allocated-byte estimate=", output.getvalue())

    def test_batch_revalidation_failure_preserves_every_candidate(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp) / "repo"
            candidates = []
            for name in ("first", "second"):
                worktree = repo / ".worktrees" / name
                target = worktree / "target"
                target.mkdir(parents=True)
                (target / "artifact").write_bytes(b"artifact")
                candidates.append(
                    DISK_SAFETY.CleanupCandidate(repo, worktree, target, 4096)
                )

            with patch.object(
                DISK_SAFETY,
                "_revalidate_candidate",
                side_effect=(4096, DISK_SAFETY.DiskSafetyError("became unsafe")),
            ):
                with self.assertRaises(DISK_SAFETY.DiskSafetyError):
                    DISK_SAFETY.remove_candidates(
                        candidates, apply=True, min_age_seconds=0
                    )

            self.assertTrue(all(candidate.target_path.exists() for candidate in candidates))

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

    def test_relevant_process_metadata_is_active_or_fail_closed(self):
        self.assertTrue(hasattr(DISK_SAFETY, "active_process_state"))
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            worktree = root / "repo" / ".worktrees" / "active"
            worktree.mkdir(parents=True)
            proc_root = root / "proc"
            active_proc = proc_root / "101"
            active_proc.mkdir(parents=True)
            (active_proc / "comm").write_text("cargo\n")
            (active_proc / "cwd").symlink_to(worktree, target_is_directory=True)
            (active_proc / "cmdline").write_bytes(b"cargo\0check\0")

            active, warnings = DISK_SAFETY.active_process_state(
                worktree, proc_root=proc_root
            )
            self.assertTrue(active)
            self.assertEqual(warnings, [])

            (active_proc / "cwd").unlink()
            (active_proc / "cmdline").unlink()
            active, warnings = DISK_SAFETY.active_process_state(
                worktree, proc_root=proc_root
            )
            self.assertFalse(active)
            self.assertTrue(warnings)

    def test_make_disk_help_and_dry_run_commands_are_exposed(self):
        repo = Path(__file__).parents[1]
        makefile = repo / "Makefile"
        text = makefile.read_text()
        for target in ("disk-report", "disk-check", "disk-clean", "disk-clean-apply", "disk-help"):
            self.assertIn(target, text)
            result = subprocess.run(
                ["make", "-n", target],
                cwd=repo,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)

        dry_run = subprocess.run(
            ["make", "-n", "disk-clean"],
            cwd=repo,
            text=True,
            stdout=subprocess.PIPE,
            check=True,
        ).stdout
        apply_run = subprocess.run(
            ["make", "-n", "disk-clean-apply"],
            cwd=repo,
            text=True,
            stdout=subprocess.PIPE,
            check=True,
        ).stdout
        self.assertNotIn("--apply", dry_run)
        self.assertIn("clean --apply", apply_run)
        self.assertNotIn("DISK_MAX_WORKTREES", text)
        self.assertNotIn("--max-worktrees", text)

    def test_disk_safety_documentation_covers_the_accepted_contract(self):
        docs = " ".join(
            (Path(__file__).parents[1] / "docs" / "DISK_SAFETY.md")
            .read_text()
            .lower()
            .split()
        )
        for expected in (
            "make disk-report",
            "make disk-check",
            "make disk-clean",
            "make disk-clean-apply",
            "command-line option, then environment variable, then the built-in default",
            "free bytes less than or equal to the reserve",
            "strictly older",
            "non-negative decimal integer",
            "allocated-byte estimate",
            "main checkout",
            "active",
            "dirty",
            "unreadable",
            "unregistered",
            "outside the repository",
            "symlink",
            "direct",
            "cargo will regenerate",
        ):
            with self.subTest(expected=expected):
                self.assertIn(expected, docs)

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

    def test_guardrails_above_reserve_reaches_cargo_after_preflight(self):
        guardrails = Path(__file__).with_name("check_guardrails.sh").read_text()
        preflight = guardrails.index("python3 scripts/disk_safety.py check")
        first_cargo = guardrails.index("cargo fmt --all", preflight)
        self.assertLess(preflight, first_cargo)

        output = io.StringIO()
        with patch.object(
            DISK_SAFETY, "filesystem_free_bytes", return_value=101
        ), redirect_stdout(output):
            result = DISK_SAFETY.run_check(100, Path.cwd())

        self.assertEqual(result, 0)
        self.assertIn("disk preflight: ok", output.getvalue())


if __name__ == "__main__":
    unittest.main(verbosity=2)
