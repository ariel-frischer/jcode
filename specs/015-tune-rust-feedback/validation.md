# Rust Feedback Tuning Validation

Status: **Scaffold frozen, evidence pending**  
Feature: `specs/015-tune-rust-feedback`  
Source specification: `specs/015-tune-rust-feedback/spec.yaml`  
Technical plan: `specs/015-tune-rust-feedback/plan.yaml`

## Result convention

Every terminal result cell is completed with the exact command outcome and exit code. Use one of:

- `PASS (exit 0)`
- `FAIL (exit <code>): <short reason>`
- `N/A: <documented reason>`
- `PENDING`

Machine-readable benchmark evidence belongs under `specs/015-tune-rust-feedback/evidence/`. Adopt-or-reject conclusions belong in `specs/015-tune-rust-feedback/decisions.yaml`. This document indexes those artifacts rather than replacing their raw evidence.

## Requirement traceability

| Requirement | Intended direct check | Evidence path | Terminal result |
|---|---|---|---|
| FR-001 | `python3 -m unittest scripts.test_bench_rust_feedback.BenchmarkMatrixContractTests` verifies all six scenario classes, preparation, policy, validity, and evidence fields. | `scripts/rust_feedback_matrix.json`; `scripts/test_bench_rust_feedback.py`; benchmark receipt index below | PENDING |
| FR-002 | Benchmark receipt contract tests validate baseline/candidate identity, revision, dirty fingerprint, lockfile, toolchain, host/resources, configuration, timestamp, matrix version, compatibility disclosure, and redaction. | `scripts/test_bench_rust_feedback.py`; `specs/015-tune-rust-feedback/evidence/*.json` | PENDING |
| FR-003 | Receipt contract and runner fixture tests reject missing or inconsistent wall, execution, queue, gate, RSS, swap, cache, exit, retry, and action-count fields while accepting explicit not-applicable values. | `scripts/test_bench_rust_feedback.py`; `specs/015-tune-rust-feedback/evidence/*.json` | PENDING |
| FR-004 | Deterministic aggregation fixtures verify retained valid/invalid/retried samples, p50, nearest-rank p95, invalid exclusion, retry accounting, and report reproduction. | `scripts/test_bench_rust_feedback.py`; comparison receipts indexed below | PENDING |
| FR-005 | Execute equivalent isolated one-action benchmark lanes for adaptive, explicit 6-job, and explicit 8-job policies, then validate complete comparable sample sets. | `specs/015-tune-rust-feedback/evidence/*jobs*.json`; benchmark receipts section below | PENDING |
| FR-006 | Run the coordinated duplicate scenario and assert coordinator-authorized execution, global-gate ownership, and consistent requested/executed/follower/reused/coalesced counts. | `crates/jcode-app-core/src/tool/selfdev/tests.rs`; `specs/015-tune-rust-feedback/evidence/*duplicate*.json` | PENDING |
| FR-007 | `python3 -m unittest scripts.test_rust_validation_scope` verifies narrow eligible package/target selection and conservative fallback for ambiguous, generated, build-script, cross-package, workspace, broad, and release-sensitive paths. | `scripts/test_rust_validation_scope.py`; focused validation receipts | PENDING |
| FR-008 | Scope resolver and shell integration tests cover explicit feature, no-default-feature, all-feature, inferred, empty, unset, conflicting, and unsupported combinations with explicit input winning. | `scripts/test_rust_validation_scope.py`; `scripts/test_dev_cargo_scope.sh` | PENDING |
| FR-009 | Optional-feature regression fixtures prove insufficient feature sets fail and affected paths either receive required features or broaden to full-feature validation. | `scripts/test_rust_validation_scope.py`; full-feature guardrail section below | PENDING |
| FR-010 | Shell integration assertions verify receipts report configured and effective package, target, features, and `explicit`, `inferred`, `default`, or `conservative_fallback` resolution source. | `scripts/test_dev_cargo_scope.sh`; focused action receipts | PENDING |
| FR-011 | Exact Rust selfdev test proves a current complete discovery snapshot rejects a provably empty package/filter before producer claim, queue wait, Cargo-gate acquisition, or execution. | `crates/jcode-app-core/src/tool/selfdev/tests.rs`; zero-test receipt | PENDING |
| FR-012 | Exact Rust and Python tests cover positive, ambiguous, stale, ignored, integration, library, exact-name, and substring cases and preserve execution when emptiness is not provable. | `crates/jcode-app-core/src/tool/selfdev/tests.rs`; `scripts/test_rust_validation_scope.py` | PENDING |
| FR-013 | Run the bounded nextest-compatible focused-test lane, validate availability, compatibility, timing, resources, disk, reporting, fallback, and final decision. | `specs/015-tune-rust-feedback/evidence/*nextest*.json`; `specs/015-tune-rust-feedback/decisions.yaml` | PENDING |
| FR-014 | Run bounded non-incremental fresh/cold sccache miss and reusable-hit lanes, including cache failure fallback and proof warm incremental defaults remain unchanged. | `specs/015-tune-rust-feedback/evidence/*sccache*.json`; `specs/015-tune-rust-feedback/decisions.yaml` | PENDING |
| FR-015 | Decision validator requires complete baseline/candidate evidence, lower p50 and p95, unchanged failure/retry rate, acceptable RSS/swap/disk, and unchanged maintained runtime budgets before adoption. | `scripts/test_bench_rust_feedback.py`; `specs/015-tune-rust-feedback/decisions.yaml` | PENDING |
| FR-016 | Decision-ledger validation accounts for every evaluated adaptive/fixed-job, narrowing, nextest, and sccache candidate and requires evidence, rejection reason, and reconsideration condition. | `specs/015-tune-rust-feedback/decisions.yaml`; rejected experiments section below | PENDING |
| FR-017 | Compatibility fixtures compare explicit full-feature and release command arguments, selected features, targets, exits, and receipt behavior with the pre-change contract. | `scripts/test_dev_cargo_scope.sh`; full-feature guardrail section below | PENDING |
| FR-018 | Missing-feature regression test proves focused success cannot satisfy release delivery, followed by repository all-target/all-feature guardrails. | Focused test receipt; `scripts/check_guardrails.sh` receipt in full-feature section | PENDING |
| FR-019 | Run focused Python, shell, and exact Rust tests for benchmark math/contracts, scope/feature resolution, zero-test preflight, compatibility, fallback, and receipt correctness before broad validation. | Exact command/result ledger in the focused-check subsection below | PENDING |
| FR-020 | Run focused checks, `scripts/check_guardrails.sh`, coordinated self-development build/reload, resolve the active executable, and smoke-test the newly built binary through a unique socket. | Delivery sections below; build/reload and isolated-socket receipts | PENDING |
| FR-021 | Complete the principle matrix below with affected requirements, exact checks, evidence, outcomes, and explicit non-applicability where required. | Principle traceability section below | PENDING |
| NFR-001 | Compare representative baseline/candidate p50 and p95 for every adopted scenario and run every applicable maintained client/daemon runtime-budget check unchanged. | Benchmark comparison receipts; runtime budgets section below | PENDING |
| NFR-002 | Validate no increase in failures/retries and zero false successes in optional-feature and full-feature regression fixtures. | Comparison report; focused and full-feature test receipts | PENDING |
| NFR-003 | Receipt/schema validation proves every accepted comparison retains raw samples, percentile method, environment identity, schema version, command intent, and terminal outcome. | `scripts/test_bench_rust_feedback.py`; `specs/015-tune-rust-feedback/evidence/*.json` | PENDING |
| NFR-004 | Zero-test and scope-resolution tests verify actionable early rejection and visible effective package, target, features, and resolution source in every focused receipt. | Selfdev exact-test receipt; `scripts/test_dev_cargo_scope.sh`; focused receipts | PENDING |
| NFR-005 | Validate every experiment boundary, fallback, resource receipt, and adopt-or-reject decision, plus zero uncontrolled duplicate actions and zero shared-daemon disruption. | Experiment receipts; `specs/015-tune-rust-feedback/decisions.yaml`; isolated-socket section | PENDING |

## Constitution principle traceability

| Principle | Affected requirements | Intended direct check | Evidence path | Terminal result |
|---|---|---|---|---|
| PRIN-002: Backward-Compatible User and Data Contracts | FR-006, FR-007, FR-009, FR-012, FR-017 | Compatibility fixtures cover unset behavior, explicit broad/full-feature commands, Cargo filter semantics, conservative fallback, coordinator ownership, and serde defaults for any additive persisted fields. | `scripts/test_dev_cargo_scope.sh`; `scripts/test_rust_validation_scope.py`; `crates/jcode-app-core/src/tool/selfdev/tests.rs`; full-feature guardrail receipt | PENDING |
| PRIN-003: Explicit Configuration Precedence | FR-008, FR-010 | Precedence matrix proves explicit request arguments win over inference/defaults and receipts show configured/effective non-secret values plus resolution source. | `scripts/test_rust_validation_scope.py`; `scripts/test_dev_cargo_scope.sh`; focused receipts | PENDING |
| PRIN-005: Focused Behavioral Tests | FR-001, FR-009, FR-011, FR-012, FR-018, FR-019 | Run the smallest deterministic Python, shell, and exact Rust checks first and record each exact command and outcome below. | Focused-check command ledger; test sources listed in requirement matrix | PENDING |
| PRIN-007: Repository Guardrails Are Delivery Gates | FR-018, FR-020 | Run the stable-toolchain repository guardrail contract, including formatting, all-target/all-feature checks, Clippy, lockfile, ratchets, dependency boundaries, and maintained budgets. | Full-feature guardrails section below | PENDING |
| PRIN-008: Custom-First, Upstream-Aware Simplicity | FR-006, FR-007, FR-013, FR-014, FR-015, FR-016 | Verify one canonical `dev_cargo` policy, server-owned selfdev coordinator, global gate, and bounded reversible experiments with explicit fallbacks and decisions. | Architecture-focused tests; experiment receipts; `decisions.yaml` | PENDING |
| PRIN-010: Measured Efficiency and Runtime Truth | FR-001, FR-002, FR-003, FR-013, FR-014, FR-015, FR-020; NFR-001 | Retain reproducible benchmark/resource receipts, enforce unchanged runtime budgets, confirm resolved executable identity, and run the newly built binary via an isolated socket or deliberate reload. | Benchmark receipts; runtime budgets; resolved binary and isolated-socket sections below | PENDING |

## Focused automated checks

Record focused checks before broad validation.

| Check | Exact command | Result | Evidence or notes |
|---|---|---|---|
| Benchmark contract and aggregation tests | PENDING | PENDING | PENDING |
| Scope and feature resolver tests | PENDING | PENDING | PENDING |
| `dev_cargo` job policy shell tests | PENDING | PENDING | PENDING |
| `dev_cargo` scope integration shell tests | PENDING | PENDING | PENDING |
| Zero-test and coordinator exact Rust tests | PENDING | PENDING | PENDING |
| Relevant Rust build | PENDING | PENDING | PENDING |

## Benchmark receipts

Reserved for reproducible baseline/candidate and experiment evidence.

### Frozen baseline collection

- Receipt: `specs/015-tune-rust-feedback/evidence/baseline.json`
- Collection preflight: `python3 scripts/bench_rust_feedback.py validate-matrix scripts/rust_feedback_matrix.json` -> `PASS (exit 0)`
- Artifact validation: `python3 scripts/bench_rust_feedback.py validate-receipt specs/015-tune-rust-feedback/evidence/baseline.json` -> `PASS (exit 0)`
- Collection result: `INCOMPLETE`. The frozen matrix defines all six scenario intents and bounds, but it does not yet declare executable per-scenario commands. The invalid-zero-test behavior is also owned by dependent tasks T019-T021. The receipt therefore records every missing scenario and the next collection condition rather than fabricating measurements or bypassing the server-owned coordinator.

| Scenario or lane | Baseline receipt | Candidate receipt | Valid samples | Invalid/retried samples | p50 result | nearest-rank p95 result | Resource/action-count result | Terminal result |
|---|---|---|---|---|---|---|---|---|
| Warm touched-file check | `evidence/baseline.json` (incomplete) | PENDING | 0 | 0 | N/A: no controlled sample | N/A: no controlled sample | N/A: scenario command not yet executable | `INCOMPLETE` |
| Warm touched-file build | `evidence/baseline.json` (incomplete) | PENDING | 0 | 0 | N/A: no controlled sample | N/A: no controlled sample | N/A: scenario command not yet executable | `INCOMPLETE` |
| Cold or fresh-worktree build | `evidence/baseline.json` (incomplete) | PENDING | 0 | 0 | N/A: no controlled sample | N/A: no controlled sample | N/A: scenario command not yet executable | `INCOMPLETE` |
| Focused test | `evidence/baseline.json` (incomplete) | PENDING | 0 | 0 | N/A: no controlled sample | N/A: no controlled sample | N/A: scenario command not yet executable | `INCOMPLETE` |
| Invalid zero-test request | `evidence/baseline.json` (incomplete) | PENDING | 0 | 0 | N/A: dependent behavior pending | N/A: dependent behavior pending | Zero underlying scenario actions recorded | `INCOMPLETE` |
| Broad validation | `evidence/baseline.json` (incomplete) | PENDING | 0 | 0 | N/A: no controlled sample | N/A: no controlled sample | N/A: scenario command not yet executable | `INCOMPLETE` |
| Coordinated duplicate requests | `evidence/baseline.json` (incomplete) | PENDING | 0 | 0 | N/A: no controlled sample | N/A: no controlled sample | No uncontrolled duplicate action was launched | `INCOMPLETE` |
| Adaptive versus 6 versus 8 jobs | PENDING | PENDING | PENDING | PENDING | PENDING | PENDING | PENDING | PENDING |
| Bounded nextest | PENDING | PENDING | PENDING | PENDING | PENDING | PENDING | PENDING | PENDING |
| Bounded sccache cold miss/reusable hit | PENDING | PENDING | PENDING | PENDING | PENDING | PENDING | PENDING | PENDING |

## Rejected experiments

Reserved for durable rejection evidence. Every non-adopted evaluated candidate must also appear in `specs/015-tune-rust-feedback/decisions.yaml`.

| Candidate | Tested configuration and boundary | Evidence | Rejection reason | Safe fallback | Reconsideration condition | Terminal result |
|---|---|---|---|---|---|---|
| PENDING | PENDING | PENDING | PENDING | PENDING | PENDING | PENDING |

## Full-feature guardrails

Reserved for the unchanged release-quality gate.

| Validation | Exact command | Toolchain | Result | Evidence or isolated baseline note |
|---|---|---|---|---|
| Repository guardrails with all-target/all-feature validation | `scripts/check_guardrails.sh` | PENDING | PENDING | PENDING |
| Explicit full-feature check/test/build/release compatibility | PENDING | PENDING | PENDING | PENDING |
| Focused-success/full-feature-failure regression fixture | PENDING | PENDING | PENDING | PENDING |

## Runtime budgets

Reserved for maintained client and daemon budget evidence. Thresholds remain unchanged.

| Budget or workflow | Baseline | Candidate | Maintained threshold/tolerance | Result | Evidence |
|---|---|---|---|---|---|
| Applicable client runtime budgets | PENDING | PENDING | Existing maintained budget | PENDING | PENDING |
| Applicable daemon runtime budgets | PENDING | PENDING | Existing maintained budget | PENDING | PENDING |
| Self-development feedback latency | PENDING | PENDING | Adopt only when intended p50 and p95 improve | PENDING | PENDING |
| RSS, swap, and disk impact | PENDING | PENDING | No unacceptable regression | PENDING | PENDING |

## Coordinated build and reload

Reserved for the server-owned self-development lifecycle. Do not force-stop the shared server.

| Step | Exact command or observation | Result | Evidence |
|---|---|---|---|
| Build candidate through coordinated self-development path | PENDING | PENDING | PENDING |
| Activate or reload intended shared-server build gracefully | PENDING | PENDING | PENDING |
| Confirm client reconnect and continued session behavior | PENDING | PENDING | PENDING |

## Resolved binary identity

Reserved to prove which executable was measured. Resolve symlinks before inspecting identity.

| Item | Exact command | Observed value | Result |
|---|---|---|---|
| Candidate executable path | PENDING | PENDING | PENDING |
| Resolved executable path | `readlink -f <candidate-or-shared-server-path>` | PENDING | PENDING |
| Build revision/version identity | PENDING | PENDING | PENDING |
| Shared daemon identity, if reloaded | PENDING | PENDING | PENDING |

## Built-binary isolated-socket validation

Reserved for runtime truth without disturbing the shared daemon.

Canonical command shape:

```bash
./target/selfdev/jcode run --no-update --socket /run/user/1000/jcode-rust-feedback-<unique>.sock '<prompt>'
```

| Step | Exact command or observation | Result | Evidence |
|---|---|---|---|
| Build `target/selfdev/jcode` | PENDING | PENDING | PENDING |
| Resolve and confirm candidate binary identity | PENDING | PENDING | PENDING |
| Run isolated-socket smoke against candidate | PENDING | PENDING | PENDING |
| Confirm shared-daemon socket/process was not disturbed | PENDING | PENDING | PENDING |

## Final acceptance summary

| Gate | Result | Notes |
|---|---|---|
| FR-001 through FR-021 complete | PENDING | PENDING |
| NFR-001 through NFR-005 complete | PENDING | PENDING |
| Named constitution principles traced | PENDING | PENDING |
| Benchmark evidence complete and comparable | PENDING | PENDING |
| Every experiment adopted or rejected | PENDING | PENDING |
| Full-feature guardrails passed | PENDING | PENDING |
| Runtime budgets unchanged | PENDING | PENDING |
| Coordinated build/reload verified | PENDING | PENDING |
| Resolved binary identity confirmed | PENDING | PENDING |
| Isolated-socket built-binary smoke passed | PENDING | PENDING |

Final delivery decision: **PENDING**
