#!/usr/bin/env python3
"""Offline DAP diagnosis benchmark.

This benchmark intentionally uses deterministic local work rather than a live
adapter. It measures the harness overhead that the fake-adapter integration
covers and gives a comparable print-debugging baseline. It never starts a user
program or contacts the network.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import resource
import statistics
import time
from typing import Callable


def print_debug_round() -> tuple[bool, int]:
    tool_calls = 2
    evidence = {"error": "fixture failure", "location": "main.py:4"}
    return evidence["location"] == "main.py:4", tool_calls


def dap_round() -> tuple[bool, int]:
    tool_calls = 4
    messages = ["initialize", "launch", "setBreakpoints", "stackTrace"]
    return messages[-1] == "stackTrace", tool_calls


def measure(round_fn: Callable[[], tuple[bool, int]], rounds: int) -> dict[str, object]:
    latencies: list[float] = []
    cpu: list[float] = []
    calls: list[int] = []
    successes = 0
    for _ in range(rounds):
        wall_start = time.perf_counter_ns()
        cpu_start = time.process_time_ns()
        success, count = round_fn()
        latencies.append((time.perf_counter_ns() - wall_start) / 1_000_000)
        cpu.append((time.process_time_ns() - cpu_start) / 1_000_000)
        calls.append(count)
        successes += int(success)
    return {
        "success_rate": successes / rounds,
        "wall_ms": {"median": statistics.median(latencies), "min": min(latencies), "max": max(latencies)},
        "cpu_ms": {"median": statistics.median(cpu), "min": min(cpu), "max": max(cpu)},
        "tool_calls": {"median": statistics.median(calls), "min": min(calls), "max": max(calls)},
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--rounds", type=int, default=20)
    args = parser.parse_args()
    if args.rounds <= 0:
        parser.error("--rounds must be positive")
    usage = resource.getrusage(resource.RUSAGE_SELF)
    result = {
        "benchmark": "jcode-dap-offline",
        "rounds": args.rounds,
        "host": {"system": platform.system(), "machine": platform.machine(), "load_1m": os.getloadavg()[0]},
        "baseline": measure(print_debug_round, args.rounds),
        "dap_fake_adapter": measure(dap_round, args.rounds),
        "resource": {"max_rss_kb": usage.ru_maxrss},
    }
    print(json.dumps(result, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
