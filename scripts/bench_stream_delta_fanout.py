#!/usr/bin/env python3
"""Deterministic measurement harness for jcode-ms5 streaming fanout.

This harness is intentionally independent of Cargo. It generates fixed event
fixtures and can measure a supplied baseline/candidate executable that accepts
--fixture PATH --mode NAME. The executable is expected to emit one JSON object
per output event on stdout; the harness verifies the exact oracle digest.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import resource
import subprocess
import tempfile
import time
import tracemalloc
from pathlib import Path

SCENARIOS = {
    "small-token": [
        {"kind": "text", "text": "Hello"},
        {"kind": "text", "text": " world"},
        {"kind": "message_end", "stop_reason": "end_turn"},
    ],
    "long-token": [
        {"kind": "text", "text": f"token-{i:05d} "} for i in range(10_000)
    ] + [{"kind": "message_end", "stop_reason": "end_turn"}],
    "multibyte": [
        {"kind": "thinking_start"},
        {"kind": "thinking", "text": "推論🙂 café"},
        {"kind": "thinking_end"},
        {"kind": "text", "text": "こんにちは世界 🌍"},
        {"kind": "message_end", "stop_reason": "end_turn"},
    ],
    "tool-call": [
        {"kind": "thinking_start"},
        {"kind": "thinking", "text": "checking"},
        {"kind": "thinking_end"},
        {"kind": "tool_start", "id": "tool-1", "name": "read"},
        {"kind": "tool_input", "delta": "{\"file_path\":"},
        {"kind": "tool_input", "delta": "\"README.md\"}"},
        {"kind": "tool_end"},
        {"kind": "message_end", "stop_reason": "tool_use"},
    ],
    "interruption-error": [
        {"kind": "text", "text": "partial "},
        {"kind": "error", "message": "fixture interruption", "retry_after_secs": None},
    ],
    "slow-consumer": [
        {"kind": "text", "text": f"chunk-{i:04d}"} for i in range(2_000)
    ] + [{"kind": "message_end", "stop_reason": "end_turn"}],
}


def digest(events: list[dict]) -> str:
    encoded = "".join(json.dumps(event, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n" for event in events)
    return hashlib.sha256(encoded.encode("utf-8")).hexdigest()


def write_fixtures(directory: Path) -> dict[str, dict]:
    result = {}
    for name, events in SCENARIOS.items():
        path = directory / f"{name}.json"
        payload = {"scenario": name, "events": events, "oracle_digest": digest(events)}
        path.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        result[name] = {"path": str(path), "input_events": len(events), "oracle_digest": payload["oracle_digest"]}
    return result


def measure(command: list[str], fixture: Path, mode: str) -> dict:
    usage_before = resource.getrusage(resource.RUSAGE_CHILDREN)
    start = time.perf_counter_ns()
    with tempfile.NamedTemporaryFile(prefix="jcode-ms5-metrics-", suffix=".json", delete=False) as metrics:
        metrics_path = Path(metrics.name)
    try:
        proc = subprocess.run(
            command + ["--fixture", str(fixture), "--mode", mode, "--metrics", str(metrics_path)],
            capture_output=True, text=True, check=False,
        )
        worker_metrics = json.loads(metrics_path.read_text(encoding="utf-8")) if metrics_path.exists() and metrics_path.stat().st_size else {}
    finally:
        metrics_path.unlink(missing_ok=True)
    elapsed_ns = time.perf_counter_ns() - start
    usage_after = resource.getrusage(resource.RUSAGE_CHILDREN)
    outputs = [json.loads(line) for line in proc.stdout.splitlines() if line.strip()]
    expected = json.loads(fixture.read_text(encoding="utf-8"))
    return {
        "mode": mode,
        "returncode": proc.returncode,
        "wall_ms": elapsed_ns / 1_000_000,
        "child_cpu_ms": (usage_after.ru_utime - usage_before.ru_utime + usage_after.ru_stime - usage_before.ru_stime) * 1000,
        "child_max_rss_kb_delta": max(0, usage_after.ru_maxrss - usage_before.ru_maxrss),
        "allocations": worker_metrics.get("allocations"),
        "allocation_peak_bytes": worker_metrics.get("peak_bytes"),
        "allocation_measurement": "tracemalloc peak and allocation count from worker model; Rust runs may provide allocator-specific counters",
        "memory_measurement": "child max RSS delta from getrusage",
        "output_events": len(outputs),
        "output_digest": digest(outputs),
        "oracle_digest": expected["oracle_digest"],
        "parity": proc.returncode == 0 and digest(outputs) == expected["oracle_digest"],
        "stderr": proc.stderr[-2000:],
    }


def worker(fixture: Path, mode: str, metrics: Path | None) -> int:
    """Emit the exact fixture event stream for deterministic harness self-tests.

    The two models deliberately have identical semantics. The candidate avoids
    rebuilding the sorted JSON key representation on every event, which makes
    the harness useful for validating its measurement plumbing without implying
    that this Python model predicts Rust performance.
    """
    payload = json.loads(fixture.read_text(encoding="utf-8"))
    events = payload["events"]
    tracemalloc.start()
    if mode == "candidate":
        lines = [json.dumps(event, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n" for event in events]
        for line in lines:
            print(line, end="", flush=True)
    else:
        for event in events:
            print(json.dumps(event, ensure_ascii=False, sort_keys=True, separators=(",", ":")), flush=True)
    current, peak = tracemalloc.get_traced_memory()
    snapshot = tracemalloc.take_snapshot()
    allocations = len(snapshot.traces)
    if metrics:
        metrics.write_text(json.dumps({"allocations": allocations, "current_bytes": current, "peak_bytes": peak}) + "\n", encoding="utf-8")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--fixture-dir", type=Path)
    parser.add_argument("--command", nargs="+", help="Executable command accepting --fixture and --mode")
    parser.add_argument("--mode", default="baseline", choices=("baseline", "candidate"))
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--self-model", action="store_true", help="measure the deterministic Python model")
    parser.add_argument("--worker", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--fixture", type=Path, help=argparse.SUPPRESS)
    parser.add_argument("--metrics", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.worker:
        if not args.fixture:
            parser.error("--worker requires --fixture")
        return worker(args.fixture, args.mode, args.metrics)
    with tempfile.TemporaryDirectory(prefix="jcode-ms5-stream-") as owned:
        directory = args.fixture_dir or Path(owned)
        directory.mkdir(parents=True, exist_ok=True)
        fixtures = write_fixtures(directory)
        report = {"schema": 1, "scenarios": fixtures, "runs": [], "notes": [
            "The self-model is measurement-plumbing validation, not a Rust performance proxy.",
            "Candidate and baseline must have identical oracle digests before any performance comparison is considered.",
        ]}
        command = args.command
        if args.self_model:
            command = [os.fspath(Path(os.sys.executable)), os.fspath(Path(__file__).resolve()), "--worker"]
        if command:
            for name, metadata in fixtures.items():
                fixture = Path(metadata["path"])
                report["runs"].append({"scenario": name, "measurements": [measure(command, fixture, args.mode) for _ in range(args.runs)]})
        if args.self_model:
            report["parity_test"] = all(
                measurement["parity"]
                for run in report["runs"]
                for measurement in run["measurements"]
            )
        print(json.dumps(report, ensure_ascii=False, indent=2) if args.json else json.dumps(report, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
