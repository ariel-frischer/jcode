<!-- markdownlint-disable MD013 MD060 -->

# Quality Ratchet Reconciliation Validation

## Acceptance boundary

This document is the durable requirement-to-check matrix and receipt index for feature `015-reconcile-quality-ratchets`. A requirement is complete only when its assigned direct check has a current passing receipt from the final repository state. Planned checks and historical results are not passing evidence.

### Fixed execution rules

- **Branch base:** all blocker-branch oversized-file comparisons use commit `85f9f5e3b`.
- **Runtime truth:** built-product checks use the resolved worktree binary at `target/selfdev/jcode` with a unique private socket. They must not exercise, restart, or repoint `~/.jcode/builds/shared-server/jcode`.
- **Serialized heavy validation:** Cargo-heavy builds, tests, Clippy, and `scripts/check_guardrails.sh` run serially. No overlapping Cargo-heavy receipt is accepted.
- **No automatic ratchet updates:** `--update`, generated update output, exemptions, or bypasses are prohibited as ownership evidence and must not be used to reconcile a baseline.
- **Stable toolchain:** canonical guardrail receipts use the current stable Rust toolchain or explicitly disclose toolchain drift.
- **Current-state evidence:** a stale failed or passing receipt must be explicitly superseded. The final acceptance set must be internally consistent.

## Direct check catalog

| Check | Class | Direct check and pass condition | Primary tasks |
|---|---|---|---|
| C01 | focused | Run `python3 scripts/check_quality_ratchet_provenance.py`. Pass only when every adjusted code-size, test-size, and swallowed-error scope has complete ledger fields, matching previous/current budget values, valid causal and baseline commits, accepted merged ancestry, reconciliation Bead `jcode-l89.5`, an honest bounded historical-Bead result, and resolved review-disposition references. Missing, duplicate, mismatched, fabricated, reverted, superseded, disputed, or untraced records must fail. | T003, T017, T019, T020, T025 |
| C02 | focused | Compare every modified already-oversized production and test file with `85f9f5e3b`. Pass only when the complete file-by-file inventory has zero positive deltas, including growth moved into another oversized file. | T004, T009-T16, T034 |
| C03 | focused | Review the stable independent-review inventory and validate its ledger references. Pass only when every applicable PR/review/Bead-comment identifier has an owner, affected scope, final disposition of `implemented`, `rejected_with_evidence`, or `out_of_scope`, and evidence satisfying its resolution condition. | T002, T019, T027, T034 |
| C04 | focused | Run deterministic negative fixtures for the provenance validator and the production-size, test-size, and swallowed-error ratchets. Pass only when incomplete provenance and controlled positive per-file, per-pattern, and aggregate growth are rejected with non-zero, actionable failures while accepted values still pass. | T017, T018, T025 |
| C05 | focused | Run the three canonical ratchets: `python3 scripts/check_code_size_budget.py`, `python3 scripts/check_test_size_budget.py`, and `python3 scripts/check_swallowed_error_budget.py`. Pass only when all succeed against the reconciled state and a scoped diff proves unrelated thresholds were not weakened. | T022-T025 |
| C06 | behavior | Run exact-name or narrow-module app-core, harness, telemetry, TUI, ACP, and provider regression tests identified by T026-T028. Pass only when accepted success behavior is unchanged, invalid/error paths remain actionable and observable, no new swallowed error appears, and any moved or consolidated test has equivalent or stronger coverage. | T005-T008, T026-T029, T036-T038 |
| C07 | broad | Run `scripts/check_guardrails.sh` once, serially, on stable Rust after focused checks pass. Pass only when provenance executes before the three ratchets and the canonical formatting, all-target/all-feature, Clippy-with-warnings-denied, lockfile, warning, size, panic, swallowed-error, dependency, re-export, frame-budget, and unused-dependency gates succeed without unrelated threshold changes. | T021, T030 |
| C08 | built-product | Run `cargo build --profile selfdev`, resolve `target/selfdev/jcode`, then execute deterministic public and integration scenarios with `target/selfdev/jcode run --no-update --socket <unique-private-socket> '<prompt>'`. Pass only when public command, server, provider-mock, tool, and explicit error-path outcomes match accepted behavior and evidence confirms the shared daemon was neither measured nor disturbed. | T031-T033 |

## Requirement-to-check acceptance matrix

Each row maps to one direct check. Supporting evidence may be referenced by that check, but completion is determined by the single assigned check.

| Requirement | Class | Direct check | Passing evidence required |
|---|---|---|---|
| FR-001 | focused | C01 | Every adjusted baseline is tied to intentional active merged state. Every untraced, reverted, superseded, or disputed delta is rejected. |
| FR-002 | focused | C01 | Complete and valid provenance exists for 100% of adjusted production code-size scopes. |
| FR-003 | focused | C01 | Complete and valid provenance exists for 100% of adjusted test-size scopes. |
| FR-004 | focused | C01 | Complete and valid provenance exists for 100% of adjusted swallowed-error scopes and dimensions. |
| FR-005 | focused | C03 | Every applicable independent review concern has a stable identifier and final evidence-backed disposition. |
| FR-006 | focused | C02 | The full changed oversized-file audit against `85f9f5e3b` reports zero positive deltas. |
| FR-007 | behavior | C06 | All affected focused behavior and public provider scenarios retain accepted observable outcomes. |
| FR-008 | behavior | C06 | No accepted coverage boundary regresses. Removed, moved, or consolidated tests demonstrate equivalent or stronger direct coverage. |
| FR-009 | behavior | C06 | Focused exceptional paths remain explicit and observable, and the changed source surface has no newly swallowed error. |
| FR-010 | focused | C04 | Controlled post-reconciliation growth is rejected for production LOC, test LOC, and swallowed-error per-pattern, per-file, and aggregate dimensions. |
| FR-011 | focused | C05 | Provenance plus all three directly affected canonical ratchets pass against the reconciled repository state. |
| FR-012 | broad | C07 | The serialized stable all-feature guardrail suite passes with unrelated thresholds unchanged. |
| FR-013 | behavior | C06 | Every identified affected workflow and explicit error path has a passing exact-name or narrow-module test receipt. |
| FR-014 | built-product | C08 | The resolved worktree selfdev binary passes isolated public-interface and integration scenarios on a private socket. |
| SC-001 | focused | C01 | Provenance completeness is 100% for all adjusted historical quality deltas without undocumented ownership assumptions. |
| SC-002 | focused | C02 | Modified already-oversized files with positive branch-base growth equal `0`. |
| SC-003 | built-product | C08 | Required built-product public and integration scenarios pass at 100% with unchanged outcomes. |
| SC-004 | focused | C04 | 100% of sampled controlled future-growth and newly swallowed-error violations are rejected. |
| SC-005 | focused | C03 | 100% of applicable independent review comments have transparent final dispositions. |

## Required pass receipts

Receipts are appended or updated by later tasks. Each receipt must include the check ID, requirement IDs, exact non-secret command, UTC timestamp, resolved worktree and commit, result, durable or reproducible artifact reference, and any explicitly superseded receipt ID. A receipt may report only `PASS`, `FAIL`, or an explicitly approved `BLOCKED` state. Only `PASS` satisfies the matrix.

### R01: Provenance completeness

- **Check:** C01
- **Pass receipt:** exact provenance-validator command and output summary; ledger/budget diff; counts by category; evidence that every changed scope has required ownership, bounded Bead-search, merged-state, and review fields; explicit rejected-delta count and reasons.
- **Pending owner:** T025 and T033.

### R02: Changed oversized-file deltas

- **Check:** C02
- **Pass receipt:** exact comparison command or script; base `85f9f5e3b`; complete modified oversized-file list; base/current measured sizes and deltas; assertion that positive-delta count is zero and growth was not shifted to another oversized file.
- **Pending owner:** T016 and T034.

### R03: Focused behavior, coverage, and error paths

- **Check:** C06
- **Pass receipt:** exact package/module/test commands and results for app-core lifecycle and bash, harness translation, telemetry recovery and worker-spawn failures, local/remote TUI submission and path handling, ACP explicit errors, and affected provider behavior; coverage equivalence notes for any moved or consolidated tests; swallowed-error changed-surface result.
- **Pending owner:** T026-T029.

### R04: Canonical guardrails

- **Check:** C07
- **Pass receipt:** exact stable-toolchain `scripts/check_guardrails.sh` command, start/end timestamps demonstrating serialized Cargo-heavy execution, successful gate summary, provenance-before-ratchets ordering, and diff evidence that unrelated thresholds remain unchanged.
- **Pending owner:** T030 and T033.

### R05: Built selfdev binary

- **Check:** C08
- **Pass receipt:** exact `cargo build --profile selfdev` command; resolved executable path and identity for `target/selfdev/jcode`; unique private socket path; exact deterministic public and integration invocations; expected and observed outcomes; confirmation that the shared daemon symlink/process was not used, restarted, or repointed.
- **Pending owner:** T031-T033.

### R06: Future-growth rejection

- **Check:** C04
- **Pass receipt:** exact deterministic fixture commands and results proving rejection of incomplete provenance, positive production LOC, positive test LOC, and per-pattern, per-file, and aggregate swallowed-error growth while accepted current values pass.
- **Pending owner:** T017, T018, and T025.

### R07: Independent review dispositions

- **Check:** C03
- **Pass receipt:** complete stable-ID inventory with bounded concern, affected paths/scopes, owner, evidence requirement, final disposition, and final-resolution evidence; zero unresolved applicable concerns.
- **Pending owner:** T002, T027, and T034.

## Final consistency gate

T033 may declare the acceptance run complete only when R01 through R07 are current and passing, every FR-001 through FR-014 and SC-001 through SC-005 row resolves to its assigned passing check, no failed receipt is left ambiguous, and the final diff remains within the approved reconciliation scope.
