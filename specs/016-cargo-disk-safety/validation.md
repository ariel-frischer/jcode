# Cargo disk-safety validation

Validated on 2026-08-23 in the isolated `016-cargo-disk-safety` worktree.

## Focused receipts

- PASS — `python3 -m unittest scripts/test_disk_safety.py`: 31 tests in 0.591s after the focused CLI lexical-boundary repair.
- PASS — `python3 -m py_compile scripts/disk_safety.py scripts/test_disk_safety.py`.
- PASS — `bash -n scripts/check_guardrails.sh`.
- PASS — `make -n disk-help disk-report disk-check disk-clean disk-clean-apply`; the dry-run target has no `--apply`, and only `disk-clean-apply` supplies it.
- PASS — real `make disk-help` and `make disk-check`; the latter reported about 160.7 GiB free against the 10 GiB effective reserve before any Cargo work.
- NOT COUNTED — a chained real `make disk-report` scan did not finish inside the managed command window while traversing existing registered targets. It was not rerun unchanged; report behavior is covered by deterministic registered-worktree fixtures.
- PASS — `git diff --check`.
- BASELINE FAILURE — `python3 scripts/check_code_size_budget.py` and `python3 scripts/check_test_size_budget.py` report existing Rust files changed between the ratchet baseline and current `dev`; neither report names an owned disk-safety file and no baseline was updated.
- SCOPED PASS / BASELINE FAILURE — durable `scripts/check_guardrails.sh --skip-slow` receipt at `/home/ari/.jcode/scratch/disk-safety-guardrails-016/run-2/output.log`: reserve preflight passed at about 159.2 GiB free; module resolution, rustfmt, Cargo.lock, dependency boundaries, wildcard re-exports, desktop frame budget, and onboarding invariants passed. Exit 1 came only from five unrelated current-branch ratchets: warning, oversized production files, oversized Rust tests, panic-prone usage, and swallowed-error usage. No failing receipt names an owned disk-safety file; no ratchet was weakened or rebaselined.

No developer-worktree cleanup apply command was run. The only apply execution is `test_apply_uses_only_disposable_git_worktrees_and_preserves_exclusions`, whose repository, registered linked worktrees, targets, Jcode metadata, source sentinel, and external sentinel all live under `tempfile.TemporaryDirectory()`.

## Acceptance matrix

| Requirement | Direct check |
| --- | --- |
| FR-001 | `test_report_has_one_estimate_record_for_every_registered_worktree` asserts one uncapped record for all 30 registrations. |
| FR-002 | `test_report_distinguishes_absent_and_unsafe_targets` and `test_target_observation_deduplicates_inodes_and_ignores_symlinks`. |
| FR-003 | Report, dry-run, and apply assertions require the literal `allocated-byte estimate` wording. |
| FR-004 | `test_guardrails_low_space_exits_before_invoking_cargo`, `test_guardrails_malformed_reserve_exits_before_invoking_cargo`, and the static preflight-before-first-Cargo assertion. |
| FR-005 | `test_threshold_boundary_is_inclusive` covers threshold - 1, equality, and threshold + 1. |
| FR-006 | `test_invalid_configuration_is_rejected`, `test_check_rejects_malformed_environment_with_actionable_exit_2`, and `test_cli_preserves_lexical_configuration_for_strict_validation`. |
| FR-007 | `test_cleanup_dry_run_preserves_target_and_apply_reports_bytes` plus Make dry-run/apply resolution assertions. |
| FR-008 | `test_target_must_be_contained_direct_non_symlink_directory`, report state tests, and disposable registered-worktree apply. |
| FR-009 | `test_cleanup_excludes_main_active_dirty_and_recent_worktrees` and the disposable apply preservation assertions. |
| FR-010 | Live-session discovery and `test_relevant_process_metadata_is_active_or_fail_closed`. |
| FR-011 | `test_live_session_with_unreadable_metadata_fails_closed`, uncertain dry-run reporting, and relevant-process incomplete-metadata coverage. |
| FR-012 | Strict equality coverage in `test_cleanup_age_requires_strictly_older_newest_artifact`, nested newest-artifact coverage, and empty-target unknown-age coverage. |
| FR-013 | Dirty/active/recent, unregistered, post-scan activity, path/symlink, and batch revalidation tests exercise the final boundary. |
| FR-014 | `test_batch_revalidation_failure_preserves_every_candidate` proves complete-set validation precedes deletion. |
| FR-015 | Disposable Git apply proves only the eligible direct target is removed while source, roots, registrations, dirty target, and external sentinel remain. |
| FR-016 | `test_cleanup_dry_run_reports_every_exclusion_and_selected_estimate`. |
| FR-017 | The sole end-to-end apply test constructs all paths beneath a temporary directory; no real apply command appears in these receipts. |
| FR-018 | `test_make_disk_help_and_dry_run_commands_are_exposed` and the five-target `make -n` receipt. |
| FR-019 | `test_disk_safety_documentation_covers_the_accepted_contract`. |
| NFR-001 | All negative-path and batch-revalidation tests assert zero unsafe deletions. |
| NFR-002 | Inclusive reserve boundary and zero-Cargo-start tests. |
| NFR-003 | Deterministic report/exclusion records and actionable configuration/preflight assertions. |
| NFR-004 | Registration-list inputs and direct-target-only observation tests; no source or unregistered traversal API exists. |
| NFR-005 | FR-006 through FR-015 each map above to focused negative-path coverage. |

## Safety review

Deletion authority remains in one path: explicit `clean --apply` creates candidates only from Git porcelain registrations, then validates the entire candidate batch and revalidates registration, session/process activity, dirty state, repository containment, direct-child identity, non-symlink status, readability, and strict recency immediately before each `shutil.rmtree(target)` call. The removed path is always the candidate worktree's direct `target` directory. Source files, worktree roots, branches, the main checkout, shared caches, symlinks, and paths outside the repository are never passed to deletion.
