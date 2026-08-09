# Render-cache investigation finding

- **Bead:** jcode-g25, terminal-B investigation-only negative result
- **Revision:** `4bc21f92ecef08eae8a894b66eb16cb290821f56`
- **Finding:** `jcode-tui-messages` stores `Arc<Vec<Line<'static>>>`, but the public API and direct TUI consumers require owned `Vec<Line>` values for alignment, mapping, truncation, and preparation. A shared view would either recreate the deep clone at the ownership boundary or require an out-of-scope PreparedMessages/renderer redesign.
- **Baseline:** five fresh TestBackend processes per cell, warm p50 medians: 1 KiB/80 `0.274 ms`, 1 KiB/160 `0.393 ms`, 64 KiB/80 `0.614 ms`, 64 KiB/160 `0.807 ms`.
- **Decision:** reject runtime change. The existing harness lacks required CPU, allocation, allocator-memory, peak-RSS, cache-classification, invalidation, eviction, display-mode, and concurrency cells, so no 10%/3% paired gate can be claimed. Revisit only with complete instrumentation and representative ownership proof.
