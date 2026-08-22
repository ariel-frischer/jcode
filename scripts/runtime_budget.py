#!/usr/bin/env python3
"""Shared models and lifecycle primitives for runtime budget collection."""

from __future__ import annotations

import hashlib
import math
import os
import shutil
import signal
import stat
import statistics
import subprocess
import tempfile
import time
from dataclasses import asdict, dataclass
from enum import Enum
from pathlib import Path
from typing import Any, Mapping, Sequence


class RuntimeBudgetError(Exception):
    """Base error for invalid runtime-budget evidence."""


class ValidationError(RuntimeBudgetError):
    """Raised when report evidence violates the versioned model contract."""


class CompatibilityError(RuntimeBudgetError):
    """Raised when a report and baseline cannot be compared safely."""


class IdentityError(RuntimeBudgetError):
    """Raised when an executable or process identity cannot be trusted."""


class IsolationError(RuntimeBudgetError):
    """Raised when private-runtime ownership cannot be established."""


class Classification(str, Enum):
    PASS = "pass"
    REVIEW_REQUIRED = "review_required"
    DETERMINISTIC_FAILURE = "deterministic_failure"
    INVALID = "invalid"
    UNSUPPORTED = "unsupported"


SCHEMA_VERSION = "1.0.0"


def canonical_metric_definitions() -> dict[str, MetricDefinition]:
    """Return the maintained FR-001 through FR-007 metric inventory."""
    latency_policy = {
        "kind": "noisy_comparison",
        "relative_tolerance": 0.15,
        "absolute_tolerance": 20.0,
        "review_action": "maintainer_review",
    }
    round_trip_policy = {
        "kind": "noisy_comparison",
        "relative_tolerance": 0.15,
        "absolute_tolerance": 1.0,
        "review_action": "maintainer_review",
    }
    definitions = (
        MetricDefinition(
            id="first_visible_ms",
            unit="milliseconds",
            sampling={"warm_up_count": 1, "recorded_count": 10},
            aggregation=["median", "nearest_rank_p95"],
            policy=dict(latency_policy),
        ),
        MetricDefinition(
            id="input_ready_ms",
            unit="milliseconds",
            sampling={"warm_up_count": 1, "recorded_count": 10},
            aggregation=["median", "nearest_rank_p95"],
            policy=dict(latency_policy),
        ),
        MetricDefinition(
            id="daemon_ready_ms",
            unit="milliseconds",
            sampling={"recorded_count": 5, "timeout_seconds": 5},
            aggregation=["median"],
            policy={
                "kind": "deterministic",
                "ceiling": 80.0,
                "review_action": "fail",
            },
        ),
        MetricDefinition(
            id="idle_cpu_percent",
            unit="percentage_points",
            sampling={
                "settle_seconds": 5,
                "sample_interval_seconds": 1,
                "recorded_count": 5,
            },
            aggregation=["median"],
            policy={
                "kind": "noisy_comparison",
                "relative_tolerance": 0.0,
                "absolute_tolerance": 0.5,
                "review_action": "maintainer_review",
            },
        ),
        MetricDefinition(
            id="idle_rss_mib",
            unit="mib",
            sampling={
                "settle_seconds": 5,
                "sample_interval_seconds": 1,
                "recorded_count": 5,
            },
            aggregation=["median"],
            policy={
                "kind": "noisy_comparison",
                "relative_tolerance": 0.10,
                "absolute_tolerance": 8.0,
                "review_action": "maintainer_review",
            },
        ),
        MetricDefinition(
            id="session_scaling_mib_per_session",
            unit="mib_per_session",
            sampling={
                "populations": [1, 4, 8],
                "trials_per_population": 3,
                "recorded_count": 3,
            },
            aggregation=["median"],
            policy={
                "kind": "noisy_comparison",
                "relative_tolerance": 0.15,
                "absolute_tolerance": 2.0,
                "review_action": "maintainer_review",
            },
        ),
        MetricDefinition(
            id="frame_update_work_count",
            unit="work_count",
            sampling={"recorded_count": 1},
            aggregation=["exact"],
            policy={
                "kind": "deterministic",
                "unchanged_transcript_layout_work": 0,
                "review_action": "fail",
            },
        ),
        MetricDefinition(
            id="protocol_round_trip_ms",
            unit="milliseconds",
            sampling={"warm_up_count": 5, "recorded_count": 30},
            aggregation=["median", "nearest_rank_p95"],
            policy=dict(round_trip_policy),
        ),
        MetricDefinition(
            id="tool_round_trip_ms",
            unit="milliseconds",
            sampling={"warm_up_count": 5, "recorded_count": 30},
            aggregation=["median", "nearest_rank_p95"],
            policy=dict(round_trip_policy),
        ),
    )
    return {definition.id: definition for definition in definitions}


def median(samples: Sequence[float]) -> float:
    if not samples:
        raise ValidationError("median requires at least one sample")
    return float(statistics.median(samples))


def nearest_rank_percentile(samples: Sequence[float], percentile: float) -> float:
    if not samples:
        raise ValidationError("nearest-rank percentile requires at least one sample")
    if not 0 < percentile <= 100:
        raise ValidationError("percentile must be greater than 0 and at most 100")
    ordered = sorted(float(sample) for sample in samples)
    rank = math.ceil(percentile / 100 * len(ordered))
    return ordered[rank - 1]


def classify_tolerance(
    *,
    candidate: float,
    baseline: float,
    relative_tolerance: float,
    absolute_tolerance: float,
) -> Classification:
    if relative_tolerance < 0 or absolute_tolerance < 0:
        raise ValidationError("tolerances must not be negative")
    allowed_increase = max(abs(baseline) * relative_tolerance, absolute_tolerance)
    if candidate > baseline + allowed_increase:
        return Classification.REVIEW_REQUIRED
    return Classification.PASS


@dataclass(frozen=True)
class MetricDefinition:
    id: str
    unit: str
    sampling: dict[str, Any]
    aggregation: list[str]
    policy: dict[str, Any]

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_dict(cls, value: Mapping[str, Any]) -> "MetricDefinition":
        return cls(
            id=str(value["id"]),
            unit=str(value["unit"]),
            sampling=dict(value["sampling"]),
            aggregation=[str(item) for item in value["aggregation"]],
            policy=dict(value["policy"]),
        )


@dataclass(frozen=True)
class MetricResult:
    samples: list[float]
    aggregates: dict[str, float]
    classification: Classification
    diagnostics: list[str]

    def to_dict(self) -> dict[str, Any]:
        return {
            "samples": list(self.samples),
            "aggregates": dict(self.aggregates),
            "classification": self.classification.value,
            "diagnostics": list(self.diagnostics),
        }

    @classmethod
    def from_dict(cls, value: Mapping[str, Any]) -> "MetricResult":
        try:
            classification = Classification(value["classification"])
        except (KeyError, ValueError) as error:
            raise ValidationError(
                "metric result has an unknown classification"
            ) from error
        return cls(
            samples=[float(sample) for sample in value.get("samples", [])],
            aggregates={
                str(name): float(result)
                for name, result in dict(value.get("aggregates", {})).items()
            },
            classification=classification,
            diagnostics=[str(item) for item in value.get("diagnostics", [])],
        )


@dataclass(frozen=True)
class EnvironmentProvenance:
    platform: dict[str, Any]
    toolchain_profile: dict[str, Any]
    command_parameters: dict[str, Any]

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_dict(cls, value: Mapping[str, Any]) -> "EnvironmentProvenance":
        return cls(
            platform=dict(value["platform"]),
            toolchain_profile=dict(value["toolchain_profile"]),
            command_parameters=dict(value["command_parameters"]),
        )


@dataclass(frozen=True)
class RuntimeReport:
    schema_version: str
    executable: dict[str, Any]
    daemon: dict[str, Any]
    environment: EnvironmentProvenance
    command: dict[str, Any]
    definitions: dict[str, MetricDefinition]
    metrics: dict[str, MetricResult]
    baseline: dict[str, Any] | None
    cleanup: dict[str, Any]

    def to_dict(self) -> dict[str, Any]:
        return {
            "schema_version": self.schema_version,
            "executable": dict(self.executable),
            "daemon": dict(self.daemon),
            "environment": self.environment.to_dict(),
            "command": dict(self.command),
            "definitions": {
                metric_id: definition.to_dict()
                for metric_id, definition in self.definitions.items()
            },
            "metrics": {
                metric_id: result.to_dict()
                for metric_id, result in self.metrics.items()
            },
            "baseline": None if self.baseline is None else dict(self.baseline),
            "cleanup": dict(self.cleanup),
        }

    @classmethod
    def from_dict(cls, value: Mapping[str, Any]) -> "RuntimeReport":
        return cls(
            schema_version=str(value["schema_version"]),
            executable=dict(value["executable"]),
            daemon=dict(value["daemon"]),
            environment=EnvironmentProvenance.from_dict(value["environment"]),
            command=dict(value["command"]),
            definitions={
                str(metric_id): MetricDefinition.from_dict(definition)
                for metric_id, definition in dict(value["definitions"]).items()
            },
            metrics={
                str(metric_id): MetricResult.from_dict(result)
                for metric_id, result in dict(value["metrics"]).items()
            },
            baseline=(
                None if value.get("baseline") is None else dict(value["baseline"])
            ),
            cleanup=dict(value["cleanup"]),
        )


@dataclass(frozen=True)
class RuntimeBaseline:
    schema_version: str
    collected_at: str
    reason: str
    environment: EnvironmentProvenance
    definitions: dict[str, MetricDefinition]
    metrics: dict[str, MetricResult]

    def to_dict(self) -> dict[str, Any]:
        return {
            "schema_version": self.schema_version,
            "collected_at": self.collected_at,
            "reason": self.reason,
            "environment": self.environment.to_dict(),
            "definitions": {
                metric_id: definition.to_dict()
                for metric_id, definition in self.definitions.items()
            },
            "metrics": {
                metric_id: result.to_dict()
                for metric_id, result in self.metrics.items()
            },
        }

    @classmethod
    def from_dict(cls, value: Mapping[str, Any]) -> "RuntimeBaseline":
        return cls(
            schema_version=str(value["schema_version"]),
            collected_at=str(value["collected_at"]),
            reason=str(value["reason"]),
            environment=EnvironmentProvenance.from_dict(value["environment"]),
            definitions={
                str(metric_id): MetricDefinition.from_dict(definition)
                for metric_id, definition in dict(value["definitions"]).items()
            },
            metrics={
                str(metric_id): MetricResult.from_dict(result)
                for metric_id, result in dict(value["metrics"]).items()
            },
        )


def validate_metric_result(definition: MetricDefinition, result: MetricResult) -> None:
    if result.classification is Classification.UNSUPPORTED:
        if result.samples or result.aggregates or not result.diagnostics:
            raise ValidationError(
                f"unsupported metric {definition.id} must contain diagnostics only"
            )
        return
    if result.classification is Classification.INVALID:
        if not result.diagnostics:
            raise ValidationError(
                f"invalid metric {definition.id} must explain the failure"
            )
        return

    expected_count = definition.sampling.get("recorded_count")
    if not isinstance(expected_count, int) or expected_count < 1:
        raise ValidationError(f"metric {definition.id} has no valid recorded_count")
    if len(result.samples) != expected_count:
        raise ValidationError(
            f"metric {definition.id} requires {expected_count} samples; "
            f"received {len(result.samples)}"
        )
    if not all(math.isfinite(sample) for sample in result.samples):
        raise ValidationError(f"metric {definition.id} contains a non-finite sample")

    expected_aggregates: dict[str, float] = {}
    for aggregation in definition.aggregation:
        if aggregation == "median":
            expected_aggregates[aggregation] = median(result.samples)
        elif aggregation == "nearest_rank_p95":
            expected_aggregates[aggregation] = nearest_rank_percentile(
                result.samples, 95
            )
        elif aggregation not in result.aggregates:
            raise ValidationError(
                f"metric {definition.id} is missing aggregate {aggregation}"
            )
    for name, expected in expected_aggregates.items():
        actual = result.aggregates.get(name)
        if actual is None or not math.isclose(actual, expected):
            raise ValidationError(
                f"metric {definition.id} has incorrect aggregate {name}"
            )


def _require_provenance_fields(
    section: str, value: Mapping[str, Any], fields: Sequence[str]
) -> None:
    missing = [field for field in fields if value.get(field) in (None, "", [], {})]
    if missing:
        raise ValidationError(
            f"report {section} provenance is missing: {', '.join(missing)}"
        )


def validate_report(report: RuntimeReport) -> None:
    """Validate a complete report against the canonical metric contract."""
    if report.schema_version != SCHEMA_VERSION:
        raise ValidationError(
            f"unsupported report schema version: {report.schema_version}"
        )

    expected_definitions = canonical_metric_definitions()
    if report.definitions != expected_definitions:
        missing = sorted(set(expected_definitions) - set(report.definitions))
        extra = sorted(set(report.definitions) - set(expected_definitions))
        details = []
        if missing:
            details.append(f"missing {', '.join(missing)}")
        if extra:
            details.append(f"unexpected {', '.join(extra)}")
        if not details:
            details.append(
                "one or more definitions differ from the maintained contract"
            )
        raise ValidationError(
            f"canonical metric inventory mismatch: {'; '.join(details)}"
        )

    if set(report.metrics) != set(expected_definitions):
        raise ValidationError(
            "report results must exactly match the canonical metric inventory"
        )

    _require_provenance_fields(
        "executable",
        report.executable,
        ("requested_path", "resolved_path", "version_revision", "sha256"),
    )
    _require_provenance_fields(
        "daemon", report.daemon, ("pid", "resolved_path", "socket")
    )
    _require_provenance_fields("command", report.command, ("argv", "started_at"))
    _require_provenance_fields(
        "environment platform",
        report.environment.platform,
        ("os", "architecture"),
    )
    _require_provenance_fields(
        "environment toolchain", report.environment.toolchain_profile, ("python",)
    )
    if not report.environment.command_parameters:
        raise ValidationError("report environment command parameters are missing")
    if report.baseline is not None:
        _require_provenance_fields(
            "baseline", report.baseline, ("path", "sha256")
        )
    _require_provenance_fields(
        "cleanup",
        report.cleanup,
        ("owned_processes_stopped", "private_paths_removed"),
    )

    for metric_id, definition in expected_definitions.items():
        validate_metric_result(definition, report.metrics[metric_id])


def validate_baseline_compatibility(
    report: RuntimeReport, baseline: RuntimeBaseline
) -> None:
    if report.schema_version != baseline.schema_version:
        raise CompatibilityError("report and baseline schema versions are incompatible")
    if report.environment != baseline.environment:
        raise CompatibilityError(
            "report and baseline environment or command parameters differ"
        )


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as executable:
        for chunk in iter(lambda: executable.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


@dataclass(frozen=True)
class ExecutableIdentity:
    requested_path: str
    resolved_path: str
    version_revision: str
    sha256: str
    size_bytes: int
    mtime_ns: int
    device: int
    inode: int

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


def inspect_executable(
    path: str | Path, *, not_before_mtime_ns: int | None = None
) -> ExecutableIdentity:
    requested = Path(path).expanduser()
    requested_absolute = Path(os.path.abspath(requested))
    try:
        resolved = requested.resolve(strict=True)
    except OSError as error:
        raise IdentityError(
            f"candidate executable does not exist: {requested}"
        ) from error
    if requested_absolute != resolved:
        raise IdentityError(
            f"candidate path resolves to a different executable: {requested_absolute} -> {resolved}"
        )
    try:
        details = resolved.stat()
    except OSError as error:
        raise IdentityError(
            f"cannot inspect candidate executable: {resolved}"
        ) from error
    if not stat.S_ISREG(details.st_mode) or not os.access(resolved, os.X_OK):
        raise IdentityError(f"candidate is not an executable file: {resolved}")
    if not_before_mtime_ns is not None and details.st_mtime_ns < not_before_mtime_ns:
        raise IdentityError(
            f"candidate executable predates the declared build start: {resolved}"
        )
    try:
        completed = subprocess.run(
            [str(resolved), "--version"],
            check=True,
            capture_output=True,
            text=True,
            timeout=15,
        )
    except (OSError, subprocess.SubprocessError) as error:
        raise IdentityError(f"cannot read candidate version: {resolved}") from error
    version_revision = (completed.stdout or completed.stderr).strip()
    if not version_revision:
        raise IdentityError(f"candidate returned an empty version: {resolved}")
    return ExecutableIdentity(
        requested_path=str(requested_absolute),
        resolved_path=str(resolved),
        version_revision=version_revision,
        sha256=_sha256(resolved),
        size_bytes=details.st_size,
        mtime_ns=details.st_mtime_ns,
        device=details.st_dev,
        inode=details.st_ino,
    )


def verify_executable_unchanged(identity: ExecutableIdentity) -> None:
    path = Path(identity.resolved_path)
    try:
        details = path.stat()
    except OSError as error:
        raise IdentityError(f"candidate executable disappeared: {path}") from error
    current = (details.st_size, details.st_mtime_ns, details.st_dev, details.st_ino)
    expected = (
        identity.size_bytes,
        identity.mtime_ns,
        identity.device,
        identity.inode,
    )
    if current != expected or _sha256(path) != identity.sha256:
        raise IdentityError(f"candidate executable changed during collection: {path}")


def verify_daemon_executable(identity: ExecutableIdentity, pid: int) -> None:
    try:
        running = Path(f"/proc/{pid}/exe").resolve(strict=True)
    except OSError as error:
        raise IdentityError(
            f"cannot resolve executable for daemon pid {pid}"
        ) from error
    if running != Path(identity.resolved_path):
        raise IdentityError(
            f"daemon pid {pid} runs {running}, expected {identity.resolved_path}"
        )


def _socket_inodes(path: Path) -> set[int]:
    try:
        details = path.stat()
    except OSError as error:
        raise IsolationError(f"runtime socket does not exist: {path}") from error
    if not stat.S_ISSOCK(details.st_mode):
        raise IsolationError(f"runtime path is not a Unix socket: {path}")
    try:
        entries = Path("/proc/net/unix").read_text(encoding="utf-8").splitlines()
    except OSError as error:
        raise IsolationError("cannot inspect Linux Unix socket ownership") from error
    expected_path = str(path.resolve())
    inodes: set[int] = set()
    for entry in entries[1:]:
        fields = entry.split(maxsplit=7)
        if len(fields) == 8 and fields[7] == expected_path:
            try:
                inodes.add(int(fields[6]))
            except ValueError as error:
                raise IsolationError(
                    f"invalid kernel inode for runtime socket: {path}"
                ) from error
    if inodes:
        return inodes
    raise IsolationError(f"runtime socket is not registered by the kernel: {path}")


def _pid_socket_inodes(pid: int) -> set[int]:
    inodes: set[int] = set()
    try:
        descriptors = list(Path(f"/proc/{pid}/fd").iterdir())
    except OSError:
        return inodes
    for descriptor in descriptors:
        try:
            target = os.readlink(descriptor)
        except OSError:
            continue
        if target.startswith("socket:[") and target.endswith("]"):
            try:
                inodes.add(int(target[8:-1]))
            except ValueError:
                continue
    return inodes


def socket_owner_pid(path: str | Path) -> int:
    socket_path = Path(path)
    socket_inodes = _socket_inodes(socket_path)
    owners: list[int] = []
    for proc_dir in Path("/proc").glob("[0-9]*"):
        try:
            pid = int(proc_dir.name)
        except ValueError:
            continue
        if socket_inodes & _pid_socket_inodes(pid):
            owners.append(pid)
    if len(owners) == 1:
        return owners[0]
    if not owners:
        raise IsolationError(f"no process owns runtime socket {socket_path}")
    raise IsolationError(
        f"multiple processes own runtime socket {socket_path}: {sorted(owners)}"
    )


def verify_socket_owner(path: str | Path, pid: int) -> None:
    socket_path = Path(path)
    if _socket_inodes(socket_path) & _pid_socket_inodes(pid):
        return
    raise IsolationError(f"pid {pid} does not own runtime socket {socket_path}")


@dataclass
class PrivateRuntime:
    root: Path
    home_dir: Path
    runtime_dir: Path
    socket_path: Path
    _cleaned: bool = False

    @classmethod
    def create(
        cls,
        *,
        parent_dir: Path | None = None,
        socket_path: Path | None = None,
    ) -> "PrivateRuntime":
        if socket_path is not None and socket_path.exists():
            raise IsolationError(f"refusing to reuse existing socket: {socket_path}")
        parent = None if parent_dir is None else str(parent_dir)
        root = Path(tempfile.mkdtemp(prefix="jcode-runtime-budget-", dir=parent))
        home_dir = root / "home"
        runtime_dir = root / "run"
        home_dir.mkdir(mode=0o700)
        runtime_dir.mkdir(mode=0o700)
        selected_socket = socket_path or runtime_dir / "jcode.sock"
        return cls(
            root=root,
            home_dir=home_dir,
            runtime_dir=runtime_dir,
            socket_path=selected_socket,
        )

    def environment(self) -> dict[str, str]:
        env = os.environ.copy()
        env.update(
            {
                "JCODE_HOME": str(self.home_dir),
                "JCODE_RUNTIME_DIR": str(self.runtime_dir),
                "JCODE_SOCKET": str(self.socket_path),
                "JCODE_NO_TELEMETRY": "1",
                "JCODE_DEBUG_CONTROL": "1",
            }
        )
        return env

    def cleanup(self) -> None:
        if self._cleaned:
            return
        try:
            if self.root.exists():
                shutil.rmtree(self.root, ignore_errors=True)
        except OSError:
            pass
        finally:
            self._cleaned = True


def _process_state(pid: int) -> tuple[str, int, int]:
    try:
        record = Path(f"/proc/{pid}/stat").read_text(encoding="utf-8")
    except OSError as error:
        raise IsolationError(f"cannot inspect owned process {pid}") from error
    command_end = record.rfind(")")
    fields = record[command_end + 1 :].split() if command_end >= 0 else []
    if len(fields) < 20:
        raise IsolationError(f"process identity is incomplete for pid {pid}")
    return fields[0], int(fields[2]), int(fields[19])


@dataclass(frozen=True)
class OwnedProcess:
    pid: int
    process_group: int
    start_ticks: int

    @classmethod
    def capture(cls, pid: int) -> "OwnedProcess":
        _, process_group, start_ticks = _process_state(pid)
        return cls(pid=pid, process_group=process_group, start_ticks=start_ticks)

    def is_same_process(self) -> bool:
        try:
            _, process_group, start_ticks = _process_state(self.pid)
        except IsolationError:
            return False
        return process_group == self.process_group and start_ticks == self.start_ticks

    def is_stopped(self) -> bool:
        try:
            state, process_group, start_ticks = _process_state(self.pid)
        except IsolationError:
            return True
        if process_group != self.process_group or start_ticks != self.start_ticks:
            return True
        return state == "Z"


@dataclass(frozen=True)
class CleanupResult:
    all_stopped: bool
    diagnostics: list[str]


def _wait_until_stopped(process: OwnedProcess, timeout_s: float) -> bool:
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        if process.is_stopped():
            return True
        time.sleep(min(0.01, max(0.0, deadline - time.monotonic())))
    return process.is_stopped()


def cleanup_owned_processes(
    processes: Sequence[OwnedProcess],
    *,
    term_timeout_s: float = 2.0,
    kill_timeout_s: float = 2.0,
) -> CleanupResult:
    diagnostics: list[str] = []
    active: list[OwnedProcess] = []
    current_group = os.getpgrp()
    for process in processes:
        if process.is_stopped():
            continue
        if process.pid != process.process_group:
            diagnostics.append(
                f"pid {process.pid} is not the leader of its owned process group"
            )
            continue
        if process.process_group == current_group or not process.is_same_process():
            diagnostics.append(f"ownership changed for pid {process.pid}")
            continue
        try:
            os.killpg(process.process_group, signal.SIGTERM)
            active.append(process)
        except ProcessLookupError:
            continue
        except OSError as error:
            diagnostics.append(f"failed to terminate pid {process.pid}: {error}")

    remaining = [
        process
        for process in active
        if not _wait_until_stopped(process, term_timeout_s)
    ]
    for process in remaining:
        if not process.is_same_process():
            continue
        try:
            os.killpg(process.process_group, signal.SIGKILL)
        except ProcessLookupError:
            continue
        except OSError as error:
            diagnostics.append(f"failed to kill pid {process.pid}: {error}")
    still_running = [
        process
        for process in remaining
        if not _wait_until_stopped(process, kill_timeout_s)
    ]
    diagnostics.extend(
        f"owned process {process.pid} did not stop" for process in still_running
    )
    return CleanupResult(
        all_stopped=not diagnostics and not still_running, diagnostics=diagnostics
    )
