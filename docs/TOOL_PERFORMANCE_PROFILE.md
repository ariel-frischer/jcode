# Common Tool CPU and Memory Profile

Status: current-state measurement and implemented optimization  
Measured: 2026-08-08  
Bead: `jcode-skr`

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
