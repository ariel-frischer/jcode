# Jcode terminal crash and scratch cleanup incident

Status: forensic incident record; the code fix is deferred.
Date: 2026-09-16 through 2026-09-17
Scope: Ariel's local Jcode fork, Linux desktop, Kitty terminal, and Jcode-owned scratch.

## Executive summary

A cleanup pass corrected the immediate filesystem-pressure condition, but it did not correct the Jcode fatal-error path that can abort a client when its terminal or standard error is unavailable. The latest evidence shows two `SIGABRT` cores from the old Jcode build `0cb6f7834` during the session-restore burst. Both cores include the Ratatui panic-hook path and Jcode's `stderr_blank_line` and `report_main_error` functions.

The original Kitty instance was replaced shortly afterward, and the visible symptom was a lost Kitty window. No Kitty coredump was recorded, so direct Jcode-to-Kitty process causation is not proven. The bounded conclusion is that an old Jcode client can double-panic while handling terminal loss or another fatal error, and that failure can cascade through the terminal lifecycle in a way that looks like a Kitty crash.

The storage and panic failures are related operationally but are separate defects:

- Earlier cores contained `No space left on device`, and scratch growth was a real disk-pressure event.
- The latest cores occurred after cleanup with about 407 GiB free on `/home`, about 20 GiB of RAM available, healthy swap, and zero memory PSI pressure. The latest core strings did not contain `ENOSPC`.

## Timeline and actions

### Resource cleanup

The first scratch inventory measured 355,885,809,664 allocated bytes, approximately 331.5 GiB, across 18,781 units. The main contributors were independent build, test, Locus, factoryflowbench, and agent artifacts rather than one Jcode runtime cache.

The following explicitly approved cleanup actions were completed:

1. Verified and cleaned the npm cache, including garbage collection, and cleared the idle Yarn cache.
2. Deleted 32 stale Jcode Rust build/test output roots after a second exact validation. The approved estimate was 184,811,692,032 bytes, approximately 172.1 GiB.
3. Deleted the approved `pytest-of-ari` tree after adding only owner write permission to 72 read-only directories. The tree contained 22 child runs totaling about 61.5 GiB.
4. Freshly revalidated and deleted 1,498 stale generated cache/build/test artifacts. The deletion estimate was 18,833,485,824 allocated bytes, approximately 17.5 GiB. No active process references or deletion errors were found.

After the last cleanup, scratch measured 85,262,835,712 bytes, approximately 79.4 GiB or 85.3 decimal GB. `/home` reported 431 GiB used and 407 GiB available at 52% usage. A memory check reported about 20 GiB available, about 42 GiB of free swap, and zero memory pressure PSI. A verification scan encountered 11 permission-denied temporary subdirectories and preserved them rather than weakening their permissions or deleting them without an exact decision.

The cleanup records are retained outside the repository under `~/.jcode/cleanup/`, including:

- `scratch-deletion-approved-2026-09-16T2319Z.json`
- `pytest-of-ari-deletion-2026-09-16T2331Z.json`
- `scratch-generated-cleanup-20260917T001211Z-2321680.json`

The remaining scratch is not safe to delete as one blanket set. It includes recent artifacts, symlink-bearing trees, reviewed/public outputs, test homes, and permission-protected data. A second cleanup would require a new exact manifest and approval.

### Session restore and crash

The explicitly selected restore batch launched `rat`, `dove`, `hamster`, `otter`, and `mouse`. `crab`, `panda`, and `turtle` were excluded. The five launches were spaced about two seconds apart.

Around 16:39 PDT, two Jcode clients in the original Kitty instance terminated with signal 6 (`SIGABRT`):

- One core identifies the command as the `whale` resumed session.
- The second core identifies an existing Jcode client in the same old Kitty process scope but does not preserve enough command-line data to map it to a session name.

Both cores use `/home/ari/.jcode/builds/versions/0cb6f7834/jcode`. The stack evidence includes:

```text
ratatui::init::set_panic_hook
jcode::cli::output::stderr_blank_line
jcode::cli::startup::report_main_error
```

The source confirms the risky boundary:

- `src/cli/output.rs` defines `stderr_blank_line()` with `eprintln!()`.
- `src/cli/startup.rs` calls that helper from `report_main_error()`.
- `src/cli/terminal.rs` installs a panic hook and watches `SIGHUP`, `SIGTERM`, `SIGINT`, and `SIGQUIT` for terminal/session termination.
- The session resume hint writer is already best-effort, but not every fatal-output path has the same protection.

The old Kitty process was replaced by a new Kitty process about ten seconds after the Jcode aborts. No Kitty coredump was present. This supports a terminal/client lifecycle cascade, not a confirmed Kitty implementation crash. No further mass session restore was attempted.

## Root-cause assessment

### Confirmed

1. A large scratch/build/test accumulation caused genuine filesystem pressure earlier in the incident.
2. The old Jcode build `0cb6f7834` aborts with `SIGABRT` while the panic hook and fatal error reporter are active.
3. `stderr_blank_line()` uses a fallible `eprintln!()` inside fatal-error reporting.
4. Terminal signal watchers run cleanup and resume-hint handling when a terminal disappears.
5. The latest aborts occurred with healthy filesystem and memory headroom, so the latest event was not caused by current OOM or current disk exhaustion.

### Likely but not fully proven

A terminal hangup, closed PTY, broken standard error, or another startup/runtime error caused Jcode to enter fatal reporting. Writing the diagnostic then panicked, and Ratatui's panic hook or terminal restoration created a second panic. Rust converted that double-panic into `SIGABRT`. The resulting client and Kitty process lifecycle made the entire Kitty window appear to crash.

The exact underlying error for the latest two cores was not preserved. Earlier cores did preserve an `ENOSPC` message, so the same reporting defect can be triggered by real disk pressure, but the latest cores do not prove that ENOSPC was the immediate trigger.

## Deferred fix

Bead `jcode-6nhz`, **Make Jcode fatal-error reporting panic-safe when stderr or terminal is unavailable**, is open, labeled `needs-approval`, and intentionally unstarted.

The deferred fix must remain bounded to:

- Best-effort stderr writes and terminal restoration at fatal boundaries.
- Deterministic closed-stderr, `EPIPE`, `ENOSPC`, nested-panic, and terminal-restore tests.
- A nonzero exit with bounded diagnostics even when output fails.
- Redacted diagnostics that never dump environments, credentials, session bodies, or raw terminal-control responses.
- A controlled restore smoke test after the fix, not another mass restore.

No Jcode source, build configuration, provider configuration, or terminal process was changed during this incident response.

## Prevention and operating rules

### Scratch

- Keep the existing 14-day and active-reference protections.
- Treat scratch as shared across Jcode, Locus, factoryflowbench, tests, and worktrees rather than as a single Jcode cache.
- Record owner, task, worktree, newest descendant, symlink state, and active-process checks before deletion.
- Reuse compatible build targets and clean task-owned generated outputs at validation completion.
- Do not treat a numeric budget as permission to delete reviewed/public evidence. Use an exact manifest.

### Crash forensics and secrets

- Never surface raw `kitty @ ls` JSON, `/proc/*/environ`, or complete session dumps.
- Pipe control output through an allowlisted parser that emits only window, tab, title, cwd, and PID fields when those fields are needed.
- Filter coredump and journal output to known safe markers and never print environment fields.
- A credential-bearing environment field was accidentally surfaced during the initial forensic work. No credential value is recorded here. The affected credential must be rotated outside this incident note.

## Risk and rollback

- **Current risk: High.** The old client binary still has a fatal-output double-panic path that can destroy a user-visible terminal workflow during session or I/O failure.
- **Blast radius:** Jcode clients using the affected build and their hosting Kitty tabs or windows. The evidence does not establish that Kitty itself is corrupted or that every session is affected.
- **Mitigations:** Keep the panic-safe Bead deferred but ready, avoid mass restore on the affected build, preserve filesystem reserve, and use redacted forensic commands.
- **Rollback:** The cleanup was limited to exact generated artifacts and cannot be rolled back automatically. Deleted build/test outputs are reproducible. Reviewed/public and recent artifacts were preserved. The code fix should be developed in an isolated worktree and can be reverted as one bounded commit if validation fails.
- **Review action:** Before running `jcode-6nhz`, review the exact test and terminal-boundary plan. Do not infer safety from free disk alone.

## Open issues

- The immediate trigger for the latest two aborts is not preserved in the core strings.
- The second crashed Jcode client cannot be mapped to a session name from available coredump metadata.
- Scratch remains above the desired 60 GiB target because safely retained evidence and recent artifacts dominate the remainder.
- The previously exposed credential still needs rotation.

## References

- [`docs/DISK_SAFETY.md`](../DISK_SAFETY.md)
- [`docs/MEMORY_INCIDENT_RUNBOOK.md`](../MEMORY_INCIDENT_RUNBOOK.md)
- [`src/cli/output.rs`](../../src/cli/output.rs)
- [`src/cli/startup.rs`](../../src/cli/startup.rs)
- [`src/cli/terminal.rs`](../../src/cli/terminal.rs)
- Bead `jcode-6nhz`
