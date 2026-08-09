# Render-cache benchmark design

Status: executed baseline-first investigation, candidate not retained.

## Revision and controls

- Worktree: `agent/jcode-g25-render-cache`
- Baseline revision: `4bc21f92ecef08eae8a894b66eb16cb290821f56`
- Host: Linux x86_64, Ryzen AI 7 PRO 350, 27 GiB RAM
- Build: stable Rust, `--profile selfdev`, dev-bins `tui_bench`
- Samples: five isolated process invocations per representative control, after one warmup invocation. No shared daemon or production session is used.
- Fixtures: existing deterministic `tui_bench` assistant transcript, with approximately 1 KiB and 64 KiB assistant lengths where the harness permits. No private session data.

## Workload matrix

The authoritative path is a real Ratatui `TestBackend` frame through `jcode::tui::render_frame`. Controls use widths 80 and 160, unchanged-body repeated frames, and fresh-process cold starts. The existing harness also provides file-diff and streaming controls. Cache-local attribution is supplementary only.

Required conceptual cells are short/large hit and miss, widths, applicable diff and diagram modes, invalidation, bounded eviction, one/four readers, concurrent session isolation, and unchanged-body controls. A cell is incomplete when CPU, wall, allocation count, allocated bytes, allocator memory, retained memory, or peak RSS is unavailable.

## Metrics and decision formula

Each sample should contain `cpu_ns`, `wall_ns`, `allocation_count`, `allocated_bytes`, allocator `allocated/active/resident/retained` bytes, `peak_rss_bytes`, cache classification, and correctness observations. CPU and wall are measured per process interval. Allocation and allocator memory require jemalloc boundary snapshots with epoch refresh. Peak RSS requires process sampling. Missing fields never pass.

Candidate acceptance requires both short and large representative TestBackend hits to improve median CPU and allocated volume by at least 10% over five comparable samples. Every miss, invalidation, latency, and memory cell must regress no more than 3%, all cells must be complete, and correctness must have zero failures. The public Vec-returning API remains the compatibility contract.

## Clone reachability finding

`MessageCacheState` stores `Arc<Vec<Line<'static>>>`, but `get_cached_message_lines` returns an owned `Vec<Line<'static>>` by cloning the complete vector. Direct consumers in `ui_prepare.rs` immediately iterate by value, perform alignment changes, inspect lines for copy targets and mappings, and append them into owned `PreparedMessages`; `ui_viewport.rs` likewise consumes an owned vector and may truncate/extend it. An Arc-only internal view would therefore either materialize the same Vec/Line clone at the adapter boundary or require a broader PreparedMessages/renderer ownership redesign. That redesign is out of scope. The public API cannot change.

## Commands

Baseline controls are run with the newly built binary and five isolated invocations per selected width/size control. Resource output is captured by `/usr/bin/time -v` and the benchmark's JSON summary. The existing harness does not expose the complete jemalloc allocation schema or render-cache classification matrix, so those fields are explicitly incomplete rather than inferred.
