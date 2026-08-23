#!/usr/bin/env python3
"""Report and safely reclaim Jcode worktree Cargo target directories."""

from __future__ import annotations

import argparse
import errno
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


def directory_usage(path: Path) -> tuple[int, float]:
    """Return allocated-byte estimate and newest mtime without following symlinks."""
    total = 0
    seen: set[tuple[int, int]] = set()

    def include(stat: os.stat_result) -> None:
        nonlocal total
        identity = (stat.st_dev, stat.st_ino)
        if identity in seen:
            return
        seen.add(identity)
        blocks = getattr(stat, "st_blocks", None)
        total += blocks * 512 if blocks is not None else stat.st_size

    root_stat = path.stat()
    include(root_stat)
    newest_mtime = root_stat.st_mtime
    walk_errors: list[OSError] = []

    def record_error(exc: OSError) -> None:
        walk_errors.append(exc)

    for root, dirs, files in os.walk(
        path, topdown=True, followlinks=False, onerror=record_error
    ):
        root_path = Path(root)
        root_stat = root_path.stat()
        include(root_stat)
        newest_mtime = max(newest_mtime, root_stat.st_mtime)
        kept_dirs = []
        for name in dirs:
            directory = root_path / name
            if directory.is_symlink():
                continue
            directory_stat = directory.stat()
            include(directory_stat)
            newest_mtime = max(newest_mtime, directory_stat.st_mtime)
            kept_dirs.append(name)
        dirs[:] = kept_dirs
        for name in files:
            file_path = root_path / name
            if file_path.is_symlink():
                continue
            stat = file_path.stat()
            include(stat)
            newest_mtime = max(newest_mtime, stat.st_mtime)
    if walk_errors:
        raise walk_errors[0]
    return total, newest_mtime


def directory_bytes(path: Path) -> int:
    return directory_usage(path)[0]


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
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    except OSError as exc:
        if exc.errno == errno.ESRCH:
            return False
        if exc.errno == errno.EPERM:
            return True
        return True
    return True


def active_session_paths(home: Path | None = None) -> tuple[set[Path], list[str]]:
    home = home or Path(os.environ.get("JCODE_HOME", Path.home() / ".jcode"))
    active_dir = home / "active_pids"
    sessions_dir = home / "sessions"
    active: set[Path] = set()
    warnings: list[str] = []
    if not active_dir.is_dir():
        return active, warnings
    try:
        markers = list(active_dir.iterdir())
    except OSError as exc:
        return active, [f"cannot inspect active session markers: {exc}"]
    for marker in markers:
        if not marker.is_file():
            continue
        try:
            pid = parse_positive_int(marker.read_text(), "active session pid")
        except (OSError, ValueError) as exc:
            warnings.append(f"cannot trust active marker {marker.name}: {exc}")
            continue
        if not _pid_is_alive(pid):
            continue
        try:
            session_path = sessions_dir / f"{marker.name}.json"
            data = json.loads(session_path.read_text())
            working_dir = data.get("working_dir")
            if not isinstance(working_dir, str) or not working_dir.strip():
                raise DiskSafetyError("live session has no working_dir")
            active.add(Path(working_dir).resolve(strict=False))
        except (OSError, ValueError, json.JSONDecodeError, TypeError) as exc:
            warnings.append(f"cannot resolve live session {marker.name}: {exc}")
            continue
    return active, warnings


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


def inspect_worktrees(
    repo_root: Path, paths: Iterable[Path]
) -> tuple[list[WorktreeSnapshot], list[str]]:
    active_paths, activity_warnings = active_session_paths()
    active_paths |= _configured_active_paths()
    snapshots: list[WorktreeSnapshot] = []
    for path in paths:
        target = path / "target"
        safe, _ = safe_target_path(repo_root, path, target)
        target_bytes = 0
        target_mtime: float | None = None
        if safe:
            try:
                target_bytes, target_mtime = directory_usage(target)
            except OSError:
                safe = False
        dirty, status_error = worktree_is_dirty(path) if path.is_dir() else (True, True)
        snapshots.append(
            WorktreeSnapshot(
                path=path,
                target=target,
                is_main=path.resolve(strict=False) == repo_root.resolve(strict=False),
                active=(
                    bool(activity_warnings) or worktree_is_active(path, active_paths)
                    if path.is_dir()
                    else False
                ),
                dirty=dirty,
                target_bytes=target_bytes,
                target_mtime=target_mtime,
                status_error=status_error,
            )
        )
    return snapshots, activity_warnings


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


def _validate_candidate_state(candidate: CleanupCandidate) -> None:
    if candidate.worktree_path.resolve(strict=False) == candidate.repo_root.resolve(
        strict=False
    ):
        raise DiskSafetyError("refusing to remove the main checkout target")
    _, registered_paths = discover_worktrees(candidate.repo_root)
    registered = {path.resolve(strict=False) for path in registered_paths}
    if candidate.worktree_path.resolve(strict=False) not in registered:
        raise DiskSafetyError(
            f"worktree is no longer registered: {candidate.worktree_path}"
        )
    active_paths, activity_warnings = active_session_paths()
    if activity_warnings:
        raise DiskSafetyError(
            "active session state is uncertain: " + "; ".join(activity_warnings)
        )
    active_paths |= _configured_active_paths()
    if worktree_is_active(candidate.worktree_path, active_paths):
        raise DiskSafetyError(f"worktree became active: {candidate.worktree_path}")
    dirty, status_error = worktree_is_dirty(candidate.worktree_path)
    if dirty or status_error:
        raise DiskSafetyError(f"worktree became dirty or unreadable: {candidate.worktree_path}")
    safe, reason = safe_target_path(
        candidate.repo_root, candidate.worktree_path, candidate.target_path
    )
    if not safe:
        raise DiskSafetyError(f"refusing to remove {candidate.target_path}: {reason}")


def _revalidate_candidate(
    candidate: CleanupCandidate,
    *,
    now: float,
    min_age_seconds: int,
) -> int:
    _validate_candidate_state(candidate)
    bytes_before, newest_mtime = directory_usage(candidate.target_path)
    if now - newest_mtime < min_age_seconds:
        raise DiskSafetyError(f"target became too recent: {candidate.target_path}")
    # directory_usage can take seconds on a large Cargo tree. Recheck volatile
    # registration, activity, dirty-state, and path invariants after that scan,
    # immediately before the caller may remove the target.
    _validate_candidate_state(candidate)
    return bytes_before


def remove_candidates(
    candidates: Iterable[CleanupCandidate],
    *,
    apply: bool,
    min_age_seconds: int,
    now: float | None = None,
) -> int:
    now = time.time() if now is None else now
    prepared: list[tuple[CleanupCandidate, int]] = []
    for candidate in candidates:
        prepared.append(
            (
                candidate,
                _revalidate_candidate(
                    candidate, now=now, min_age_seconds=min_age_seconds
                ),
            )
        )

    reclaimed = 0
    for candidate, bytes_before in prepared:
        if apply:
            bytes_before = _revalidate_candidate(
                candidate, now=time.time(), min_age_seconds=min_age_seconds
            )
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
        snapshots, activity_warnings = inspect_worktrees(
            repo_root, all_paths[:max_worktrees]
        )
        free_bytes = filesystem_free_bytes(repo_root)
    except (OSError, subprocess.CalledProcessError, DiskSafetyError) as exc:
        print(f"disk report failed: {exc}", file=sys.stderr)
        return 1

    print(f"repository: {repo_root}")
    print(f"filesystem free: {human_bytes(free_bytes)}")
    shown = len(snapshots)
    suffix = f"; {len(all_paths) - shown} omitted" if shown < len(all_paths) else ""
    print(f"worktrees: {len(all_paths)} listed, {shown} shown{suffix}")
    for warning in activity_warnings:
        print(f"warning: cleanup disabled because {warning}")
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
        snapshots, activity_warnings = inspect_worktrees(repo_root, shown_paths)
        if activity_warnings:
            raise DiskSafetyError(
                "active session state is uncertain: " + "; ".join(activity_warnings)
            )
        min_age_seconds = min_age_days * 24 * 60 * 60
        candidates = select_cleanup_candidates(
            snapshots,
            repo_root,
            min_age_seconds=min_age_seconds,
        )
        reclaimed = remove_candidates(
            candidates,
            apply=apply,
            min_age_seconds=min_age_seconds,
        )
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
