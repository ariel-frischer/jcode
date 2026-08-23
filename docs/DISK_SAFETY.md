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

The equivalent direct commands are:

```bash
python3 scripts/disk_safety.py report
python3 scripts/disk_safety.py check
python3 scripts/disk_safety.py clean
python3 scripts/disk_safety.py clean --apply
```

`make disk-clean` is always a dry-run. `disk-clean-apply` is the only Make
command that requests deletion. Cleanup reports an allocated-byte estimate
reclaimable or reclaimed, not source files removed. Hard-linked files are
counted once within each target scan.

The report emits one bounded record for every Git-registered worktree. It does
not traverse source trees, unregistered checkouts, or paths outside each direct
`target/` tree.

## Configuration

The script uses the first configured value in this order: command-line option, then environment variable, then the built-in default.

| Purpose | Make variable / CLI | Environment | Default |
| --- | --- | --- | --- |
| Minimum filesystem reserve | `DISK_MIN_FREE_BYTES` / `--min-free-bytes` | `JCODE_DISK_MIN_FREE_BYTES` | 10 GiB |
| Minimum target age | `DISK_CLEAN_MIN_AGE_DAYS` / `--min-age-days` | `JCODE_DISK_CLEAN_MIN_AGE_DAYS` | 7 days |

Each configured value must be an unambiguous non-negative decimal integer;
empty, negative, fractional, suffixed, or whitespace-padded forms fail closed.
The threshold stop comparison is inclusive: free bytes less than or equal to
the reserve block Cargo. Only free bytes strictly above the reserve pass.

`scripts/check_guardrails.sh` runs `scripts/disk_safety.py check` before any
Cargo command. Above the threshold, the existing guardrail sequence is
unchanged. Below it, the script stops before Cargo work and points to the
report and reviewable cleanup commands.

## Cleanup safety rules

A candidate must be a direct, non-symlink `target/` directory of a registered
Git worktree inside the repository and its newest nested artifact must be
strictly older than the configured age. An artifact exactly at the age boundary
is excluded.
Cleanup skips:

- the main checkout, source files, and Git branches;
- worktrees with an active Jcode session or active Cargo/rustc/Jcode process;
- dirty worktrees, including untracked files;
- recent targets, empty targets, or targets whose artifact age is unknown;
- missing, unreadable, symlinked, nested, or otherwise non-direct targets;
- unregistered worktrees; and
- worktrees or target paths outside the repository.

Before each explicit removal, the apply path re-reads registered worktrees and
active-session state, checks active build processes and Git status again, and
revalidates containment, the non-symlink target invariant, and newest-artifact
age. A live PID marker or relevant process whose metadata cannot be trusted
makes activity uncertain. Dry-run reports that uncertainty without deleting;
apply fails closed before deleting any candidate. If any batch or immediate
deletion-boundary invariant cannot be proven, cleanup stops rather than
continuing to another candidate. Cargo will regenerate removed artifacts on the
next build or validation command. Generated targets are the only removed paths;
source, branch data, worktree roots, and shared Cargo caches are never selected.

## Focused validation

The synthetic fixture tests cover threshold boundaries, malformed settings,
containment, active/dirty/recent exclusions, nested artifact recency, uncertain
live-session metadata, deletion-time revalidation, dry-run versus apply, Make
target exposure, and guardrail ordering:

```bash
python3 scripts/test_disk_safety.py
```
