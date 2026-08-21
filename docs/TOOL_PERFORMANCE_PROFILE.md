# Common Tool CPU and Memory Profile

Status: current-state measurement and implementation retry evidence
Measured: 2026-08-08 through 2026-08-10
Bead: `jcode-znm` (portfolio baseline: `jcode-skr`)

> **Inventory-cache rollback (2026-08-09):** The local `agentgrep find`
> inventory snapshot was removed after a root-repository probe exposed an
> ignored-tree traversal regression. Its manifest recursively walked and
> hashed the checkout's `target/` tree, which contained 234,371 files. Current
> Jcode delegates `find` directly to the upstream `agentgrep::find::run_find`
> path again. Inventory-cache measurements in this document are historical
> evidence for the reverted implementation and must not be read as current
> Jcode latency. The read-offset and hook-observer changes remain separate.

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
| `jcode-yx7` | Cache large-file line offsets for repeated reads | Paired public probes showed 49.8% lower warm near-tail latency and 7.7-93.8% lower warm latency across five line shapes; 25 fresh-daemon cold samples per binary found four improved medians and one 8.4% regression | Retained; bounded memory, exact output, mutation/truncation/replacement, eviction-sequence, concurrency, and private-daemon behavior were validated |
| `jcode-ms5` | Remove streaming-delta clones or coalesce fanout | Real Rust baseline confirmed clone ownership by accumulation, parsing, tap, history, and wire consumers; no safe general-purpose candidate was established | Closed investigation-only; production unchanged |
| `jcode-bef` | Cache remaining character counts in `StreamBuffer` chunks | Directional wall gains ranged from 9.0% to 25.1%, but required paired CPU, allocation, memory, fragmentation, rendering, and guardrail evidence was incomplete | Reverted; retain the idea for a future instrumented campaign |
| `jcode-g25` | Return a shared immutable render-cache view instead of cloning `Vec<Line>` | TestBackend warm-frame p50 ranged from 0.274 ms for 1 KiB at width 80 to 0.807 ms for 64 KiB at width 160, but direct consumers require owned lines for alignment, mapping, truncation, and preparation | Rejected; a shared view would recreate materialization at the adapter or require an out-of-scope renderer ownership redesign |
| `jcode-5sx` | Cap agentgrep post-processing workers, then cap both ripgrep and post-processing workers, under concurrent grep load | Fresh five-round paired 1/2/4/8-process campaigns found only 2.0-5.3% CPU reduction at eight-way concurrency; single-call CPU fell 19.2-31.3%, but wall time regressed 10.6-14.9% | Rejected; the high-concurrency CPU gain is not material and the single-agent latency tradeoff violates the Bead contract |
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
| `jcode-yx7` | Paired built-binary public probes covered cold and warm reads across five line shapes. Warm reductions ranged from 7.7% to 93.8%; a five-sample cold matrix per fixture improved four medians and regressed one by 8.4% | 20 focused tests plus public probes covered UTF-8, CRLF, blank and oversized lines, exact metadata, same-length mutation, truncation, inode replacement, bounded fill/reread, and eight concurrent callers | Current source built successfully; focused tests and scoped Clippy passed again on 2026-08-10, and an isolated agent-run validation exercised the final binary | Validated and retained; bounded indexing materially improves repeated large-file reads without a meaningful general cold regression in the measured matrix |
| `jcode-5sx` | Five randomized paired rounds covered 1/2/4/8 concurrent profiler processes with four measured calls per process. Post-processing-only caps reduced CPU 19.2/21.5/8.8/2.0%; adding a ripgrep cap reduced CPU 31.3/21.3/8.8/5.3% | Both candidates retained the same 4,522-byte representative output size in every process. Exact-result and broader semantic/security acceptance were intentionally not promoted after the material-gain gate failed | Exact current and scratch-patched binaries were built independently; no fork revision, lockfile, repository source, daemon, or package was changed | Closed investigation-only; reject both candidates because high-concurrency CPU savings were small while single-call wall time regressed 10.6-14.9% |
| `jcode-ms5` | Six-scenario real Rust baseline covered wall, CPU, allocation peak, and RSS; no safe candidate established a gain | Exact oracle digests matched while ownership analysis covered accumulation, parsing, tap, history, and wire consumers | The ignored real-Rust baseline test passed; no runtime candidate existed to exercise through a public client | Closed investigation-only; production and public streaming semantics are unchanged |
| `jcode-bef` | Directional wall gains of 9.0% to 25.1% were observed, but paired CPU, allocation, RSS, retained-memory, and fragmentation evidence was absent | 18 focused StreamBuffer tests and all 63 `jcode-tui-core` library tests passed; focused Clippy and `jcode-tui` check passed | Representative rendering and full guardrails were incomplete, so the candidate was reverted before packaging | Rejected and reverted; no count cache is present in the public TUI path |
| `jcode-g25` | Five TestBackend baseline samples covered 1 KiB/64 KiB inputs at widths 80/160; no ownership-safe candidate demonstrated a gain | Nine focused `jcode-tui-messages` tests passed; direct-consumer analysis covered alignment, mapping, truncation, and preparation | The selfdev `tui_bench` dev binary built and ran; guardrails reported unrelated pre-existing formatting and ratchet drift | Closed investigation-only; no render-cache API or packaged runtime behavior changed |
| `jcode-ub3` | No final-hop candidate was benchmarked because three unbounded hops would merely shift retained memory upstream | Lifecycle attachment ordering, cancellation, stale/newer cancellation, and reload-recovery tests passed | No protocol-compatible overflow/resync contract or representative multi-client public acceptance existed | Closed investigation-only; queue behavior remains unchanged pending an end-to-end bounded ownership design |

Repository delivery was checked separately: rejected or acceptance-incomplete runtime candidates did not cross the repository, package, daemon, or client integration boundary. The fresh `jcode-5sx` campaign likewise used only scratch-patched dependency sources and binaries, so its rejection requires no runtime rollback.

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

A fresh-root acceptance pass then ran the complete 15-case profiler matrix for three randomized rounds with seed `20260809`, using 10 iterations per normal case and two per large-file case. The measurements used the landed `faa9860a1` build and included no-hook, ordinary-hook, and opt-in persistent-hook configurations. The new steady-state medians were:

| Case | No hooks wall/CPU | Ordinary hook wall/CPU | Persistent hook wall/CPU |
|---|---:|---:|---:|
| `read_tiny` | 0.138/0.114 ms | 0.694/0.458 ms | 0.117/0.193 ms |
| `read_large_head` | 0.367/0.341 ms | 1.073/0.635 ms | 0.558/0.539 ms |
| `read_large_tail` | 0.497/0.378 ms | 1.048/0.509 ms | 0.336/0.266 ms |
| `agentgrep_find` | 0.984/0.969 ms | 1.758/1.522 ms | 1.117/1.520 ms |
| `bash_true` | 0.747/0.899 ms | 1.464/1.345 ms | 1.007/1.608 ms |

The optimized indexed reads were 99.4% lower wall time and 99.5% lower reported CPU than the legacy head read in the no-hook steady-state comparison. Near-tail reads were 99.3% lower wall time and 99.4% lower reported CPU. These are warm measurements because the profiler performs one unmeasured warm-up per process. The persistent-worker CPU limitation remains: parent `RUSAGE_CHILDREN` does not include CPU consumed by the still-live broker worker.

A fresh private daemon using the same root binary also passed the public-path checks. Read output was stable across repeated near-tail calls, a same-length mutation was observed immediately, and inventory output remained stable across repeated calls. Inventory create, rename, delete, `max_files`, glob filtering, and four concurrent callers all returned the expected results. A persistent-hook daemon produced exactly five valid `post_tool` records for five reads with matching session IDs, tool names, statuses, and payloads.

A second fresh private-daemon matrix exercised the remaining low-cost public edge paths against the same `faa9860a1` binary. Read passed near-tail pagination, same-length mutation, truncation, inode replacement, CRLF, and invalid-UTF-8 fixtures. Inventory passed visible and nested results, `.gitignore` exclusion, sensitive `.env` exclusion, recursive `**/*.rs` filtering, and `max_files=1`. The daemon cleaned up all temporary fixtures and no tracked files changed. This closes those specific public edge observations; the subsequent hypothesis probe below covers the cold-first, eviction-sequence, worker-accounting, and read-concurrency cases that were not part of this matrix.

A third fresh private-daemon probe exercised those hypotheses directly against the same root binary:

- A first large-file read took 2.430 ms and the five-call warm median was 0.500 ms. This is an observed cold-versus-warm result through the public daemon path, not a replacement for a paired legacy cold baseline.
- The probe filled the 64-key public cache with distinct files, repeated the fill-and-reread sequence for three cycles, and preserved exact read output parity on every reread. The first reread after each fill measured 0.904, 0.679, and 0.659 ms; the following calls measured 0.335, 0.932, and 0.480 ms. The samples are noisy, so they exercise the bounded sequence without claiming latency-only proof of eviction.
- Eight concurrent public read callers returned byte-for-byte identical output to serial baselines for their ranges.
- A persistent-hook daemon processed 100 valid `post_tool` envelopes with no drops or duplicates. The probe sampled the exact worker PID through `/proc`: worker CPU increased from 0.020 s to 0.440 s, a 0.420 s delta over the measured batch. The hook intentionally performed bounded hashing per envelope so worker CPU was observable; this establishes an accounting method, not a production-hook CPU benchmark.
- During the same three-cycle cache probe, daemon `VmRSS` and `VmHWM` were 89,340 KiB before the fill and 98,540 KiB after it, a 9,200 KiB process high-water delta. This confirms bounded process growth under the exercised workload, but does not attribute every byte to the line index because runtime initialization and allocator state are included.
- A controlled comparison ran 50 identical bounded-hash observer events through separate exact-binary daemons. Ordinary hooks measured 0.198859 s wall and 2.604675 s summed child CPU; persistent hooks measured 0.021193 s wall and 0.190 s worker CPU. Both delivered 50 valid payloads. The controlled hook workload demonstrates the accounting and dispatch difference, but is not a production-hook workload benchmark.

A paired public baseline was then run with the same probe, fixtures, debug-socket
protocol, 64-file fill sequence, and eight-way read concurrency. The baseline was
the parent source commit `4ac6a7355`, built in an isolated worktree. That historical
source required three compile-only compatibility edits for the current Rust
compiler: explicit `String` types for hook envelope keys and two local copies of
the inventory usage counter to satisfy newer borrow checking. No benchmark or
runtime behavior was changed by those edits, but this is therefore a
compatibility-patched historical baseline rather than a byte-for-byte rebuild.

| Public daemon probe | Historical baseline | Optimized binary | Observation |
|---|---:|---:|---|
| First near-tail read | 2.104 ms | 2.255 ms | Single cold samples were comparable; this is not a no-regression confidence interval |
| Five-call warm median | 0.864 ms | 0.434 ms | 49.8% lower optimized wall time |
| 64-file fill median | 0.816 ms | 0.828 ms | Comparable traversal cost |
| First reread after fill | 1.280/0.677/0.707 ms | 0.747/0.797/0.716 ms | Exact output parity in all cycles |
| Eight concurrent callers | Passed | Passed | Byte-for-byte parity with serial baselines |
| Process VmRSS/VmHWM before to after fill | 88,308 to 90,300 KiB | 88,428 to 97,876 KiB | 1,992 versus 9,448 KiB process delta; allocator/runtime attribution remains open |

To reduce the uncertainty of the single cold sample, three fresh-daemon runs
then read the same 500,000-line fixture near its tail. Historical samples were
0.030371, 0.028627, and 0.029750 s, for a 0.029750 s median. Optimized samples
were 0.029054, 0.025902, and 0.027950 s, for a 0.027950 s median, or 6.0%
lower wall time. All six output hashes matched and all returned 1,676 bytes.
This is evidence of no cold regression on this fixture, but not a full workload
confidence interval across file sizes and host contention.

Both binaries also delivered 100 valid persistent-hook envelopes without drops or
duplicates. The historical and optimized hook batches measured 0.091123 s and
0.086211 s wall time, with `/proc` worker CPU deltas of 0.400 s and 0.410 s
respectively. The identical 50-event bounded-hash comparison likewise measured
historical ordinary/persistent wall times of 0.199259/0.020215 s and optimized
times of 0.198859/0.021193 s. The parent already contained a persistent-hook path,
so these paired hook numbers validate public compatibility and accounting, not a
claim that the integrated observer redesign alone produced that difference.

### Historical inventory-cache evidence (rolled back)

The same public protocol was exercised for `agentgrep find` against an
workspace-scoped eight-Rust-file fixture. Historical first/warm-median wall times
were 7.526/0.646 ms; optimized times were 10.029/0.969 ms. Repeated output,
`**/*.rs` filtering, and hashes were identical, and create, rename, and delete
transitions all returned the expected changed/changed/restored states in both
binaries. The small fixture is dominated by daemon and ranking overhead, so it
does not demonstrate a warm latency gain. It does establish public output parity
and freshness behavior, so the larger campaign below was used for the material
gain and cold-regression check.

The larger inventory campaign then repeated the same public probe three times per
binary over 2,000 Rust files in 100 directories. Historical first-call medians
were 0.038456 s and warm medians were 0.004852 s. Optimized medians were 0.039185
s and 0.002953 s, a 1.9% cold increase and 39.1% warm reduction. Every run had
identical repeated and filtered output hashes, and every run passed create,
rename, and delete freshness transitions. This demonstrates a material warm
inventory gain with no meaningful cold regression on a traversal-dominated public
fixture. Inventory-specific memory attribution, broader security cases, and
multi-caller contention remain open.

The same 2,000-file runs also sampled daemon memory. Historical `VmRSS`/`VmHWM`
rose from 88,972 to 96,396 KiB, a 7,424 KiB process delta. Optimized memory rose
from 87,920 to 95,740 KiB, a 7,820 KiB delta, or 396 KiB more in this sample.
This is bounded process-growth evidence with exact public output parity, not an
allocator-level attribution of bytes to the inventory cache.

A public cache-capacity stress probe then queried ten distinct repository roots,
each containing 300 Rust files, through the same `agentgrep find` debug-socket
path. This deliberately exceeded the implementation's eight-repository resident
cap while retaining exact output hashes. Historical process `VmHWM` rose from
88,736 to 96,164 KiB, a 7,428 KiB delta. Optimized `VmHWM` rose from 88,312 to
96,440 KiB, an 8,128 KiB delta, or 700 KiB more in this workload. All ten first
call hashes matched between binaries, repeated calls were stable, and public
create/delete transitions changed and restored the expected result. The sequence
exercises over-capacity behavior and bounded process growth, but the public
protocol does not expose resident-cache count, and `/proc` memory still includes
daemon initialization and allocator effects.

A paired guardrail probe then exercised ten concurrent public `agentgrep find`
callers against ten repository roots. Historical and optimized daemons both
matched their serial output hashes byte-for-byte. The same calls excluded the
sensitive `.env` path and its content, excluded `.gitignore` and `ignored.rs`
under normal policy, and respected `max_files=1`. This expands the public
concurrency and sensitive-path evidence, but it remains a finite fixture matrix,
not a complete security review or contention stress limit.

A paired public read probe then expanded the cold-read evidence across five line-shape
fixtures using the same private debug-socket protocol and historical compatibility-
patched binary. Every output hash and byte count matched. The first-call and
two-call warm samples were:

| Fixture | Historical cold / warm median | Optimized cold / warm median | Observation |
|---|---:|---:|---|
| 80,000 short lines | 6.514 / 5.779 ms | 5.667 / 1.119 ms | 13.0% lower cold, 80.6% lower warm |
| 8,000 approximately 1 KiB lines | 2.560 / 2.210 ms | 2.261 / 0.537 ms | 11.7% lower cold, 75.7% lower warm |
| 60,000 mixed-length lines | 7.689 / 6.523 ms | 5.243 / 0.417 ms | 31.8% lower cold, 93.6% lower warm |
| 80,000 CRLF lines | 4.735 / 5.833 ms | 4.308 / 0.363 ms | 9.0% lower cold, 93.8% lower warm |
| 1.25 MiB line plus 100 short lines | 0.567 / 0.565 ms | 0.581 / 0.522 ms | 2.5% higher cold, 7.7% lower warm |

This closes the specific cross-fixture correctness observation and adds evidence
of warm benefit across short, long, mixed, CRLF, and oversized-line inputs. The
small two-sample warm probes and five cold samples are not a host-contention
confidence interval, and the oversized-line cold result remains a near-neutral
sample rather than a universal no-regression claim.

To strengthen the cold comparison, each binary then ran five fresh private-daemon
samples for each of the same five fixtures, for 25 cold samples per binary. The
output hashes remained identical on every sample. Per-fixture medians were:

| Fixture | Historical median | Optimized median | Median delta |
|---|---:|---:|---:|
| Short lines | 4.918 ms | 4.453 ms | 9.5% lower |
| Approximately 1 KiB lines | 2.915 ms | 3.161 ms | 8.4% higher |
| Mixed-length lines | 7.619 ms | 6.553 ms | 14.0% lower |
| CRLF lines | 6.097 ms | 5.072 ms | 16.8% lower |
| 1.25 MiB line | 1.019 ms | 0.987 ms | 3.1% lower |

The optimized first-call median was lower on four of five fixture shapes, with
one small long-line regression inside the observed timing range. This materially
strengthens the no-regression observation, but does not establish a statistical
gate under host contention or a larger distribution of file sizes.

Finally, the exact current `target/selfdev/tool_profile` ran the production
`Registry::execute` profiler with configured hooks enabled for two randomized
rounds, four iterations for normal cases, and one for large cases. All 28 case
processes completed with valid tool-profile output. Representative configured-hook
medians were `read_tiny` 27.151 ms wall / 28.459 ms CPU, `read_large_head`
30.825 / 33.358 ms, `read_large_tail` 30.105 / 32.794 ms, `agentgrep_find`
29.293 / 31.113 ms, and `bash_true` 27.938 / 29.705 ms. The run verifies the
real configured-hook integration path and its output contract, but it is not a
paired historical hook benchmark and does not close the production-representative
observer CPU requirement.

The integrated acceptance Bead `jcode-znm` is complete. The read-index follow-up `jcode-yx7` is now validated and retained. Broader portfolio research remains available for allocator-level inventory memory attribution, direct resident-cap observation, production-representative persistent-hook CPU comparison, and security/concurrency cases beyond the exercised public matrices. These results validate the integrated implementation and root-binary public path without turning every remaining measurement opportunity into a delivery blocker. Rollback remains the integration merge commit or the scoped feature commit ranges for read, inventory, and hooks.

### 2026-08-10 agentgrep concurrent-CPU retry

A fresh campaign compared the exact current `target/selfdev/tool_profile` binary
with two scratch-only builds of pinned `agentgrep` v0.1.6. The first capped native
and ripgrep-result post-processing at four workers. The second also passed
`--threads 4` to ripgrep. No repository source, dependency pin, lockfile, daemon,
or installed binary changed.

Each candidate ran five randomized paired rounds at 1, 2, 4, and 8 concurrent
profiler processes, with four measured `agentgrep_grep` calls per process after
the profiler warm-up. Hooks were disabled for core attribution. Every process
reported the same 4,522-byte representative output size; exact-result promotion
was intentionally deferred unless a candidate passed the performance gate.

| Concurrent processes | Post-processing cap wall / CPU delta | Post-processing + ripgrep cap wall / CPU delta |
|---:|---:|---:|
| 1 | +14.9% / -19.2% | +10.6% / -31.3% |
| 2 | +5.2% / -21.5% | +9.8% / -21.3% |
| 4 | +2.1% / -8.8% | +7.2% / -8.8% |
| 8 | -2.0% / -2.0% | -6.1% / -5.3% |

Both candidates lowered summed process high-water deltas by roughly 26-30%, but
the Bead targets aggregate CPU under concurrent agent load. At eight-way load,
CPU improved only 2.0-5.3%, while the stronger cap regressed representative
single-agent wall time by 10.6%. The candidate therefore fails the material-gain
and wall-latency gates. Full literal/regex, cancellation, context, ignore-policy,
path-security, and public-daemon promotion tests were intentionally not run after
that rejection gate. `jcode-5sx` closes investigation-only with no production
change; revisit only with an approach that removes duplicate work rather than
only constraining worker parallelism.

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

The main proven core opportunities were large text-file streaming and bounded line indexing for repeated large-file reads. Streaming removes about 106 MiB from the peak resident-memory high-water mark for the 113 MB fixture, while the bounded index materially reduces warm range-read latency with measured cold behavior remaining broadly comparable. The remaining common tools are mostly in the sub-3 ms range, or
their cost is attributable to useful external work such as repository traversal.
The next performance investigation should target configured hook process startup
only if its user-visible integration cost is considered unacceptable.

## 2026-08-21 custom dev versus official upstream

Bead: `jcode-7u8`

### Question and comparison targets

This quick campaign checks whether the current custom development binary has
regressed on cheap, no-network CLI startup paths relative to the official
upstream source. It is intentionally a startup smoke benchmark, not a claim
about model latency, agent-turn throughput, tool execution, or long-lived
daemon performance.

| Target | Exact source ref | Binary | SHA-256 | Size |
|---|---|---|---|---:|
| Custom dev | `dev` HEAD `e43c6f0bc165f4caf87be234c8e5ccb26c35c8b4` | scratch selfdev build | `d855cffcf4d3799582f299e2fd294e01fe83a8ab2a479493aceca12968a63905` | 281,406,112 bytes |
| Official upstream | `upstream/master` `a63dbc4546895ecb4d1be1a285d98e6e13fb1b74` | scratch selfdev build | `69ff9aeb3deecf21273e9675636f1808e45b3fecc69bf1f2ee039fc679a4a147` | 281,406,112 bytes |

The custom ref is local-only and unpublished. The upstream executable was
built from an exact `git archive` of `upstream/master`; both scratch binaries
and their source archives are local measurement artifacts, not distributed
release artifacts. The two histories share the `v0.79.1` release ancestor. At
measurement time, the custom side was 506 commits ahead of that ancestor and
upstream was 13 commits ahead.

### Method and comparability controls

- Host: AMD Ryzen AI 7 PRO 350, 27 GiB RAM, Linux x86_64.
- Both binaries used Rust/Cargo `1.95.0`, target `x86_64-unknown-linux-gnu`,
  `cargo build --profile selfdev --bin jcode`, no feature flags, and the same
  `scripts/dev_cargo.sh` clang + lld wrapper.
- The `selfdev` profile inherits release settings but sets `opt-level=0`; the
  compared settings were `debug=0`, `codegen-units=256`,
  `incremental=true`, and no LTO. Both outputs are unstripped ELF PIE files.
- Builds were serialized. Incremental sccache was skipped because it cannot
  cache incremental units. The custom archive build supplied
  `JCODE_BUILD_GIT_HASH=e43c6f0bc165`; the upstream build did not, so its banner
  is not trusted as source identity.
- Four side-effect-free commands: `--version`, `--help`, `server --help`, and
  `run --help`.
- One warm-up per binary and case, followed by 15 measured subprocess samples.
  Fixed seed `20260821`; binary/case order was randomized and interleaved per
  round.
- Wall time came from `perf_counter_ns`; child CPU came from the delta of
  `RUSAGE_CHILDREN` around each subprocess.
- Each process used empty temporary `HOME` and XDG config/data/cache/state
  directories. No model, network, shared daemon, or user configuration was
  involved. All 120 measured commands returned exit code 0 and emitted no
  stderr.

### Results

Values are p50 per process. “Custom faster” is `(upstream - custom) /
upstream`; a negative value means the custom binary was slower.

| Case | Custom wall | Upstream wall | Custom faster | Custom child CPU | Upstream child CPU | CPU lower |
|---|---:|---:|---:|---:|---:|---:|
| `--version` | 9.090 ms | 8.434 ms | -7.8% | 9.991 ms | 9.411 ms | -6.2% |
| `--help` | 9.475 ms | 10.359 ms | 8.5% | 10.705 ms | 11.445 ms | 6.5% |
| `server --help` | 9.394 ms | 10.107 ms | 7.1% | 10.388 ms | 11.341 ms | 8.4% |
| `run --help` | 9.359 ms | 9.754 ms | 4.0% | 10.542 ms | 11.061 ms | 4.7% |

The three help cases had identical output byte counts and SHA-256 digests:
`--help` 7,379 bytes, `server --help` 4,423 bytes, and `run --help` 4,717
bytes. The `--version` output differed only because the custom binary printed
`jcode v0.79.507-dev (e43c6f0bc165)` while the upstream scratch binary printed
the stale local banner `jcode v0.79.507-dev (e43c6f0bc)`.

### Finding and limitations

**Finding:** this equal-profile smoke test does not show a broad startup
regression. The custom binary was 4.0-8.5% faster on the three equivalent help
paths, with 4.7-8.4% lower child CPU. The one metadata-only `--version` path
was 7.8% slower wall time and 6.2% higher child CPU, which is a small isolated
exception rather than evidence of a general regression.

This is evidence for process startup and Clap help rendering only. It does not
establish that the custom fork is faster or slower for agent turns, provider
streaming, tool execution, TUI rendering, memory retention, or daemon reuse.
The initial release-vs-selfdev comparison was rejected because its optimization
profiles were not comparable and is not used for this conclusion. A private
custom-daemon start smoke returned `{"status":"running"}` in about 201 ms,
but ordinary stop required force and was not promoted to a timed comparison.
The private process was verified absent afterward, and the shared daemon was
not touched.

For a stronger follow-up, compare the common `v0.79.1` ancestor against the
custom build using the existing private-daemon acceptance harness, with one
binary build at a time and separate cold/warm measurements for startup, one
tool call, and a short local session. Do not treat this quick result as a
reason to add an optimization until that workload-level comparison identifies
a user-visible gap.

## 2026-08-21 broader README and client-startup comparison

Bead: `jcode-fi8`

### Scope correction

The preceding `jcode-7u8` campaign measured only cheap CLI process startup and
Clap help rendering. It did not evaluate the README's broader performance
section. The README makes separate claims about resident memory and per-session
scaling, time to first frame, time to first input, and boot-up behavior. The
architecture design also defines NFR-002 targets for TUI first-frame p95 below
25 ms, first-input readiness p95 below 75 ms, local prompt acknowledgement p95
below 100 ms, and daemon-event-to-client-display p95 below 50 ms.

This follow-up extends the comparison to the client/server startup path, but it
does not retroactively turn the result into a complete validation of every
README or design goal.

### Targets and controls

The same exact scratch binaries from `jcode-7u8` were reused, avoiding another
Cargo build:

| Target | Source ref | Binary SHA-256 |
|---|---|---|
| Custom dev | `e43c6f0bc165f4caf87be234c8e5ccb26c35c8b4` | `d855cffcf4d3799582f299e2fd294e01fe83a8ab2a479493aceca12968a63905` |
| Official upstream | `a63dbc4546895ecb4d1be1a285d98e6e13fb1b74` | `69ff9aeb3deecf21273e9675636f1808e45b3fecc69bf1f2ee039fc679a4a147` |

- Host: AMD Ryzen AI 7 PRO 350, 27 GiB RAM, Linux x86_64.
- Harness: `scripts/bench_startup.py` imported without modification; five
  serialized runs per binary for binary help/version timing, isolated server
  readiness, and cold client startup profiles.
- Each run used isolated `JCODE_HOME`, `JCODE_RUNTIME_DIR`, and
  `JCODE_SOCKET` directories with telemetry disabled. No model, network,
  shared daemon, or user configuration was involved.
- Private startup daemons were terminated by the harness and their temporary
  roots were removed. The raw artifact is
  `/home/ari/.jcode/scratch/jcode-fi8-startup-raw.json`.

### Client/server startup results

Values are p50 from five runs. “Custom delta” is
`(custom - upstream) / upstream`; negative values favor the custom binary.

| Metric | Custom p50 | Upstream p50 | Custom delta | Observation |
|---|---:|---:|---:|---|
| Binary `--help` load | 55.301 ms | 44.376 ms | +24.6% | Custom slower in this harness; noisy process-load path |
| Binary `--version` | 49.817 ms | 45.985 ms | +8.3% | Custom slower; small absolute difference |
| Isolated server socket ready | 137.469 ms | 150.031 ms | -8.4% | Custom faster at p50 |
| Cold client startup total | 1,162.0 ms | 1,164.5 ms | -0.2% | Effectively tied |
| Cold `server_ready` phase | 168.9 ms | 184.5 ms | -8.5% | Custom faster at p50 |
| Cold `app_new_for_remote` phase | 3.7 ms | 3.3 ms | +12.1% | Small absolute difference |
| Remote bootstrap history | 507.0 ms | 530.0 ms | -4.3% | Directionally custom-faster, noisy |

The five-run inclusive p95 estimates for cold total were 1,166.6 ms custom and
1,168.7 ms upstream. The observed p95 estimates for isolated server readiness
were 195.4 ms custom and 168.1 ms upstream, illustrating enough scheduling
noise that these samples should not be treated as a statistical p95 guarantee.

**Finding:** the cold client startup total is effectively equal between these
exact custom and upstream binaries in this harness. The custom binary shows a
modest median advantage in private server readiness, but it is not consistently
faster across the low-level binary-load cases and the sample is too small to
claim a meaningful end-user startup improvement or regression.

### README and design-goal coverage matrix

| Goal or claim | Current evidence | Status in this comparison |
|---|---|---|
| README time to first frame: 14.0 ms historical baseline | README reports ten interactive PTY launches | Not revalidated pairwise; the visible-ready harness requires Python `pyte`, which is unavailable in this environment |
| README time to first input: 48.7 ms historical baseline | README reports ten interactive PTY launches | Not revalidated pairwise for the same dependency reason |
| NFR-002 TUI first-frame and first-input p95 targets | Design target only | Not measured here; cold startup profile is not a first-render/input-ready metric |
| NFR-002 prompt acknowledgement and event-to-client display | Design target only | Not measured; requires a local session/event harness |
| README one-session and ten-session PSS | Historical cross-tool table | Not remeasured custom versus upstream |
| README extra PSS per added session | Historical scaling table | Not remeasured; requires controlled multi-client memory sampling |
| Existing tool CPU/memory profile | `jcode-znm` report includes public daemon/tool-path evidence | Current custom behavior is profiled, but not pairwise against upstream in this Bead |
| Compile-performance plan | `docs/plans/COMPILE_PERFORMANCE_PLAN.md` records warm check/build baselines and targets | Not part of this runtime comparison; compile benchmarks need separate serialized build waves |
| TUI render, reconnect, event fanout, throughput, and memory retention | Architecture/design goals and historical investigations | Unmeasured here |

The README also contains a claim that a Mermaid rendering library is 1800x
faster than a browser/TypeScript path. That is a separate renderer benchmark,
not a client-startup or custom-versus-upstream Jcode comparison.

### Limitations and next measurement lanes

This result compares the exact `jcode-7u8` refs rather than the newer moving
`dev` tip, because reusing the existing binaries was the deliberate way to
avoid another resource-heavy build. The five-run p95 estimates are directional
only. The user-visible first-frame/input lane is blocked by the missing `pyte`
Python dependency, and no dependency installation or replacement parser was
introduced for this quick pass.

The remaining meaningful lanes are: (1) install or vendor the visible-ready
benchmark dependency and run ten PTY samples against both binaries, (2) run a
private local prompt/event acknowledgement and reconnect matrix, (3) compare
one- and ten-session PSS plus incremental PSS, and (4) run the documented
cold/warm compile checkpoints one build at a time. These should remain separate
acceptance lanes rather than being inferred from the cold startup total.
