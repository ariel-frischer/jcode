#!/usr/bin/env python3
"""Report and safely reclaim Jcode worktree Cargo target directories."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Sequence


DEFAULT_MIN_FREE_BYTES = 10 * 1024**3
DEFAULT_CLEAN_MIN_AGE_DAYS = 7
DEFAULT_MAX_WORKTREES = 25
BUILD_PROCESS_NAMES = {"cargo", "rustc", "rust-analyzer", "jcode"}


class DiskSafetyError(ValueError):
    """A disk-safety input or filesystem invariant could not be trusted."""


@dataclass(frozen=True)
class WorktreeSnapshot:
    path: Path
    target: Path
    is_main: bool = False
    active: bool = False
    dirty: bool = False
    target_bytes: int = 0
    target_mtime: float | None = None
    status_error: bool = False


@dataclass(frozen=True)
class CleanupCandidate:
    repo_root: Path
    worktree_path: Path
    target_path: Path
    target_bytes: int


def parse_non_negative_int(raw: str, name: str) -> int:
    value = raw.strip()
    if not value or not value.isdigit():
        raise DiskSafetyError(f"{name} must be a non-negative integer, got {raw!r}")
    return int(value)


def parse_positive_int(raw: str, name: str) -> int:
    value = parse_non_negative_int(raw, name)
    if value == 0:
        raise DiskSafetyError(f"{name} must be greater than zero")
    return value


def configured_value(
    cli_value: int | None,
    env_name: str,
    default: int,
    parser=parse_non_negative_int,
) -> int:
    if cli_value is not None:
        return parser(str(cli_value), env_name)
    raw = os.environ.get(env_name)
    if raw is None:
        return default
    return parser(raw, env_name)


def preflight_ok(free_bytes: int, min_free_bytes: int) -> bool:
    return free_bytes >= min_free_bytes


def human_bytes(value: int) -> str:
    units = ("B", "KiB", "MiB", "GiB", "TiB")
    amount = float(max(value, 0))
    for unit in units:
        if amount < 1024 or unit == units[-1]:
            return f"{amount:.1f}{unit}" if unit != "B" else f"{int(amount)}B"
        amount /= 1024
    return f"{int(value)}B"


def path_within(path: Path, parent: Path) -> bool:
    try:
        path.resolve(strict=False).relative_to(parent.resolve(strict=False))
    except (OSError, ValueError):
        return False
    return True


def safe_target_path(repo_root: Path, worktree_path: Path, target_path: Path) -> tuple[bool, str]:
    """Validate that target_path is the direct, non-symlink target of a repo worktree."""
    try:
        repo_root = repo_root.resolve(strict=True)
        worktree_path = worktree_path.resolve(strict=True)
    except OSError as exc:
        return False, f"unresolvable worktree: {exc}"

    if not path_within(worktree_path, repo_root):
        return False, "worktree is outside repository"
    expected = worktree_path / "target"
    try:
        target_path = target_path.absolute()
    except OSError as exc:
        return False, f"unresolvable target: {exc}"
    if target_path != expected:
        return False, "path is not the direct worktree target directory"
    try:
        if target_path.is_symlink():
            return False, "target is a symlink"
        if not target_path.is_dir():
            return False, "target directory is missing"
        resolved_target = target_path.resolve(strict=True)
    except OSError as exc:
        return False, f"target cannot be inspected: {exc}"
    if not path_within(resolved_target, repo_root):
        return False, "target resolves outside repository"
    return True, "safe"


def directory_bytes(path: Path) -> int:
    total = 0
    for root, dirs, files in os.walk(path, topdown=True, followlinks=False):
        root_path = Path(root)
        dirs[:] = [name for name in dirs if not (root_path / name).is_symlink()]
        for name in files:
            file_path = root_path / name
            try:
                if not file_path.is_symlink():
                    total += file_path.stat().st_size
            except OSError:
                continue
    return total


def _git(args: Sequence[str], cwd: Path, *, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", *args],
        cwd=cwd,
        check=check,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def parse_worktree_list(output: str) -> list[Path]:
    paths: list[Path] = []
    for line in output.splitlines():
        if line.startswith("worktree "):
            paths.append(Path(line.removeprefix("worktree ")))
    return paths


def discover_worktrees(start: Path) -> tuple[Path, list[Path]]:
    current = Path(
        _git(["rev-parse", "--show-toplevel"], start).stdout.strip()
    ).resolve(strict=True)
    listed = parse_worktree_list(_git(["worktree", "list", "--porcelain"], current).stdout)
    if not listed:
        listed = [current]
    listed = [path.resolve(strict=False) for path in listed]
    return listed[0], listed


def _pid_is_alive(pid: int) -> bool:
    if pid <= 0:
        return False
    try:
        os.kill(pid, 0)
    except (OSError, ProcessLookupError):
        return False
    return True


def active_session_paths() -> set[Path]:
    home = Path(os.environ.get("JCODE_HOME", Path.home() / ".jcode"))
    active_dir = home / "active_pids"
    sessions_dir = home / "sessions"
    active: set[Path] = set()
    if not active_dir.is_dir():
        return active
    for marker in active_dir.iterdir():
        if not marker.is_file():
            continue
        try:
            pid = parse_positive_int(marker.read_text(), "active session pid")
            session_path = sessions_dir / f"{marker.name}.json"
            data = json.loads(session_path.read_text())
            status = str(data.get("status", "")).lower()
            if status == "active" or _pid_is_alive(pid):
                working_dir = data.get("working_dir")
                if working_dir:
                    active.add(Path(working_dir).resolve(strict=False))
        except (OSError, ValueError, json.JSONDecodeError, TypeError):
            # A stale or malformed activity marker cannot authorize deletion.
            continue
    return active


def active_process_for(path: Path) -> bool:
    """Detect build/Jcode processes whose cwd or command line names this worktree."""
    proc_root = Path("/proc")
    if not proc_root.is_dir():
        return False
    path = path.resolve(strict=False)
    own_pid = os.getpid()
    for entry in proc_root.iterdir():
        if not entry.name.isdigit() or int(entry.name) == own_pid:
            continue
        try:
            comm = (entry / "comm").read_text().strip()
            if comm not in BUILD_PROCESS_NAMES:
                continue
            cwd = (entry / "cwd").resolve(strict=False)
            if path_within(cwd, path):
                return True
            command = (entry / "cmdline").read_bytes().replace(b"\0", b" ").decode(
                errors="replace"
            )
            if str(path) in command:
                return True
        except (OSError, PermissionError):
            continue
    return False


def _configured_active_paths() -> set[Path]:
    raw = os.environ.get("JCODE_DISK_SAFETY_ACTIVE_PATHS", "")
    return {Path(value).resolve(strict=False) for value in raw.split(os.pathsep) if value}


def worktree_is_active(path: Path, active_paths: set[Path]) -> bool:
    if any(path_within(active_path, path) for active_path in active_paths):
        return True
    return active_process_for(path)


def worktree_is_dirty(path: Path) -> tuple[bool, bool]:
    try:
        result = _git(["status", "--porcelain=v1", "--untracked-files=normal"], path, check=False)
    except OSError:
        return True, True
    if result.returncode != 0:
        return True, True
    return bool(result.stdout.strip()), False


def inspect_worktrees(repo_root: Path, paths: Iterable[Path]) -> list[WorktreeSnapshot]:
    active_paths = active_session_paths() | _configured_active_paths()
    snapshots: list[WorktreeSnapshot] = []
    for path in paths:
        target = path / "target"
        safe, _ = safe_target_path(repo_root, path, target)
        target_bytes = directory_bytes(target) if safe else 0
        target_mtime: float | None = None
        if safe:
            try:
                target_mtime = target.stat().st_mtime
            except OSError:
                safe = False
        dirty, status_error = worktree_is_dirty(path) if path.is_dir() else (True, True)
        snapshots.append(
            WorktreeSnapshot(
                path=path,
                target=target,
                is_main=path.resolve(strict=False) == repo_root.resolve(strict=False),
                active=worktree_is_active(path, active_paths) if path.is_dir() else False,
                dirty=dirty,
                target_bytes=target_bytes,
                target_mtime=target_mtime,
                status_error=status_error,
            )
        )
    return snapshots


def select_cleanup_candidates(
    snapshots: Iterable[WorktreeSnapshot],
    repo_root: Path,
    *,
    now: float | None = None,
    min_age_seconds: int,
) -> list[CleanupCandidate]:
    now = time.time() if now is None else now
    candidates: list[CleanupCandidate] = []
    for snapshot in snapshots:
        if snapshot.is_main or snapshot.active or snapshot.dirty or snapshot.status_error:
            continue
        if snapshot.target_mtime is None or now - snapshot.target_mtime < min_age_seconds:
            continue
        safe, _ = safe_target_path(repo_root, snapshot.path, snapshot.target)
        if not safe:
            continue
        candidates.append(
            CleanupCandidate(repo_root, snapshot.path, snapshot.target, snapshot.target_bytes)
        )
    return candidates


def remove_candidates(candidates: Iterable[CleanupCandidate], *, apply: bool) -> int:
    prepared: list[tuple[CleanupCandidate, int]] = []
    for candidate in candidates:
        safe, reason = safe_target_path(
            candidate.repo_root, candidate.worktree_path, candidate.target_path
        )
        if not safe:
            raise DiskSafetyError(
                f"refusing to remove {candidate.target_path}: {reason}"
            )
        bytes_before = directory_bytes(candidate.target_path)
        prepared.append((candidate, bytes_before))

    reclaimed = 0
    for candidate, bytes_before in prepared:
        if apply:
            shutil.rmtree(candidate.target_path)
        reclaimed += bytes_before
    return reclaimed


def filesystem_free_bytes(path: Path) -> int:
    usage = os.statvfs(path)
    return usage.f_bavail * usage.f_frsize


def _format_preflight(free_bytes: int, min_free_bytes: int, path: Path) -> str:
    state = "ok" if preflight_ok(free_bytes, min_free_bytes) else "blocked"
    return (
        f"disk preflight: {state}; path={path}; free={human_bytes(free_bytes)}; "
        f"required reserve={human_bytes(min_free_bytes)}"
    )


def run_check(min_free_bytes: int, path: Path) -> int:
    try:
        free_bytes = filesystem_free_bytes(path)
    except OSError as exc:
        print(f"disk preflight failed: cannot inspect filesystem at {path}: {exc}", file=sys.stderr)
        return 1
    message = _format_preflight(free_bytes, min_free_bytes, path)
    if preflight_ok(free_bytes, min_free_bytes):
        print(message)
        return 0
    print(message, file=sys.stderr)
    print(
        "disk preflight blocked Cargo work: run `make disk-report`, review "
        "`make disk-clean`, then explicitly apply `make disk-clean-apply` if safe; "
        "configure JCODE_DISK_MIN_FREE_BYTES only for an intentional reserve change.",
        file=sys.stderr,
    )
    return 1


def run_report(start: Path, max_worktrees: int) -> int:
    try:
        repo_root, all_paths = discover_worktrees(start)
        snapshots = inspect_worktrees(repo_root, all_paths[:max_worktrees])
        free_bytes = filesystem_free_bytes(repo_root)
    except (OSError, subprocess.CalledProcessError, DiskSafetyError) as exc:
        print(f"disk report failed: {exc}", file=sys.stderr)
        return 1

    print(f"repository: {repo_root}")
    print(f"filesystem free: {human_bytes(free_bytes)}")
    shown = len(snapshots)
    suffix = f"; {len(all_paths) - shown} omitted" if shown < len(all_paths) else ""
    print(f"worktrees: {len(all_paths)} listed, {shown} shown{suffix}")
    for snapshot in snapshots:
        if snapshot.target_mtime is None:
            target = "missing or unsafe"
        else:
            target = human_bytes(snapshot.target_bytes)
        flags = []
        if snapshot.is_main:
            flags.append("main")
        if snapshot.active:
            flags.append("active")
        if snapshot.dirty:
            flags.append("dirty")
        print(f"  {snapshot.path}: target={target}" + (f" ({', '.join(flags)})" if flags else ""))
    return 0


def run_clean(start: Path, max_worktrees: int, min_age_days: int, apply: bool) -> int:
    try:
        repo_root, all_paths = discover_worktrees(start)
        shown_paths = all_paths[:max_worktrees]
        snapshots = inspect_worktrees(repo_root, shown_paths)
        candidates = select_cleanup_candidates(
            snapshots,
            repo_root,
            min_age_seconds=min_age_days * 24 * 60 * 60,
        )
        reclaimed = remove_candidates(candidates, apply=apply)
    except (OSError, subprocess.CalledProcessError, DiskSafetyError) as exc:
        print(f"disk cleanup refused: {exc}", file=sys.stderr)
        return 1

    mode = "apply" if apply else "dry-run"
    print(f"disk cleanup: {mode}; minimum target age={min_age_days}d")
    for candidate in candidates:
        verb = "removed" if apply else "would remove"
        print(f"  {verb}: {candidate.target_path} ({human_bytes(candidate.target_bytes)})")
    if len(all_paths) > len(shown_paths):
        print(f"  not inspected: {len(all_paths) - len(shown_paths)} worktrees beyond report bound")
    label = "reclaimed" if apply else "reclaimable"
    print(f"total {label}: {human_bytes(reclaimed)}")
    if not apply:
        print("dry-run only; use `make disk-clean-apply` after reviewing the list")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Report disk headroom and safely clean stale Jcode worktree target directories."
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    check = subparsers.add_parser("check", help="fail if filesystem reserve is below threshold")
    check.add_argument("--min-free-bytes", type=int, help="required free bytes")
    check.add_argument("--path", type=Path, default=Path.cwd(), help="filesystem path to inspect")

    report = subparsers.add_parser("report", help="show bounded filesystem and target usage report")
    report.add_argument("--max-worktrees", type=int, help="maximum worktrees to inspect")
    report.add_argument("--path", type=Path, default=Path.cwd(), help="repository worktree")

    clean = subparsers.add_parser("clean", help="dry-run stale target cleanup unless --apply is given")
    clean.add_argument("--apply", action="store_true", help="actually remove selected target directories")
    clean.add_argument("--min-age-days", type=int, help="minimum target age for cleanup")
    clean.add_argument("--max-worktrees", type=int, help="maximum worktrees to inspect")
    clean.add_argument("--path", type=Path, default=Path.cwd(), help="repository worktree")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        if args.command == "check":
            minimum = configured_value(
                args.min_free_bytes,
                "JCODE_DISK_MIN_FREE_BYTES",
                DEFAULT_MIN_FREE_BYTES,
            )
            return run_check(minimum, args.path.resolve())
        maximum = configured_value(
            args.max_worktrees,
            "JCODE_DISK_MAX_WORKTREES",
            DEFAULT_MAX_WORKTREES,
            parse_positive_int,
        )
        if args.command == "report":
            return run_report(args.path.resolve(), maximum)
        min_age_days = configured_value(
            args.min_age_days,
            "JCODE_DISK_CLEAN_MIN_AGE_DAYS",
            DEFAULT_CLEAN_MIN_AGE_DAYS,
            parse_non_negative_int,
        )
        return run_clean(args.path.resolve(), maximum, min_age_days, args.apply)
    except (DiskSafetyError, OSError) as exc:
        print(f"disk safety configuration error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
