# Render-cache investigation decision

## Decision: REJECT runtime candidate, preserve baseline

No runtime candidate was retained. The baseline-first run used the unchanged revision `4bc21f92ecef08eae8a894b66eb16cb290821f56` and the newly built `target/selfdev/tui_bench` through Ratatui `TestBackend` and `jcode::tui::render_frame`.

Five fresh-process samples were collected for short/large fixtures at widths 80 and 160. Warm-frame p50 medians were: short/80 `0.274227 ms`, short/160 `0.392861 ms`, large/80 `0.613687 ms`, large/160 `0.807331 ms`. The existing harness did not expose CPU, allocation count/bytes, allocator active/resident/retained memory, peak RSS, explicit cache classification, invalidation, bounded eviction, display-mode/diagram-mode, or concurrent-reader/session-isolation cells. These omissions make the required 100% matrix and 10%/3% gate incomplete, so no candidate comparison or acceptance claim is valid.

## Ownership trace

`MessageCacheState::get` currently clones `Arc<Vec<Line<'static>>>` into an owned `Vec`. The public `get_cached_message_lines` contract is Vec-returning. Direct `ui_prepare` consumers inspect, align, map, and move lines into owned `PreparedMessages`; `ui_viewport` truncates and extends owned lines. An immutable Arc view would recreate the same deep materialization at the adapter boundary or require broad PreparedMessages/renderer redesign. Both violate scope. Therefore no runtime cache/API files were changed.

## Correctness and validation

The unchanged cache crate tests passed: `cargo test -p jcode-tui-messages`, 9 tests. The benchmark built successfully with `cargo build --profile selfdev --features dev-bins --bin tui_bench`. A first build without `--features dev-bins` failed because the target requires that feature, then the corrected command passed. `/usr/bin/time` was unavailable on this host, so process CPU/RSS collection was not fabricated. `git diff --check` passed.

## Requirement traceability

- FR-001: `baseline.yaml`, unchanged revision and timestamp.
- FR-002/003/004: partial only. Four representative size/width controls measured; required dimensions and resource fields explicitly marked incomplete.
- FR-005: Bead comment records release at 2026-08-09 07:00 UTC.
- FR-006 through FR-010: unchanged runtime and focused crate tests; no candidate claim.
- FR-011/012: candidate rejected because ownership proof and incomplete mandatory matrix prevent a safe general-purpose acceptance decision; runtime files unchanged.
- NFR-001/002: not evaluable without paired candidate and complete metrics.
- NFR-003: baseline crate tests pass; full candidate parity matrix not applicable.
- NFR-004: satisfied by no runtime change and no dependency.

## Risk

### Low

- Rationale: runtime cache behavior and public Vec API are unchanged.
- Blast radius: benchmark/report artifacts only.
- Mitigation/rollback: delete the investigation artifacts or revert the documentation-only commit.
- Required review action: maintainer review of the negative result.

### Medium

- Rationale: the current benchmark harness cannot substantiate allocation or memory claims.
- Blast radius: future performance decisions based on this harness.
- Mitigation/rollback: add a separately scoped benchmark-instrumentation task with allocator/process metrics before reconsidering the candidate.
- Required review action: do not approve a runtime optimization from this report alone.

### High

- Rationale: accepting an Arc view without end-to-end ownership evidence could move equivalent cloning into callers or require an out-of-scope renderer redesign.
- Blast radius: TUI render latency, memory, immutability, and public API compatibility.
- Mitigation/rollback: retain the baseline and require complete paired representative evidence before any future prototype.
- Required review action: explicit maintainer approval before reopening runtime work.

## Finding for `docs/TOOL_PERFORMANCE_PROFILE.md`

Render-cache investigation (jcode-g25, 2026-08-09): the cache stores `Arc<Vec<Line<'static>>>` but its public and direct TUI consumers require owned `Vec<Line>` values for alignment, mapping, truncation, and preparation. A shared Arc view would either recreate the same deep clone at the ownership boundary or require a broad PreparedMessages/renderer redesign. The baseline TestBackend run measured warm-frame p50 medians of 0.274 ms (1 KiB/80), 0.393 ms (1 KiB/160), 0.614 ms (64 KiB/80), and 0.807 ms (64 KiB/160) across five fresh processes, but the harness lacked CPU/allocation/memory/cache-classification and invalidation/concurrency cells. Result: reject runtime change and preserve the baseline. Revisit only with complete paired metrics and representative ownership proof.
