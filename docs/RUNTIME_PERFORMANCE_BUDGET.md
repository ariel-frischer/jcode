# Runtime Performance Budget

Status: maintained contract
Updated: 2026-08-22

This document defines the runtime evidence required for changes that can affect
Jcode client responsiveness or the shared daemon. The normal product remains one
authoritative daemon serving multiple clients. Benchmarking observes that model
through private state and transport; it does not introduce a benchmark-only
runtime or use the active shared daemon.

## Canonical full report

Build the candidate first, install the pinned terminal-rendering dependency in
the Python environment used for the benchmark, and pass the candidate as an
absolute, fully resolved path:

```bash
python3 -m pip install -r scripts/requirements-runtime-benchmarks.txt
cargo build --profile selfdev
python3 scripts/bench_runtime_budgets.py collect \
  --binary "$(realpath ./target/selfdev/jcode)" \
  --output /tmp/jcode-runtime-report.json
```

`scripts/bench_runtime_budgets.py collect` is the only canonical command for a
complete report. It composes the owning collectors listed below and writes one
schema-versioned JSON model; the human summary is rendered from that same
validated model. Direct collector commands remain useful for diagnosis, but
their separate output is not a complete runtime-budget result.

The runner must create a private `JCODE_HOME`, `JCODE_RUNTIME_DIR`, and unique
socket for each scenario. It must verify the requested candidate before
collection, the executable of every measured private daemon, socket ownership,
and the candidate fingerprint again after collection. It may stop and remove
only processes and paths it created. It must never connect to, reload, or stop
the active shared daemon or its sessions.

The full report performs no provider or network requests. Protocol and tool
measurements use deterministic local operations through the private daemon.

## Maintained metric inventory

The report contains seven metric groups and nine stable metric IDs. Every
recorded sample is retained. Medians use the ordinary sample median;
`nearest_rank_p95` sorts the samples and selects rank `ceil(0.95 * n)` without
interpolation. Warm-ups are excluded from the recorded sample set.

| Group and metric IDs | Definition | Exact sampling and aggregation | Policy | Owning surface |
|---|---|---|---|---|
| First visible: `first_visible_ms` | Elapsed time from client launch to the first meaningful rendered PTY content, excluding terminal-control traffic. | One unrecorded warm-up followed by 10 recorded runs paired with input-ready collection; median and nearest-rank p95. | Same-environment comparison. Either aggregate is `review_required` only when it is strictly above the baseline by more than `max(15%, 20 ms)`. | [`scripts/bench_startup_visible_ready.py`](../scripts/bench_startup_visible_ready.py), composed by the canonical runner. |
| Input ready: `input_ready_ms` | Elapsed time from client launch until a typed readiness probe is visibly accepted by the rendered client. | The same one warm-up and 10 recorded runs as first-visible collection; median and nearest-rank p95. | Same-environment comparison. Either aggregate is `review_required` only when it is strictly above the baseline by more than `max(15%, 20 ms)`. | [`scripts/bench_startup_visible_ready.py`](../scripts/bench_startup_visible_ready.py), composed by the canonical runner. |
| Private daemon readiness: `daemon_ready_ms` | Elapsed time from private daemon launch until its isolated local socket accepts a connection. | Five recorded launches; five-second timeout per attempt; median. | Deterministic gate: median must be at most 80 ms. A value strictly above 80 ms is `deterministic_failure`. | [`scripts/bench_startup.py`](../scripts/bench_startup.py), using the isolated startup path. |
| Idle daemon resources: `idle_cpu_percent`, `idle_rss_mib` | CPU utilization in percentage points and resident memory in MiB for a private isolated instance of the shared-daemon runtime after startup work settles. | Wait five seconds after readiness, then take five samples at one-second intervals; median for each metric. | Same-environment comparison. CPU is `review_required` above baseline by more than 0.5 percentage points. RSS is `review_required` above baseline by more than `max(10%, 8 MiB)`. | [`scripts/bench_memory_cli.py`](../scripts/bench_memory_cli.py), reusing `/proc` sampling from [`scripts/profile_spawn.py`](../scripts/profile_spawn.py). |
| Session memory scaling: `session_scaling_mib_per_session` | Incremental MiB per added settled session while one private shared daemon serves the population. Evidence includes daemon RSS and attributable session memory at each population. | Populations of 1, 4, and 8 equivalent clients; three trials per population; retain population evidence and aggregate the three incremental-slope samples by median. | Same-environment comparison. The slope is `review_required` above baseline by more than `max(15%, 2 MiB/session)`. | [`scripts/bench_memory_cli.py`](../scripts/bench_memory_cli.py), with attribution from [`scripts/analyze_runtime_memory_log.py`](../scripts/analyze_runtime_memory_log.py). |
| Frame/update work: `frame_update_work_count` | Deterministic work performed while rebuilding unchanged steady-state desktop frames across the maintained state space. | One complete state-space sweep; exact work counts. Cold and warm timings remain diagnostic evidence, while unchanged-frame work is the portable invariant. | Deterministic gate: an unchanged frame performs zero transcript relayouts, and maintained exact work-count limits may not grow silently. The existing guardrail also retains its loose 40 ms catastrophic warm-frame gate; the 4 ms timing budget is advisory, not a machine-independent ratchet. | [`scripts/check_guardrails.sh`](../scripts/check_guardrails.sh) runs `jcode-desktop2`'s `profile::` state-space tests in [`crates/jcode-desktop2/src/profile.rs`](../crates/jcode-desktop2/src/profile.rs). |
| Local round trips: `protocol_round_trip_ms`, `tool_round_trip_ms` | A versioned local socket handshake or control exchange, and one deterministic local tool request, through the private shared daemon. Both include real cross-process serialization and dispatch; neither includes provider or network latency. | Each case performs five unrecorded warm-ups followed by 30 recorded operations; median and nearest-rank p95 for each metric. | Same-environment comparison. Either aggregate is `review_required` only when strictly above baseline by more than `max(15%, 1 ms)`. | [`scripts/bench_runtime_budgets.py`](../scripts/bench_runtime_budgets.py), reusing the existing local protocol/harness and tool request contracts rather than terminal-output parsing. |

Only values strictly above a ceiling or tolerance regress. A value exactly on
the boundary remains within budget. Missing samples, partial populations,
timeouts, nonzero collector exits, malformed evidence, and identity failures
invalidate the affected result instead of being aggregated into a pass.

## Report provenance

An acceptable report records enough information to prove what ran and whether
the comparison is meaningful:

- schema version and the complete canonical metric definitions;
- requested and resolved candidate paths, version or revision, SHA-256, and
  freshness evidence;
- private daemon PID, resolved executable, socket, and verified ownership;
- operating system, architecture, relevant machine identity, Python and Rust
  toolchain/profile, collector versions, and command parameters;
- command argv and start time, every raw sample, required aggregates,
  diagnostics, and per-metric classification;
- baseline path and SHA-256 when comparing; and
- cleanup evidence showing owned processes stopped and private paths removed.

A baseline is comparable only when its schema, metric definitions, environment,
toolchain/profile, command parameters, sample counts, and aggregation rules
match the candidate report. Do not apply same-machine tolerances across
incompatible environments. Baseline creation and replacement are explicit,
reviewable file changes; collection and comparison never update a baseline.

### Reviewed Linux reference baseline

[`docs/runtime-performance-baselines/linux-reference.json`](./runtime-performance-baselines/linux-reference.json)
was created from `run-2.json` after reviewing three sequential complete reports
from the designated Linux host. The host was on AC power, no Cargo or rustc
process was active when collection began, the candidate SHA-256 was
`20d2d505a52285ecf96e6ff1d93cd6d62d4c78559decd2a516f824a28e4ed0bd`, and
the three reports ran from 2026-08-22 00:02 UTC through 00:26 UTC. Run 2 was
selected because its daemon-readiness median is the median of the three report
medians and its noisy metrics are representative rather than uniformly best.
The baseline file retains that report's complete raw samples, aggregates,
definitions, environment, toolchain, command, timestamp, and review reason.

| Metric aggregate | Run 1 | Run 2 (reference) | Run 3 |
|---|---:|---:|---:|
| daemon readiness median (ms) | 138.505 | 108.773 | 186.405 |
| first visible median (ms) | 14.955 | 14.795 | 29.210 |
| input ready median (ms) | 85.496 | 84.492 | 116.998 |
| idle CPU median (%) | 0.000 | 0.000 | 0.000 |
| idle RSS median (MiB) | 88.961 | 89.070 | 88.719 |
| session scaling median (MiB/session) | 0.001 | 0.001 | 0.001 |
| protocol round-trip median (ms) | 0.103 | 0.088 | 0.282 |
| tool round-trip median (ms) | 2.804 | 3.586 | 6.975 |
| unchanged-frame work count | 0 | 0 | 0 |

Collection classifies deterministic gates immediately. Comparing any of the
three reports with the reference therefore returns exit `4` because its
daemon-readiness median is above the maintained 80 ms ceiling. Run 3 also
requires review for first visible, input-ready, and tool-round-trip latency.
This checked-in reference records the current runtime state without waiving the
gate. The `baseline` command refuses any report with a deterministic failure, so
a replacement reference must pass every deterministic gate. A
developer-velocity change must preserve or improve these results, and the
daemon-readiness failure remains actionable follow-up work.

Create or replace a reviewed baseline only after repeated compatible reports:

```bash
python3 scripts/bench_runtime_budgets.py baseline \
  --report /path/to/reviewed-report.json \
  --output docs/runtime-performance-baselines/linux-reference.json \
  --reason "reviewed reason tied to repeated reports"
```

Replacement additionally requires `--overwrite`. Reviewers must inspect the
baseline diff, the repeated source reports, environmental compatibility, and
every deterministic or noisy regression before approving it.

## Classifications and delivery action

Every metric has exactly one classification:

| Classification | Meaning and required action |
|---|---|
| `pass` | Complete, compatible evidence is within its maintained gate or tolerance. |
| `review_required` | A noisy metric is strictly outside its same-environment tolerance. Provide before/after evidence and a written explanation, then fix the regression or obtain explicit maintainer approval for a visible baseline change. |
| `deterministic_failure` | An exact or low-variance gate failed. Fix it or obtain explicit approval for an intentional reviewed ratchet; do not weaken the check locally. |
| `invalid` | Evidence is incomplete, malformed, timed out, failed, stale, mismatched, or unsafe. Correct the measurement and rerun it; invalid evidence is never a pass. |
| `unsupported` | The platform cannot supply the required measurement. Preserve actionable platform diagnostics; do not fabricate samples or translate this outcome into `pass`. |

The CLI reserves distinct exit categories: `0` pass, `2` usage, `3`
review-required, `4` deterministic failure, `5` invalid, and `6` unsupported.
Linux is the canonical environment for PTYs, Unix sockets, `/proc` CPU/RSS/PSS,
and executable-process identity. Other platforms may report individual metrics
as unsupported without changing normal Jcode behavior. A complete canonical
Linux report treats missing required platform support as a nonzero outcome.

## Review rule for developer-velocity work

Runtime performance is the primary constraint on build, reload, validation,
self-development, and other developer-velocity changes. A change touching an
applicable startup, daemon, memory, frame, protocol, or tool path must include
before/after evidence from this contract. It may proceed only when the relevant
budget is unchanged or improved, unless the regression is explicitly disclosed,
explained, approved by a maintainer, and represented by a reviewed baseline or
ratchet diff. A faster development loop never silently buys slower client or
daemon runtime behavior.

This contract implements the measurement obligations in constitution
PRIN-002, PRIN-004, PRIN-005, PRIN-007, and PRIN-010. The existing
[memory regression budget](./MEMORY_BUDGET.md) remains the owner of cache caps
and memory-attribution expectations; this document owns reproducible runtime
resource measurements and candidate-versus-baseline delivery decisions.
