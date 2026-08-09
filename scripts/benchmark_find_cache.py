#!/usr/bin/env python3
"""Compare forced-fresh and cached agentgrep-find profiles in identical processes."""
import argparse, json, os, statistics, subprocess
from pathlib import Path

def run(binary: Path, repo: Path, iterations: int, fresh: bool) -> dict:
    env = os.environ.copy()
    env["JCODE_HOOKS_DISABLED"] = "1"
    if fresh:
        env["JCODE_AGENTGREP_FIND_CACHE"] = "0"
    output = subprocess.check_output([
        str(binary), "--case", "agentgrep_find", "--iterations", str(iterations),
        "--repo", str(repo),
    ], env=env, text=True)
    return json.loads(output)

def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--repo", type=Path, default=Path.cwd())
    parser.add_argument("--rounds", type=int, default=5)
    parser.add_argument("--iterations", type=int, default=20)
    args = parser.parse_args()
    fresh = [run(args.binary.resolve(), args.repo.resolve(), args.iterations, True) for _ in range(args.rounds)]
    warm = [run(args.binary.resolve(), args.repo.resolve(), args.iterations, False) for _ in range(args.rounds)]
    def summary(rows):
        return {
            "wall_p50_ms": statistics.median(row["wall_us"]["p50"] / 1000 for row in rows),
            "cpu_ms": statistics.median(row["cpu_total_us_per_iteration"] / 1000 for row in rows),
            "rss_delta_kib": statistics.median(row["rss_kib_delta"] for row in rows),
            "hwm_delta_kib": statistics.median(row["high_water_kib_delta"] for row in rows),
        }
    result = {"binary": str(args.binary.resolve()), "fresh": summary(fresh), "warm_process": summary(warm)}
    result["warm_improvement_percent"] = 100 * (1 - result["warm_process"]["wall_p50_ms"] / result["fresh"]["wall_p50_ms"])
    print(json.dumps(result, indent=2))
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
