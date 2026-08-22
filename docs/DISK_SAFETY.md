# Disk safety for Jcode development

Jcode worktrees can accumulate large Cargo `target/` trees. The disk-safety
workflow reports filesystem headroom, blocks broad Cargo guardrails when the
configured reserve is unavailable, and reclaims only explicitly selected stale
worktree targets.

## Commands

Run these from any checkout of this repository:

```bash
make disk-help
make disk-report                         # bounded headroom and target inventory
make disk-check                          # the same reserve check used by guardrails
make disk-clean                          # dry-run, never deletes
make disk-clean-apply                    # explicit deletion after reviewing dry-run
```

`make disk-clean` is always a dry-run. `disk-clean-apply` is the only Make
command that requests deletion. Cleanup reports logical bytes reclaimable or
reclaimed, not source files removed.

The report is bounded to 25 worktrees by default. Override the bound when
needed:

```bash
make disk-report DISK_MAX_WORKTREES=50
```

## Configuration

The script uses the first configured value in this order: command-line option,
then environment variable, then the built-in default.

| Purpose | Make variable / CLI | Environment | Default |
| --- | --- | --- | --- |
| Minimum filesystem reserve | `DISK_MIN_FREE_BYTES` / `--min-free-bytes` | `JCODE_DISK_MIN_FREE_BYTES` | 10 GiB |
| Minimum target age | `DISK_CLEAN_MIN_AGE_DAYS` / `--min-age-days` | `JCODE_DISK_CLEAN_MIN_AGE_DAYS` | 7 days |
| Report and cleanup bound | `DISK_MAX_WORKTREES` / `--max-worktrees` | `JCODE_DISK_MAX_WORKTREES` | 25 |

The threshold comparison is inclusive: free space equal to the reserve passes.
Malformed or negative values fail closed with an actionable error.

`scripts/check_guardrails.sh` runs `scripts/disk_safety.py check` before any
Cargo command. Above the threshold, the existing guardrail sequence is
unchanged. Below it, the script stops before Cargo work and points to the
report and reviewable cleanup commands.

## Cleanup safety rules

A candidate must be a direct, non-symlink `target/` directory of a registered
Git worktree inside the repository and must be older than the configured age.
Cleanup skips:

- the main checkout, source files, and Git branches;
- worktrees with an active Jcode session or active Cargo/rustc/Jcode process;
- dirty worktrees, including untracked files;
- targets newer than the age threshold;
- missing, symlinked, or path-traversal targets; and
- worktrees or target paths outside the repository.

The apply path revalidates containment and the non-symlink target invariant
immediately before removal. If that invariant cannot be proven, it refuses to
remove anything in question. Cargo will regenerate removed artifacts on the
next build. A dry-run is the rollback point because generated artifacts are
rebuildable, but cleanup does not restore the disk space used by source or
branch data because those are never selected.

## Focused validation

The synthetic fixture tests cover threshold boundaries, malformed settings,
containment, active/dirty/recent exclusions, dry-run versus apply, Make target
exposure, and guardrail ordering:

```bash
python3 scripts/test_disk_safety.py
```
