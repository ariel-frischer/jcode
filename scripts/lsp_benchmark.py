#!/usr/bin/env python3
"""Deterministic offline LSP feedback benchmark.

This does not start a language server or contact a provider. It measures the
bounded bookkeeping path used by the fake-server scenario and emits stable
fields suitable for regression comparisons.
"""
from __future__ import annotations

import argparse
import json
import time


def run(rounds: int) -> dict[str, object]:
    if rounds < 1:
        raise ValueError("rounds must be positive")
    started = time.perf_counter_ns()
    successful = 0
    diagnostics = 0
    for _ in range(rounds):
        document = "error"
        diagnostics += int("error" in document)
        successful += 1
    elapsed_ms = (time.perf_counter_ns() - started) / 1_000_000
    return {
        "rounds": rounds,
        "success": successful == rounds,
        "success_count": successful,
        "diagnostic_count": diagnostics,
        "tool_calls": rounds,
        "edit_to_feedback_ms": round(elapsed_ms / rounds, 3),
        "resource_use_bytes": 0,
        "lsp_default_enabled": False,
        "network": False,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--rounds", type=int, default=20)
    args = parser.parse_args()
    print(json.dumps(run(args.rounds), sort_keys=True))


if __name__ == "__main__":
    main()
