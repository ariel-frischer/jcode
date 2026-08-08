#!/usr/bin/env python3
"""Run randomized CPU, memory, and latency profiles for common Jcode tools."""

from __future__ import annotations

import argparse
import json
import os
import random
import statistics
import subprocess
import time
from pathlib import Path


CASES = [
    "ls_root",
    "read_tiny",
    "read_large_head",
    "read_large_tail",
    "legacy_read_large_head",
    "legacy_read_large_tail",
    "agentgrep_grep",
    "agentgrep_find",
    "bash_true",
    "bash_output_64k",
    "bash_background_start",
    "bg_list",
    "batch_read_4",
    "sequential_read_4",
]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--repo", type=Path, default=Path.cwd())
    parser.add_argument("--rounds", type=int, default=5)
    parser.add_argument("--iterations", type=int, default=10)
    parser.add_argument("--large-iterations", type=int, default=4)
    parser.add_argument("--seed", type=int, default=8100)
    parser.add_argument("--output", type=Path)
    parser.add_argument(
        "--include-hooks",
        action="store_true",
        help="Include configured pre/post tool hooks. Disabled by default for core attribution.",
    )
    return parser.parse_args()


def make_fixtures(root: Path) -> Path:
    fixture_dir = root / "jcode-tool-profile-fixtures"
    fixture_dir.mkdir(parents=True, exist_ok=True)
    (fixture_dir / "tiny.txt").write_text("hello world\nsecond line\n")
    large = fixture_dir / "large.txt"
    expected_size = 113_000_000
    if not large.exists() or large.stat().st_size != expected_size:
        line = ("0123456789abcdef" * 7 + "\n").encode()
        with large.open("wb") as handle:
            for _ in range(1_000_000):
                handle.write(line)
    return fixture_dir


def median_summary(rows: list[dict]) -> dict[str, dict]:
    result: dict[str, dict] = {}
    for case in CASES:
        selected = [row for row in rows if row["case"] == case]
        wall = [row["wall_us"]["p50"] / 1000 for row in selected]
        cpu = [row["cpu_total_us_per_iteration"] / 1000 for row in selected]
        high_water = [row["high_water_kib_delta"] / 1024 for row in selected]
        load_averages = [
            value
            for row in selected
            for value in (
                row["load_average_1m_before"],
                row["load_average_1m_after"],
            )
        ]
        result[case] = {
            "wall_p50_ms_median": statistics.median(wall),
            "wall_p50_ms_min": min(wall),
            "wall_p50_ms_max": max(wall),
            "cpu_ms_median": statistics.median(cpu),
            "cpu_ms_min": min(cpu),
            "cpu_ms_max": max(cpu),
            "high_water_delta_mib_median": statistics.median(high_water),
            "high_water_delta_mib_min": min(high_water),
            "high_water_delta_mib_max": max(high_water),
            "load_average_1m_min": min(load_averages),
            "load_average_1m_max": max(load_averages),
        }
    return result


def main() -> int:
    args = parse_args()
    if args.rounds < 1 or args.iterations < 1 or args.large_iterations < 1:
        raise SystemExit("round and iteration counts must be positive")

    repo = args.repo.resolve()
    binary = args.binary.resolve()
    scratch = Path(os.environ.get("JCODE_SCRATCH_DIR", Path.home() / ".jcode" / "scratch"))
    fixtures = make_fixtures(scratch)
    output = args.output or scratch / "jcode-tool-profile-results.json"
    env = os.environ.copy()
    if not args.include_hooks:
        env["JCODE_HOOKS_DISABLED"] = "1"

    rows: list[dict] = []
    total = args.rounds * len(CASES)
    completed = 0
    for round_number in range(1, args.rounds + 1):
        order = CASES.copy()
        random.Random(args.seed + round_number).shuffle(order)
        for case in order:
            iterations = args.large_iterations if "large" in case else args.iterations
            command = [
                str(binary),
                "--case",
                case,
                "--iterations",
                str(iterations),
                "--repo",
                str(repo),
                "--fixture-dir",
                str(fixtures),
            ]
            process = subprocess.run(
                command,
                env=env,
                text=True,
                capture_output=True,
                timeout=300,
                check=False,
            )
            if process.returncode != 0:
                raise SystemExit(
                    f"{case} failed with exit {process.returncode}:\n"
                    f"{process.stderr}\n{process.stdout}"
                )
            row = json.loads(process.stdout)
            row["round"] = round_number
            rows.append(row)
            completed += 1
            print(
                "JCODE_PROGRESS "
                + json.dumps(
                    {
                        "current": completed,
                        "total": total,
                        "unit": "cases",
                        "message": f"round {round_number} {case}",
                    }
                ),
                flush=True,
            )
            time.sleep(0.05)

    summary = median_summary(rows)
    payload = {
        "binary": str(binary),
        "repo": str(repo),
        "hooks_included": args.include_hooks,
        "rounds": args.rounds,
        "iterations": args.iterations,
        "large_iterations": args.large_iterations,
        "seed": args.seed,
        "rows": rows,
        "summary": summary,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(payload, indent=2) + "\n")

    for case in CASES:
        item = summary[case]
        print(
            f"{case:24} "
            f"wall={item['wall_p50_ms_median']:8.3f}ms "
            f"[{item['wall_p50_ms_min']:.3f},{item['wall_p50_ms_max']:.3f}] "
            f"cpu={item['cpu_ms_median']:8.3f}ms "
            f"hwm={item['high_water_delta_mib_median']:7.2f}MiB"
        )
    print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
