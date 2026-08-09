# Common Tool CPU and Memory Profile

Status: current-state measurement and implementation retry evidence
Measured: 2026-08-09
Bead: `jcode-znm` (portfolio baseline: `jcode-skr`)

## Scope

This profile measures common built-in tools through the production
`Registry::execute` path: `bash`, `bg`, `agentgrep`, `read`, `batch`, and `ls`.
The path retains tool policy checks, lifecycle logging, telemetry, output guards,
and the tools' normal validation and security gates.

The benchmark does not call a model, access the network, or alter other Jcode
processes. Multiple unrelated agents were active during measurement. To reduce
contention bias, cases ran in five deterministic randomized rounds and the table
reports the median of each round's steady-state p50. Per-process CPU includes
completed child processes, while wall-time ranges expose scheduling noise.

## Environment and method

- Host: AMD Ryzen AI 7 PRO 350, 27 GiB RAM, Linux x86_64
- Build: Rust `selfdev` profile, isolated worktree
- Fixture: 113,000,000-byte UTF-8 file with 1,000,000 lines
- Rounds: 5 randomized rounds
- Iterations per round: 10, or 4 for large-file cases
- Host one-minute load during the campaign: 8.06 to 9.35
- Hooks: disabled for core attribution with `JCODE_HOOKS_DISABLED=1`
- Memory: Linux `VmHWM` delta from before case warm-up through completion
- CPU: `getrusage(RUSAGE_SELF)` plus `RUSAGE_CHILDREN`

Run the same campaign after building the development binary:

```bash
cargo build --profile selfdev --features dev-bins --bin tool_profile
scripts/profile_tools.py \
  --binary target/selfdev/tool_profile \
  --repo . \
  --rounds 5
```

Use `--include-hooks` only when intentionally measuring locally configured hook
processes. Core attribution disables hooks because an arbitrary user hook can
otherwise dominate every tool's CPU cost.

## Results

| Case | Median p50 wall | Round range | CPU per call | HWM delta |
|---|---:|---:|---:|---:|
| `ls` repository root | 1.024 ms | 0.848-1.560 ms | 1.055 ms | 3.07 MiB |
| `read` tiny text | 0.125 ms | 0.100-0.135 ms | 0.120 ms | 2.46 MiB |
| `read` 113 MB, head | 43.877 ms | 42.892-46.596 ms | 43.423 ms | 2.59 MiB |
| `read` 113 MB, near tail | 50.434 ms | 41.049-86.064 ms | 49.949 ms | 2.59 MiB |
| legacy `read` 113 MB, head | 88.735 ms | 64.099-188.888 ms | 85.304 ms | 108.76 MiB |
| legacy `read` 113 MB, near tail | 70.522 ms | 64.169-82.698 ms | 69.985 ms | 108.67 MiB |
| `agentgrep grep` | 16.410 ms | 15.801-18.633 ms | 72.007 ms | 6.80 MiB |
| `agentgrep find` | 50.161 ms | 48.182-70.589 ms | 49.194 ms | 6.39 MiB |
| `bash true` | 1.802 ms | 1.065-2.385 ms | 1.904 ms | 3.45 MiB |
| `bash` 64 KiB output | 2.461 ms | 2.165-2.654 ms | 3.221 ms | 3.80 MiB |
| background `bash` start | 0.765 ms | 0.513-0.955 ms | 2.909 ms | 5.15 MiB |
| `bg list` | 2.802 ms | 2.339-3.180 ms | 2.930 ms | 3.94 MiB |
| `batch` four tiny reads | 0.419 ms | 0.406-0.447 ms | 0.498 ms | 3.44 MiB |
| four sequential tiny reads | 0.357 ms | 0.298-0.549 ms | 0.353 ms | 2.46 MiB |

The HWM figures include roughly 2.5-3.5 MiB of per-case harness and lazy runtime
initialization. The paired legacy/current comparison is therefore more useful
than treating a current HWM delta as file data retained by the tool.

`agentgrep grep` reports more CPU than wall time because the measurement adds
CPU consumed by completed child processes and ripgrep can use parallel workers.
This is not a timing inconsistency.

## Built-binary public-interface acceptance

The optimized implementation was also exercised outside the custom profiler
through a newly built `target/selfdev/jcode` binary and the real public control
path:

1. Start an isolated Jcode daemon with a private `XDG_RUNTIME_DIR`, `JCODE_HOME`,
   and socket.
2. Create a headless session with `jcode debug create_session`.
3. Invoke the tool with `jcode debug tool 'read {...}' --session <id>`.

Five calls reading 20 lines near the tail of the 113 MB fixture observed:

- CLI-to-daemon-to-registry median wall time: 51.57 ms
- Range returned: lines 900,000 through 900,019
- Exact remaining count: 99,981 lines
- Exact continuation hint: `start_line=900020`
- Tool output: 2,458 bytes

The final five-call wall range was 49.58 to 59.78 ms after review hardening. This includes launching the
short-lived debug CLI client, Unix-socket transport, daemon dispatch, registry
checks, tool execution, output guards, JSON serialization, and response parsing.
The isolated acceptance daemon was stopped through its private socket and verified
removed. No shared daemon or other agent process was inspected or modified.

## Implemented optimization: streamed text reads

The old text path used `tokio::fs::read_to_string`, retaining the complete file
while scanning every line for exact total-line and continuation metadata. The
new path scans fixed-size chunks on Tokio's blocking pool. A vectorized newline
search retains at most the permitted output prefix for each line, plus rendered
output, while still scanning to EOF and validating the complete UTF-8 stream.

Paired results in the same binary and campaign:

| Large-file case | Wall improvement | CPU improvement | HWM reduction |
|---|---:|---:|---:|
| First 20 lines | 50.6% | 49.4% | 108.45 to 3.27 MiB, 97.0% |
| 20 lines near line 900,000 | 50.1% | 50.0% | 108.54 to 3.08 MiB, 97.2% |

Those figures are from a final three-round randomized rerun after review
hardening, with six ordinary iterations and three large-file iterations per
round. It ran under the same shared-host constraint and used seed `20260808`.
The earlier five-round campaign independently showed the same memory result and
material CPU/latency reductions.

The change preserves exact line counts and continuation hints, CRLF and blank
line behavior, bounded oversized-line handling, long-line truncation, full-file UTF-8 validation, binary/image/PDF
routing, path resolution, lifecycle logging, context-output guards, and file
touch events. It does not relax any trust-boundary or command security check.

## Follow-up investigation outcomes

Follow-up work retains production changes only when representative, paired
measurements satisfy the Bead's acceptance gate. Evidence-only branches and
benchmark harnesses are not merged into `dev`; durable conclusions are recorded
here instead.

| Bead | Investigated change | Result | Decision |
|---|---|---|---|
| `jcode-d5o` | Avoid per-tool observer-hook process startup | `setsid` reduced wall time 17.8% and child CPU 9.2%, below the 80% target | Reverted; a persistent or connection-reusing hook design is required |
| `jcode-8mj` | Cache repeated `agentgrep find` inventory | Warm p50 improved from 50.745 ms to 40.013 ms, 21.15%, below the 30% gate | Reverted; experimental reusable-inventory seam retained only in the external agentgrep fork |
| `jcode-yx7` | Cache large-file line offsets for repeated reads | Focused parity tests passed, but no paired built-binary CPU, latency, memory, and daemon-invalidation campaign completed | Reverted; redesign only with integrated evidence |
| `jcode-ms5` | Remove streaming-delta clones or coalesce fanout | Real Rust baseline confirmed clone ownership by accumulation, parsing, tap, history, and wire consumers; no safe general-purpose candidate was established | Closed investigation-only; production unchanged |
| `jcode-bef` | Cache remaining character counts in `StreamBuffer` chunks | Directional wall gains ranged from 9.0% to 25.1%, but required paired CPU, allocation, memory, fragmentation, rendering, and guardrail evidence was incomplete | Reverted; retain the idea for a future instrumented campaign |
| `jcode-g25` | Return a shared immutable render-cache view instead of cloning `Vec<Line>` | TestBackend warm-frame p50 ranged from 0.274 ms for 1 KiB at width 80 to 0.807 ms for 64 KiB at width 160, but direct consumers require owned lines for alignment, mapping, truncation, and preparation | Rejected; a shared view would recreate materialization at the adapter or require an out-of-scope renderer ownership redesign |
| `jcode-5sx` | Cap agentgrep post-processing workers under concurrent grep load | Across five randomized rounds at 1/2/4/8 concurrent calls, baseline aggregate CPU medians were 10.61/21.23/47.43/102.40 ms versus 17.44/32.20/62.91/102.28 ms with a four-worker cap; the candidate was slower at every concurrency level while exact output hashes matched eight semantic cases | Reverted; fixed worker capping is not a general-purpose CPU win and adds wall latency |
| `jcode-ub3` | Bound slow-client `ServerEvent` queues | Source inspection found three unbounded delivery hops: processing ingress, live-attachment fanout, and the per-client writer queue. Bounding only the final hop would shift retention upstream, and the protocol has no compatible overflow/resync contract for critical, tool, text, or reasoning events | Rejected; future work requires one bounded ownership path across all hops with explicit loss/resync semantics and multi-client measurements |

These negative results are useful constraints rather than failed deliveries:
they prevent narrow microbenchmark wins from adding runtime state, invalidation
complexity, or ownership changes without a demonstrated user-visible benefit.

### Follow-up acceptance matrix

Each investigation ran in its own worktree. Rust-heavy commands were serialized,
and no candidate could land without a material general-purpose gain, focused
compatibility checks, and public-path acceptance. The table maps those shared
requirements to the evidence actually obtained; an incomplete public-path row is
a rejection condition, not an implied pass.

| Bead | Baseline and material-gain check | Safety, compatibility, and edge checks | Public/integration and packaging check | Observed acceptance result |
|---|---|---|---|---|
| `jcode-d5o` | 200-run paired process benchmark; 17.8% wall and 9.2% child-CPU reduction missed the 80% gate | 10,000-event stress produced 10,000 valid records; focused hook and detached-session tests passed after revert | Selfdev build passed; full guardrails exposed seven unrelated baseline failures | Rejected and reverted; public behavior remains unchanged, and a persistent/connection-reusing design is required |
| `jcode-8mj` | Paired fresh/warm find p50 was 50.745/40.013 ms, a 21.15% gain below the 30% gate | Focused freshness, replacement, policy, and output-parity tests passed; the required concurrency, eviction, and complete security matrix remained incomplete | `jcode-app-core` check, selfdev build, and private-socket runtime smoke passed; Jcode did not pin the experimental fork | Rejected and reverted; the external seam remains experimental and is not part of the packaged Jcode path |
| `jcode-yx7` | The bounded index candidate was not credited with a material gain because paired built-binary CPU, latency, and memory measurements did not complete | 19 focused read tests passed, including warm parity and file-replacement invalidation | Full build, Clippy, guardrails, and isolated-daemon pagination/invalidation acceptance did not complete | Rejected and reverted; the existing streamed-read implementation remains the public path |
| `jcode-5sx` | Five randomized rounds at 1/2/4/8 concurrent calls showed the candidate slower at every level and no material CPU reduction | Exact hashes matched for literal, regex, paths-only, type, glob, hidden, no-ignore, and dense-result cases; 27/31 focused Jcode tests passed, with four environment-dependent fixture failures | Canonical selfdev build and private-daemon session creation passed; the final tool call was not claimed because the debug CLI rewrote the explicit socket path | Rejected and reverted; no agentgrep fork revision or lockfile change was packaged |
| `jcode-ms5` | Six-scenario real Rust baseline covered wall, CPU, allocation peak, and RSS; no safe candidate established a gain | Exact oracle digests matched while ownership analysis covered accumulation, parsing, tap, history, and wire consumers | The ignored real-Rust baseline test passed; no runtime candidate existed to exercise through a public client | Closed investigation-only; production and public streaming semantics are unchanged |
| `jcode-bef` | Directional wall gains of 9.0% to 25.1% were observed, but paired CPU, allocation, RSS, retained-memory, and fragmentation evidence was absent | 18 focused StreamBuffer tests and all 63 `jcode-tui-core` library tests passed; focused Clippy and `jcode-tui` check passed | Representative rendering and full guardrails were incomplete, so the candidate was reverted before packaging | Rejected and reverted; no count cache is present in the public TUI path |
| `jcode-g25` | Five TestBackend baseline samples covered 1 KiB/64 KiB inputs at widths 80/160; no ownership-safe candidate demonstrated a gain | Nine focused `jcode-tui-messages` tests passed; direct-consumer analysis covered alignment, mapping, truncation, and preparation | The selfdev `tui_bench` dev binary built and ran; guardrails reported unrelated pre-existing formatting and ratchet drift | Closed investigation-only; no render-cache API or packaged runtime behavior changed |
| `jcode-ub3` | No final-hop candidate was benchmarked because three unbounded hops would merely shift retained memory upstream | Lifecycle attachment ordering, cancellation, stale/newer cancellation, and reload-recovery tests passed | No protocol-compatible overflow/resync contract or representative multi-client public acceptance existed | Closed investigation-only; queue behavior remains unchanged pending an end-to-end bounded ownership design |

Repository delivery was checked separately: the portfolio commits on `dev` modify
only this Markdown file, the public GitHub raw-file endpoint serves all eight
outcomes, and the Beads workflow records `jcode-d5o`, `jcode-8mj`, and
`jcode-yx7` as blocked `needs-redesign` while the other five are closed
`investigation-only`. Thus no rejected or acceptance-incomplete runtime candidate
crossed the repository, package, daemon, or client integration boundary.

## 2026-08-09 implementation retry evidence

The three redesign Beads were resumed in isolated worktrees and integrated into a
fresh checkout for validation. The implementation is opt-in where behavior could
change: persistent hooks require the `persistent:` command prefix, while ordinary
hooks retain their one-process-per-event contract. The integrated checkout built
both `target/selfdev/jcode` and `target/selfdev/tool_profile` after formatting.

Focused fresh-base checks passed:

- `jcode-base` hooks tests: 15 passed, including persistent reuse, restart,
  fallback, lifecycle ordering, payload, environment, recursion suppression, and
  fail-open behavior.
- `jcode-app-core` read tests: 20 passed, including warm parity, same-length
  replacement invalidation, UTF-8, CRLF, blank lines, oversized lines, and range
  metadata.
- `jcode-app-core` inventory tests: 3 passed, including bounded retention,
  filter-key isolation, create/delete/policy invalidation, and same-length writes.
- `cargo fmt --all -- --check` and `git diff --check` passed.

The built profiler ran three randomized rounds with seed `20260809`. Its warm
medians are useful evidence, but not a complete acceptance claim because the
harness warms once before measuring each process:

| Case | No hooks wall/CPU | Ordinary hook wall/CPU | Persistent hook wall/CPU |
|---|---:|---:|---:|
| `read_tiny` | 0.115/0.102 ms | 0.688/0.375 ms | 0.123/0.253 ms |
| `bash_true` | 1.296/1.496 ms | 1.227/1.262 ms | 0.917/1.539 ms |
| `read_large_head` | 0.278/0.222 ms | 1.490/0.895 ms | 0.935/0.841 ms |
| `read_large_tail` | 0.336/0.249 ms | 1.105/0.547 ms | 0.758/0.577 ms |
| `agentgrep_find` | 1.044/1.127 ms | 1.923/1.541 ms | 1.075/1.678 ms |

Persistent observers reduced `read_tiny` wall time by 82.1% versus ordinary
spawn and `bash_true` wall time by 25.3%. The profiler's parent `RUSAGE_CHILDREN`
value is not a valid per-event CPU comparison for the persistent case because the
broker worker remains alive instead of exiting per event. That accounting gap is
explicitly retained as an acceptance limitation rather than reported as a CPU
win.

The private built-daemon path also passed representative behavior checks:

- Five `read` calls through one persistent-hook daemon produced exactly five
  NDJSON `post_tool` envelopes, with matching session ID, tool name, status,
  payload, and `JCODE_HOOKS_DISABLED=1` recursion suppression. No duplicates or
  drops were observed.
- A near-tail read returned lines 900,000 through 900,019 with the exact
  continuation count of 99,981. A same-length in-place mutation was observed on
  the next read at line 123,456, proving index freshness in the daemon path.
- Repeated daemon `agentgrep find` calls returned identical Cargo results. A
  temporary fixture was then created, renamed, and deleted; the built daemon
  returned 1, 1, and 0 matching files respectively, proving cache invalidation
  across those transitions.

The original Beads remain open pending the still-required cold/random benchmark
matrix, complete built-binary CPU attribution for persistent workers, and full
security/guardrail review. These results validate the implementation and public
smoke path without overstating acceptance. Rollback is the integration merge
commit or the three scoped feature commit ranges for read, inventory, and hooks.

## Other opportunities

### 1. Configured post-tool hooks can dominate cheap tools

A preliminary hooks-enabled smoke run on this machine measured a tiny read at
about 51 ms and about 50 ms of child CPU per call, versus 0.125 ms and 0.120 ms
with hooks suppressed for core attribution. The configured observer starts a
detached process after each tool call. This result is machine- and hook-specific,
so it is evidence of integration overhead rather than a core-tool regression.

Potential improvement: a persistent local hook broker or a lightweight resident
notification integration could avoid process startup per tool. Any design must
preserve hook isolation, environment filtering, recursion suppression, event
delivery semantics, and failure containment. No hook behavior was changed here.

### 2. `agentgrep find` is the largest ordinary search wall-time

`agentgrep find` was about 50 ms, while targeted grep was about 16 ms wall and
72 ms aggregate CPU. Process startup, filesystem traversal, and ripgrep work are
the dominant costs. A persistent index could reduce repeated find latency, but
it introduces freshness, invalidation, memory, and sensitive-path concerns.
Given the current latency and security tradeoff, no change is recommended without
a workload showing repeated find calls as a material user-visible bottleneck.

### 3. Batch has fixed coordination overhead

Batching four tiny cached reads was about 17% slower than executing them
sequentially because each read is only about 0.1 ms and batch must create,
schedule, track, and order subcalls. This is expected. Batch remains valuable for
independent calls whose I/O or subprocess latency can overlap. Special-casing
tiny tools would add complexity for negligible absolute savings.

### 4. Bash and background management are already inexpensive

`bash true`, background launch, and `bg list` were all below 3 ms median wall
apart from command output work. The destructive-command gate, process-group
handling, timeout behavior, status persistence, and cross-process truth should
not be weakened for microsecond-scale savings.

## Conclusion

The main proven core opportunity was large text-file reading. Streaming removes
about 106 MiB from the peak resident-memory high-water mark for the 113 MB fixture and also materially cuts
CPU and latency. The remaining common tools are mostly in the sub-3 ms range, or
their cost is attributable to useful external work such as repository traversal.
The next performance investigation should target configured hook process startup
only if its user-visible integration cost is considered unacceptable.
