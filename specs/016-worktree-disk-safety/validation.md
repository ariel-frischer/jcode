# Validation evidence for jcode-6gp

Date: 2026-08-23
Branch: `agent/jcode-6gp-disk-safety`
Base reviewed: `dev` at `29d7ec132`

## Autospec

- Bead policy: `autospec:required`.
- The configured `oauth-cheap` native Jcode SDK attempt was stopped after it reported that command execution is unsupported. No provider or credential fallback was used.
- The accepted Bead contract was reconstructed as `spec.yaml`, `plan.yaml`, and `tasks.yaml` in this directory.
- Passed:
  - `autospec artifact specs/016-worktree-disk-safety/spec.yaml`
  - `autospec artifact specs/016-worktree-disk-safety/plan.yaml`
  - `autospec artifact specs/016-worktree-disk-safety/tasks.yaml`

## Focused tests and static checks

Passed after test-first repair of the preserved draft:

- `timeout 60s python3 scripts/test_disk_safety.py`
  - 14 focused tests passed.
  - Coverage includes threshold boundaries, malformed configuration, path containment, registered-worktree enforcement, main/active/dirty/recent exclusions, nested artifact recency, uncertain live-session metadata, dry-run, temporary-fixture apply, post-scan activity revalidation, Make target exposure, and behavioral proof that low space exits before invoking Cargo.
- `python3 -m py_compile scripts/disk_safety.py scripts/test_disk_safety.py`
- `bash -n scripts/check_guardrails.sh`
- `git diff --check`
- Ruff and Black were not installed, so their optional checks were skipped.

## Real repository acceptance, non-destructive only

Passed:

- `make disk-check`
  - Free: 175.5 GiB.
  - Required reserve: 10.0 GiB.
- `make disk-report`
  - Reported 30 registered worktrees, bounded to 25 entries with 5 explicitly omitted.
  - Reported filesystem headroom and allocated-byte target estimates with main, active, and dirty flags.
- `make disk-clean`
  - Dry-run only.
  - Reported 0 B reclaimable in the inspected set.
  - No target, source, branch, or worktree was removed.
- No live `disk-clean-apply` command was executed. Apply behavior was exercised only inside temporary fixtures.

## Repository guardrails

Ran after the disk preflight passed:

- `scripts/check_guardrails.sh --skip-slow` under the coordinated self-dev test lock.
- Passed: disk preflight, module declaration resolution, `cargo fmt --all --check`, Cargo.lock metadata, dependency boundaries, wildcard re-export ratchet, desktop2 frame-budget sweep, and focused onboarding state-space invariants.
- The command returned nonzero for five unrelated repository baseline failures: warning budget, oversized production-file ratchet, oversized test-file ratchet, panic-prone usage ratchet, and swallowed-error ratchet.
- Each same failing script also returned nonzero from the clean `dev` checkout. This change adds no Rust files or warning/panic/swallowed-error usage and does not update unrelated baselines.

## Independent safety review

A read-only OpenAI GPT-5.6 Sol reviewer inspected the deletion path, tests, and docs.

- Initial blocker: activity/dirty checks occurred before the potentially long target scan.
- Repair: `_revalidate_candidate` now repeats registered-worktree, live-session/process, Git dirty-state, and safe-path checks after the scan, immediately before a caller may remove the target.
- Added a regression where activity begins between the pre-scan and post-scan checks.
- Changed byte reporting from logical file sizes to an allocated-byte estimate with per-target inode de-duplication, and documented that cross-target/outside hard links may make the estimate conservative.
- Final reviewer conclusion: no merge blocker remains.

## Risk

**Medium.** The feature contains an explicit deletion command, but its blast radius is limited to generated `target/` directories of registered in-repository non-main worktrees. Dry-run is the default, apply is explicit, volatile safety state is revalidated after scanning, uncertain activity fails closed, and Cargo rebuilds removed artifacts. Rollback is rebuilding the affected target. Landing requires Ariel approval under the cleanup/deletion risk gate.
