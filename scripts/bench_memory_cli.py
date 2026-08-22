#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib.util
import json
import os
import platform
import pty
import re
import select
import shutil
import signal
import socket
import statistics
import subprocess
import sys
import tempfile
import time
from dataclasses import asdict, dataclass
from pathlib import Path

try:
    import runtime_budget as budget
except ModuleNotFoundError:  # Imported as scripts.bench_memory_cli.
    from scripts import runtime_budget as budget

ANSI_RE = re.compile(
    r"\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~]|\][^\x1b\x07]*(?:\x07|\x1b\\))"
)
PROBE = "jqx92"
DEFAULT_TIMEOUT_S = 20.0
DEFAULT_SETTLE_S = 1.0
IDLE_SETTLE_S = 5.0
ATTRIBUTION_SETTLE_S = 5.1
IDLE_SAMPLE_INTERVAL_S = 1.0
IDLE_SAMPLE_COUNT = 5
SCALING_POPULATIONS = (1, 4, 8)
SCALING_TRIALS = 3
DEFAULT_TOOLS = [
    "jcode_memory_off",
    "jcode_memory_on",
    "pi",
    "codex",
    "opencode",
    "copilot_cli",
    "cursor_agent",
    "claude_code",
    "antigravity_cli",
]


@dataclass
class ToolSpec:
    name: str
    argv: list[str]
    version_argv: list[str]
    env: dict[str, str] | None = None
    jcode: bool = False


@dataclass
class SessionLaunch:
    root_pid: int
    pgid: int
    master_fd: int
    ready: bool
    input_ready: bool
    excerpt: str | None
    seconds_to_visible: float | None
    seconds_to_input_ready: float | None
    buffer_excerpt: str | None


@dataclass
class ToolRunResult:
    tool: str
    sessions: int
    pss_mb: float
    process_count: int
    version: str
    notes: list[str]


def shutil_which(name: str) -> str | None:
    return (
        subprocess.run(
            ["bash", "-lc", f"command -v {name}"],
            capture_output=True,
            text=True,
            check=False,
        ).stdout.strip()
        or None
    )


def detect_pi_bin() -> str:
    direct = shutil_which("pi")
    if direct:
        return direct
    prefix = subprocess.check_output(["npm", "prefix", "-g"], text=True).strip()
    candidate = Path(prefix) / "bin" / "pi"
    if candidate.exists():
        return str(candidate)
    raise FileNotFoundError("could not find pi binary")


def build_specs() -> dict[str, ToolSpec]:
    jcode = shutil.which("jcode") or str(Path.home() / ".local/bin/jcode")
    codex = shutil.which("codex") or "/usr/bin/codex"
    opencode = shutil.which("opencode") or "/usr/bin/opencode"
    copilot = shutil.which("copilot") or str(Path.home() / ".local/bin/copilot")
    cursor_agent = shutil.which("cursor-agent") or str(
        Path.home() / ".local/bin/cursor-agent"
    )
    claude = shutil.which("claude") or str(Path.home() / ".local/bin/claude")
    agy = shutil.which("agy") or str(Path.home() / ".local/bin/agy")
    specs = {
        "jcode_memory_off": ToolSpec(
            name="jcode_memory_off",
            argv=[jcode, "--no-update", "--no-selfdev"],
            version_argv=[jcode, "version"],
            env={"JCODE_NO_TELEMETRY": "1", "JCODE_MEMORY_ENABLED": "0"},
            jcode=True,
        ),
        "jcode_memory_on": ToolSpec(
            name="jcode_memory_on",
            argv=[jcode, "--no-update", "--no-selfdev"],
            version_argv=[jcode, "version"],
            env={"JCODE_NO_TELEMETRY": "1", "JCODE_MEMORY_ENABLED": "1"},
            jcode=True,
        ),
        "pi": ToolSpec(
            name="pi",
            argv=[detect_pi_bin()],
            version_argv=[detect_pi_bin(), "--version"],
        ),
        "codex": ToolSpec(
            name="codex",
            argv=[codex],
            version_argv=[codex, "--version"],
        ),
        "opencode": ToolSpec(
            name="opencode",
            argv=[opencode],
            version_argv=[opencode, "--version"],
        ),
        "copilot_cli": ToolSpec(
            name="copilot_cli",
            argv=[copilot],
            version_argv=[copilot, "--version"],
        ),
        "cursor_agent": ToolSpec(
            name="cursor_agent",
            argv=[cursor_agent],
            version_argv=[cursor_agent, "--version"],
        ),
        "claude_code": ToolSpec(
            name="claude_code",
            argv=[claude],
            version_argv=[claude, "--version"],
        ),
        "antigravity_cli": ToolSpec(
            name="antigravity_cli",
            argv=[agy],
            version_argv=[agy, "--version"],
        ),
    }
    return specs


def reply_queries(master_fd: int, buffer: bytes) -> bytes:
    replies = [
        (b"\x1b[6n", b"\x1b[1;1R"),
        (b"\x1b[c", b"\x1b[?62;c"),
        (b"\x1b]10;?\x1b\\", b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\"),
        (b"\x1b]11;?\x1b\\", b"\x1b]11;rgb:0000/0000/0000\x1b\\"),
        (b"\x1b]10;?\x07", b"\x1b]10;rgb:ffff/ffff/ffff\x07"),
        (b"\x1b]11;?\x07", b"\x1b]11;rgb:0000/0000/0000\x07"),
        (b"\x1b]4;0;?\x07", b"\x1b]4;0;rgb:0000/0000/0000\x07"),
        (b"\x1b[14t", b"\x1b[4;600;800t"),
        (b"\x1b[16t", b"\x1b[6;16;8t"),
        (b"\x1b[18t", b"\x1b[8;24;80t"),
        (b"\x1b[?1016$p", b"\x1b[?1016;1$y"),
        (b"\x1b[?2027$p", b"\x1b[?2027;1$y"),
        (b"\x1b[?2031$p", b"\x1b[?2031;1$y"),
        (b"\x1b[?1004$p", b"\x1b[?1004;1$y"),
        (b"\x1b[?2004$p", b"\x1b[?2004;1$y"),
        (b"\x1b[?2026$p", b"\x1b[?2026;1$y"),
    ]
    changed = True
    while changed:
        changed = False
        for query, response in replies:
            if query in buffer:
                os.write(master_fd, response)
                buffer = buffer.replace(query, b"")
                changed = True
    return buffer


def strip_ansi(text: str) -> str:
    return ANSI_RE.sub("", text).replace("\r", "\n")


def first_meaningful_line(text: str) -> str | None:
    for raw_line in text.splitlines():
        line = " ".join(raw_line.split())
        if not line:
            continue
        alnum_count = sum(ch.isalnum() for ch in line)
        if alnum_count >= 3 and len(line) >= 4:
            return line[:160]
    return None


def wait_for_socket(path: str, timeout_s: float) -> bool:
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        if os.path.exists(path):
            try:
                sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                sock.connect(path)
                sock.close()
                return True
            except OSError:
                pass
        time.sleep(0.05)
    return False


def create_debug_session(*, socket_path: Path, cwd: Path, timeout_s: float) -> str:
    request = {
        "type": "debug_command",
        "id": 1,
        "command": f"create_session:{cwd}",
    }
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(timeout_s)
        connection.connect(str(socket_path))
        connection.sendall(
            (json.dumps(request, separators=(",", ":")) + "\n").encode()
        )
        received = b""
        while True:
            chunk = connection.recv(64 * 1024)
            if not chunk:
                raise RuntimeError("debug session creation ended without a response")
            received += chunk
            while b"\n" in received:
                line, received = received.split(b"\n", 1)
                if not line:
                    continue
                response = json.loads(line)
                if response.get("id") != request["id"]:
                    continue
                if (
                    response.get("type") != "debug_response"
                    or response.get("ok") is not True
                ):
                    raise RuntimeError(
                        str(
                            response.get("output")
                            or response.get("message")
                            or "debug session creation failed"
                        )
                    )
                metadata = json.loads(str(response.get("output") or ""))
                session_id = (
                    metadata.get("session_id") if isinstance(metadata, dict) else None
                )
                if not isinstance(session_id, str) or not session_id:
                    raise RuntimeError("debug session creation omitted session_id")
                return session_id


def launch_interactive(
    argv: list[str], cwd: Path, env: dict[str, str], timeout_s: float, settle_s: float
) -> SessionLaunch:
    master_fd, slave_fd = pty.openpty()
    proc = subprocess.Popen(
        argv,
        cwd=str(cwd),
        env=env,
        stdin=slave_fd,
        stdout=slave_fd,
        stderr=slave_fd,
        preexec_fn=os.setsid,
    )
    os.close(slave_fd)
    os.set_blocking(master_fd, False)
    start = time.perf_counter()
    buf = b""
    ready = False
    input_ready = False
    probe_sent = False
    excerpt = None
    while time.perf_counter() - start < timeout_s:
        rlist, _, _ = select.select([master_fd], [], [], 0.05)
        if rlist:
            try:
                chunk = os.read(master_fd, 65536)
            except BlockingIOError:
                chunk = b""
            if chunk:
                buf += chunk
                buf = reply_queries(master_fd, buf)
                plain = strip_ansi(buf.decode("utf-8", "replace"))
                excerpt = first_meaningful_line(plain)
                if excerpt:
                    ready = True
                    if not probe_sent:
                        try:
                            os.write(master_fd, PROBE.encode())
                            probe_sent = True
                        except OSError:
                            break
                if probe_sent and PROBE in plain:
                    input_ready = True
                    break
        if proc.poll() is not None:
            break
    if input_ready or ready:
        time.sleep(settle_s)
    elapsed = time.perf_counter() - start
    return SessionLaunch(
        root_pid=proc.pid,
        pgid=os.getpgid(proc.pid),
        master_fd=master_fd,
        ready=ready,
        input_ready=input_ready,
        excerpt=excerpt,
        seconds_to_visible=elapsed if ready else None,
        seconds_to_input_ready=elapsed if input_ready else None,
        buffer_excerpt=(strip_ansi(buf.decode("utf-8", "replace"))[:300] or None),
    )


def iter_proc_stat() -> dict[int, tuple[int, int]]:
    out: dict[int, tuple[int, int]] = {}
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            stat = (entry / "stat").read_text()
        except Exception:
            continue
        try:
            close = stat.rfind(")")
            rest = stat[close + 2 :].split()
            ppid = int(rest[1])
            pgid = int(rest[2])
            out[int(entry.name)] = (ppid, pgid)
        except Exception:
            continue
    return out


def collect_descendants(root_pids: list[int]) -> set[int]:
    ppid_of = iter_proc_stat()
    children: dict[int, list[int]] = {}
    for pid, (ppid, _pgid) in ppid_of.items():
        children.setdefault(ppid, []).append(pid)
    seen: set[int] = set()
    stack = list(root_pids)
    while stack:
        pid = stack.pop()
        if pid in seen:
            continue
        seen.add(pid)
        stack.extend(children.get(pid, []))
    return seen


def collect_process_group_pids(pgids: list[int]) -> set[int]:
    proc_map = iter_proc_stat()
    wanted = set(pgids)
    return {pid for pid, (_ppid, pgid) in proc_map.items() if pgid in wanted}


def read_pss_mb(pid: int) -> float | None:
    path = Path(f"/proc/{pid}/smaps_rollup")
    try:
        for line in path.read_text().splitlines():
            if line.startswith("Pss:"):
                return int(line.split()[1]) / 1024.0
    except Exception:
        return None
    return None


def _read_proc_cpu_ticks(pid: int) -> int:
    stat = Path(f"/proc/{pid}/stat").read_text()
    fields = stat[stat.rfind(")") + 2 :].split()
    return int(fields[11]) + int(fields[12])


def _read_proc_rss_mib(pid: int) -> float:
    for line in Path(f"/proc/{pid}/status").read_text().splitlines():
        if line.startswith("VmRSS:"):
            return int(line.split()[1]) / 1024.0
    raise RuntimeError(f"/proc/{pid}/status does not contain VmRSS")


def sample_process_resources(
    pid: int, sample_window_s: float = IDLE_SAMPLE_INTERVAL_S
) -> dict[str, float]:
    """Sample one Linux process using the same procfs surface as profile_spawn.py."""
    clock_ticks = os.sysconf("SC_CLK_TCK")
    started = time.perf_counter()
    initial_ticks = _read_proc_cpu_ticks(pid)
    time.sleep(sample_window_s)
    elapsed = time.perf_counter() - started
    final_ticks = _read_proc_cpu_ticks(pid)
    cpu_percent = ((final_ticks - initial_ticks) / clock_ticks) / elapsed * 100.0
    return {
        "cpu_percent": round(cpu_percent, 3),
        "rss_mib": round(_read_proc_rss_mib(pid), 3),
    }


def collect_idle_resources(
    daemon_pid: int,
    *,
    platform_name: str | None = None,
) -> dict[str, object]:
    platform_name = platform_name or platform.system()
    sampling = {
        "settle_seconds": IDLE_SETTLE_S,
        "sample_interval_seconds": IDLE_SAMPLE_INTERVAL_S,
        "recorded_count": IDLE_SAMPLE_COUNT,
    }
    if platform_name != "Linux":
        return {
            "status": "unsupported",
            "platform": platform_name,
            "sampling": sampling,
            "samples": [],
            "aggregates": {},
            "diagnostic": "Linux procfs is required for idle CPU and RSS evidence",
        }

    samples: list[dict[str, float]] = []
    try:
        time.sleep(IDLE_SETTLE_S)
        for _ in range(IDLE_SAMPLE_COUNT):
            sample = sample_process_resources(
                daemon_pid, sample_window_s=IDLE_SAMPLE_INTERVAL_S
            )
            if not isinstance(sample.get("cpu_percent"), int | float) or not isinstance(
                sample.get("rss_mib"), int | float
            ):
                raise RuntimeError("procfs sample omitted CPU or RSS")
            samples.append(sample)
    except (OSError, RuntimeError, ValueError) as error:
        return {
            "status": "invalid",
            "platform": platform_name,
            "sampling": sampling,
            "samples": samples,
            "aggregates": {},
            "diagnostic": str(error),
        }

    return {
        "status": "valid",
        "platform": platform_name,
        "sampling": sampling,
        "samples": samples,
        "aggregates": {
            "median_cpu_percent": statistics.median(
                sample["cpu_percent"] for sample in samples
            ),
            "median_rss_mib": statistics.median(
                sample["rss_mib"] for sample in samples
            ),
        },
        "diagnostic": None,
    }


def sum_tree_pss(root_pids: list[int], pgids: list[int]) -> tuple[float, int]:
    all_pids = collect_descendants(root_pids) | collect_process_group_pids(pgids)
    total = 0.0
    counted = 0
    for pid in sorted(all_pids):
        pss = read_pss_mb(pid)
        if pss is None:
            continue
        total += pss
        counted += 1
    return round(total, 1), counted


def terminate_pgroup(pgid: int) -> None:
    for sig in (signal.SIGTERM, signal.SIGKILL):
        try:
            os.killpg(pgid, sig)
            time.sleep(0.2)
        except ProcessLookupError:
            return


def version_for(spec: ToolSpec) -> str:
    proc = subprocess.run(
        spec.version_argv, capture_output=True, text=True, check=False
    )
    output = (proc.stdout + proc.stderr).strip().splitlines()
    return output[0] if output else f"exit {proc.returncode}"


def run_tool(
    spec: ToolSpec, sessions: int, cwd: Path, timeout_s: float, settle_s: float
) -> ToolRunResult:
    notes: list[str] = []
    version = version_for(spec)
    launches: list[SessionLaunch] = []
    cleanup_pgids: list[int] = []
    temp_root: str | None = None
    try:
        if spec.jcode:
            temp_root = tempfile.mkdtemp(prefix="jcode-memory-bench-")
            env = os.environ.copy()
            if spec.env:
                env.update(spec.env)
            env["JCODE_HOME"] = os.path.join(temp_root, "home")
            env["JCODE_RUNTIME_DIR"] = os.path.join(temp_root, "run")
            env["JCODE_TEMP_SERVER"] = "1"
            env["JCODE_SERVER_OWNER_PID"] = str(os.getpid())
            os.makedirs(env["JCODE_HOME"], exist_ok=True)
            os.makedirs(env["JCODE_RUNTIME_DIR"], exist_ok=True)
            for auth_name in (
                "anthropic-auth.json",
                "openai-auth.json",
                "antigravity_oauth.json",
                "gemini_oauth.json",
                "config.toml",
            ):
                real_auth = Path.home() / ".jcode" / auth_name
                bench_auth = Path(env["JCODE_HOME"]) / auth_name
                if real_auth.exists() and not bench_auth.exists():
                    bench_auth.symlink_to(real_auth)
            if spec.name == "jcode_memory_on":
                real_models = Path.home() / ".jcode" / "models"
                bench_models = Path(env["JCODE_HOME"]) / "models"
                if real_models.exists() and not bench_models.exists():
                    bench_models.symlink_to(real_models)
            socket_path = os.path.join(env["JCODE_RUNTIME_DIR"], "bench.sock")
            server_proc = subprocess.Popen(
                [
                    spec.argv[0],
                    "--no-update",
                    "--no-selfdev",
                    "serve",
                    "--socket",
                    socket_path,
                ],
                cwd=str(cwd),
                env=env,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                preexec_fn=os.setsid,
            )
            cleanup_pgids.append(os.getpgid(server_proc.pid))
            if not wait_for_socket(socket_path, timeout_s):
                raise RuntimeError("jcode server did not become ready")
            if spec.name == "jcode_memory_on":
                time.sleep(max(settle_s, 5.0))
            per_session_settle = (
                max(settle_s, 2.0) if spec.name == "jcode_memory_on" else settle_s
            )
            for _ in range(sessions):
                launches.append(
                    launch_interactive(
                        [
                            spec.argv[0],
                            "--no-update",
                            "--no-selfdev",
                            "--socket",
                            socket_path,
                        ],
                        cwd,
                        env,
                        timeout_s,
                        per_session_settle,
                    )
                )
                cleanup_pgids.append(launches[-1].pgid)
            root_pids = [server_proc.pid] + [launch.root_pid for launch in launches]
            sample_pgids = cleanup_pgids.copy()
        else:
            env = os.environ.copy()
            if spec.env:
                env.update(spec.env)
            for _ in range(sessions):
                launches.append(
                    launch_interactive(spec.argv, cwd, env, timeout_s, settle_s)
                )
                cleanup_pgids.append(launches[-1].pgid)
            root_pids = [launch.root_pid for launch in launches]
            sample_pgids = cleanup_pgids.copy()

        for idx, launch in enumerate(launches, start=1):
            if not launch.ready:
                notes.append(
                    f"session {idx}: no meaningful screen content before timeout"
                )
            elif launch.excerpt:
                notes.append(f"session {idx}: {launch.excerpt}")
        pss_mb, process_count = sum_tree_pss(root_pids, sample_pgids)
        return ToolRunResult(
            tool=spec.name,
            sessions=sessions,
            pss_mb=pss_mb,
            process_count=process_count,
            version=version,
            notes=notes,
        )
    finally:
        for launch in launches:
            try:
                os.close(launch.master_fd)
            except Exception:
                pass
        for pgid in reversed(cleanup_pgids):
            terminate_pgroup(pgid)
        if temp_root:
            shutil.rmtree(temp_root, ignore_errors=True)


def _runtime_memory_attribution(
    log_root: Path, *, expected_population: int
) -> tuple[int, float]:
    module_path = Path(__file__).with_name("analyze_runtime_memory_log.py")
    spec = importlib.util.spec_from_file_location(
        "jcode_runtime_memory_analyzer", module_path
    )
    if spec is None or spec.loader is None:
        raise RuntimeError("unable to load runtime memory attribution parser")
    analyzer = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = analyzer
    spec.loader.exec_module(analyzer)

    paths = sorted(log_root.rglob("*runtime-memory-*.jsonl"))
    samples = [
        sample for sample in analyzer.load_samples(paths) if sample.target == "server"
    ]
    if not samples:
        raise RuntimeError(
            "private daemon emitted no runtime memory attribution samples"
        )
    try:
        evidence = analyzer.build_scaling_evidence(
            samples, expected_population=expected_population
        )
    except ValueError as error:
        raise RuntimeError(str(error)) from error
    return evidence["observed_population"], round(
        evidence["attributed_session_bytes"] / (1024 * 1024), 3
    )


def wait_for_population_attribution(
    log_root: Path,
    *,
    expected_population: int,
    timeout_s: float = 20.0,
    poll_interval_s: float = 0.25,
) -> tuple[int, float]:
    deadline = time.monotonic() + timeout_s
    last_observed: tuple[int, float] | None = None
    last_error: RuntimeError | None = None
    while time.monotonic() < deadline:
        try:
            last_observed = _runtime_memory_attribution(
                log_root, expected_population=expected_population
            )
            if last_observed[0] == expected_population:
                return last_observed
        except RuntimeError as error:
            last_error = error
        time.sleep(poll_interval_s)
    if last_observed is None and last_error is not None:
        raise last_error
    raise RuntimeError(
        "runtime memory attribution population did not settle: "
        f"expected {expected_population}, observed {last_observed}"
    )


def scaling_environment(temp_root: str) -> dict[str, str]:
    env = os.environ.copy()
    env.update(
        {
            "JCODE_HOME": os.path.join(temp_root, "home"),
            "JCODE_RUNTIME_DIR": os.path.join(temp_root, "run"),
            "JCODE_TEMP_SERVER": "1",
            "JCODE_SERVER_OWNER_PID": str(os.getpid()),
            "JCODE_NO_TELEMETRY": "1",
            "JCODE_DEBUG_CONTROL": "1",
            "JCODE_MEMORY_ENABLED": "1",
            "JCODE_RUNTIME_MEMORY_LOG_PROCESS_INTERVAL_SECS": "15",
            "JCODE_RUNTIME_MEMORY_LOG_ATTRIBUTION_INTERVAL_SECS": "60",
            "JCODE_RUNTIME_MEMORY_LOG_ATTRIBUTION_MIN_SPACING_SECS": "5",
            "JCODE_RUNTIME_MEMORY_LOG_EVENT_PROCESS_MIN_SPACING_SECS": "1",
        }
    )
    return env


def run_population_trial(
    *,
    binary: Path,
    cwd: Path,
    population: int,
    trial: int,
    timeout_s: float = DEFAULT_TIMEOUT_S,
) -> dict[str, object]:
    temp_root = Path(tempfile.mkdtemp(prefix="jcode-memory-scaling-"))
    env = scaling_environment(str(temp_root))
    Path(env["JCODE_HOME"]).mkdir(parents=True)
    Path(env["JCODE_RUNTIME_DIR"]).mkdir(parents=True)
    socket_path = os.path.join(env["JCODE_RUNTIME_DIR"], "scaling.sock")
    launches: list[SessionLaunch] = []
    owned_processes: list[budget.OwnedProcess] = []
    result: dict[str, object] = {
        "status": "invalid",
        "population": population,
        "observed_population": None,
        "trial": trial,
        "daemon_rss_mib": None,
        "attributed_session_mib": None,
        "failure": {
            "kind": "collection_failed",
            "diagnostic": "population trial did not complete",
        },
    }
    try:
        server = subprocess.Popen(
            [
                str(binary),
                "--no-update",
                "--no-selfdev",
                "serve",
                "--socket",
                socket_path,
            ],
            cwd=str(cwd),
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            preexec_fn=os.setsid,
        )
        owned_processes.append(budget.OwnedProcess.capture(server.pid))
        if not wait_for_socket(socket_path, timeout_s):
            raise RuntimeError("private daemon did not become ready")
        debug_socket_path = Path(socket_path).with_name("scaling-debug.sock")
        if not wait_for_socket(str(debug_socket_path), timeout_s):
            raise RuntimeError("private debug socket did not become ready")
        # The daemon writes an initial attribution sample at startup. Wait beyond
        # the supported event spacing so the first session event is not coalesced.
        time.sleep(ATTRIBUTION_SETTLE_S)
        for _ in range(population):
            session_id = create_debug_session(
                socket_path=debug_socket_path,
                cwd=cwd,
                timeout_s=timeout_s,
            )
            launch = launch_interactive(
                [
                    str(binary),
                    "--no-update",
                    "--no-selfdev",
                    "--socket",
                    socket_path,
                    "--resume",
                    session_id,
                ],
                cwd,
                env,
                min(timeout_s, 2.0),
                0.0,
            )
            launches.append(launch)
            owned_processes.append(budget.OwnedProcess.capture(launch.root_pid))
            if not launch.ready:
                raise RuntimeError("private resumed client did not become ready")
            time.sleep(ATTRIBUTION_SETTLE_S)
        time.sleep(IDLE_SETTLE_S)
        daemon_rss_mib = _read_proc_rss_mib(server.pid)
        observed_population, attributed_session_mib = wait_for_population_attribution(
            Path(env["JCODE_HOME"]),
            expected_population=population,
            timeout_s=timeout_s,
        )
        result = {
            "status": "valid",
            "population": population,
            "observed_population": observed_population,
            "trial": trial,
            "daemon_rss_mib": round(daemon_rss_mib, 3),
            "attributed_session_mib": attributed_session_mib,
            "failure": None,
        }
    except Exception as error:
        result = {
            "status": "invalid",
            "population": population,
            "observed_population": None,
            "trial": trial,
            "daemon_rss_mib": None,
            "attributed_session_mib": None,
            "failure": {"kind": "collection_failed", "diagnostic": str(error)},
        }
    finally:
        for launch in launches:
            try:
                os.close(launch.master_fd)
            except OSError:
                pass
        cleanup_diagnostics: list[str] = []
        try:
            cleanup = budget.cleanup_owned_processes(owned_processes)
            cleanup_diagnostics.extend(cleanup.diagnostics)
            all_stopped = cleanup.all_stopped
        except Exception as error:
            all_stopped = False
            cleanup_diagnostics.append(f"owned-process cleanup failed: {error}")
        try:
            shutil.rmtree(temp_root)
        except FileNotFoundError:
            pass
        except OSError as error:
            cleanup_diagnostics.append(f"private-path cleanup failed: {error}")
        private_paths_removed = not temp_root.exists()
    result["cleanup"] = {
        "status": (
            "complete" if all_stopped and private_paths_removed else "incomplete"
        ),
        "owned_processes_terminated": len(owned_processes),
        "private_paths_removed": private_paths_removed,
        "diagnostics": cleanup_diagnostics,
    }
    if result["cleanup"]["status"] != "complete" and result["status"] == "valid":
        result["status"] = "invalid"
        result["failure"] = {
            "kind": "cleanup_failed",
            "diagnostic": "; ".join(cleanup_diagnostics)
            or "private runtime cleanup was incomplete",
        }
    return result


def collect_session_scaling(
    *,
    binary: Path,
    cwd: Path,
    platform_name: str | None = None,
) -> dict[str, object]:
    platform_name = platform_name or platform.system()
    sampling = {
        "populations": list(SCALING_POPULATIONS),
        "trials_per_population": SCALING_TRIALS,
    }
    if platform_name != "Linux":
        return {
            "status": "unsupported",
            "platform": platform_name,
            "sampling": sampling,
            "trials": [],
            "incremental_mib_per_session_samples": [],
            "median_incremental_mib_per_session": None,
            "failures": [
                {"kind": "unsupported", "diagnostic": "Linux procfs is required"}
            ],
        }
    binary = binary.resolve()
    if not binary.is_file() or not os.access(binary, os.X_OK):
        return {
            "status": "invalid",
            "platform": platform_name,
            "sampling": sampling,
            "trials": [],
            "incremental_mib_per_session_samples": [],
            "median_incremental_mib_per_session": None,
            "failures": [
                {"kind": "invalid_binary", "diagnostic": f"not executable: {binary}"}
            ],
        }

    trials = [
        run_population_trial(
            binary=binary,
            cwd=cwd,
            population=population,
            trial=trial,
        )
        for trial in range(1, SCALING_TRIALS + 1)
        for population in SCALING_POPULATIONS
    ]
    failures: list[dict[str, object]] = []
    for evidence in trials:
        requested = evidence.get("population")
        observed = evidence.get("observed_population")
        if evidence.get("status") != "valid":
            failures.append(
                {
                    "kind": "trial_invalid",
                    "population": requested,
                    "trial": evidence.get("trial"),
                    "failure": evidence.get("failure"),
                }
            )
        elif observed != requested:
            failures.append(
                {
                    "kind": "population_mismatch",
                    "requested_population": requested,
                    "observed_population": observed,
                    "trial": evidence.get("trial"),
                }
            )
        elif not isinstance(
            evidence.get("daemon_rss_mib"), int | float
        ) or not isinstance(evidence.get("attributed_session_mib"), int | float):
            failures.append(
                {
                    "kind": "incomplete_evidence",
                    "population": requested,
                    "trial": evidence.get("trial"),
                }
            )
        elif (evidence.get("cleanup") or {}).get("status") != "complete":
            failures.append(
                {
                    "kind": "cleanup_incomplete",
                    "population": requested,
                    "trial": evidence.get("trial"),
                }
            )

    slopes: list[float] = []
    if not failures:
        for trial in range(1, SCALING_TRIALS + 1):
            by_population = {
                int(evidence["population"]): float(evidence["attributed_session_mib"])
                for evidence in trials
                if evidence["trial"] == trial
            }
            slopes.append(
                round(
                    (
                        by_population[SCALING_POPULATIONS[-1]]
                        - by_population[SCALING_POPULATIONS[0]]
                    )
                    / (SCALING_POPULATIONS[-1] - SCALING_POPULATIONS[0]),
                    3,
                )
            )
    return {
        "status": "invalid" if failures else "valid",
        "platform": platform_name,
        "sampling": sampling,
        "trials": trials,
        "incremental_mib_per_session_samples": slopes,
        "median_incremental_mib_per_session": statistics.median(slopes)
        if slopes
        else None,
        "failures": failures,
    }


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Benchmark interactive CLI memory using process-tree PSS"
    )
    parser.add_argument("--sessions", type=int, required=True)
    parser.add_argument("--tools", nargs="*", default=DEFAULT_TOOLS)
    parser.add_argument("--timeout", type=float, default=DEFAULT_TIMEOUT_S)
    parser.add_argument("--settle", type=float, default=DEFAULT_SETTLE_S)
    parser.add_argument("--cwd", default=os.getcwd())
    parser.add_argument("--json-out", default=None)
    args = parser.parse_args()

    specs = build_specs()
    cwd = Path(args.cwd).resolve()
    results = []
    for name in args.tools:
        spec = specs[name]
        print(
            f"=== {name} ({args.sessions} session{'s' if args.sessions != 1 else ''}) ===",
            flush=True,
        )
        result = run_tool(spec, args.sessions, cwd, args.timeout, args.settle)
        print(json.dumps(asdict(result), indent=2), flush=True)
        results.append(asdict(result))
    payload = {"cwd": str(cwd), "sessions": args.sessions, "results": results}
    if args.json_out:
        Path(args.json_out).write_text(json.dumps(payload, indent=2))
    else:
        print(json.dumps(payload, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
