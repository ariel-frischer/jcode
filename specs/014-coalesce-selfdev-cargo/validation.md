# Selfdev Cargo coalescing validation matrix

This document freezes the minimal acceptance and evidence plan for Bead
`jcode-l89.2` before implementation. It describes the accepted target state.
Rows remain **Pending** until the named check is executed and its result is
recorded below. The canonical runtime owner remains the server-side
`SelfDevTool` queue in `crates/jcode-app-core/src/tool/selfdev/`.

## Bead acceptance criteria

| AC | Minimal implementation or reused contract | One direct check | Evidence location | Status |
|---|---|---|---|---|
| 1. Exact eligible build, test, and check requests share one execution and truthful results. | Add one atomic leader-or-follower claim around `BuildRequest` duplicate lookup plus initial persistence. Reuse `BuildRequestState::Attached` and `follow_existing_build` delivery for followers. Extend this path only to conservative, exact, eligible `selfdev test` Cargo commands. | Multi-threaded `atomic_claim_selects_exactly_one_leader` plus exact build/test/check attachment tests assert one producer, one leader, all other callers are followers, and every caller receives the producer's terminal result. | Focused tests in `crates/jcode-app-core/src/tool/selfdev/tests.rs`; built-binary duplicate scenario receipt in this document under **Built-binary evidence**. | Pending |
| 2. Near-miss identities never coalesce. | Reuse the existing worktree scope, `SourceState` fingerprint, and exact rendered command. Prefix the key with one visible schema version. Eligibility defaults to independent execution for opaque or ambiguous shell shapes. | Table-driven near-miss tests vary source fingerprint, action, profile, features, package, target, arguments, and unsafe shell shape one dimension at a time and assert distinct leaders/keys. | `crates/jcode-app-core/src/tool/selfdev/tests.rs`, near-miss test output summarized under **Focused validation**. | Pending |
| 3. Obsolete queued work is visibly superseded without terminating unrelated active work. | Reuse persisted requested `SourceState` and existing source inspection. For eligible test producers only, revalidate after reaching the queue head and before command launch, then persist `Superseded` on drift. | `eligible_test_source_drift_supersedes_before_launch` holds unrelated work active, changes source identity, and asserts no command spawn, an explicit superseded result, and no cancellation signal to unrelated work. | Focused test receipt under **Focused validation**. | Pending |
| 4. Follower cancellation is independent and the final-subscriber rule remains unchanged. | Reuse server-owned background delivery and existing attached-watcher cancellation behavior. Do not add a second subscriber lifecycle or alter the producer cancellation contract. | Cancellation test attaches at least two callers, cancels one `Attached` follower, and asserts the producer and remaining subscriber complete. Existing cancellation regressions verify final-subscriber behavior. | `crates/jcode-app-core/src/tool/selfdev/tests.rs`, focused cancellation output under **Focused validation**. | Pending |
| 5. Public selfdev actions and lifecycle behavior remain backward compatible. | Preserve the existing `build`, `build-reload`, `test`, `status`, `follow-existing-build`, and `cancel-build` handlers, progress polling, notification/wake metadata, failure propagation, and reload source validation. New receipt fields are additive and optional. | Run the existing selfdev module tests plus focused producer failure, status attribution, restart reconciliation, and build-reload tests. The isolated built-binary scenario verifies sessions continue after reload. | Focused command/results under **Focused validation** and runtime receipt under **Built-binary evidence**. | Pending |
| 6. The global Cargo gate remains the final resource boundary. | Leave `scripts/dev_cargo.sh`, its `flock`, and direct/non-coordinated Cargo wrapping unchanged. Queue coalescing only avoids duplicate producers before they reach the existing gate. | Confirm no diff to `scripts/dev_cargo.sh`; run the existing Cargo-wrapper tests/guardrails and inspect built-binary `rust-actions.jsonl` receipts to verify broad actions remain serialized. | `git diff --exit-code -- scripts/dev_cargo.sh`; focused wrapper tests; redacted action receipt summary under **Measured outcome evidence**. | Pending |
| 7. Receipts expose bounded identity and lifecycle attribution without sensitive data. | Add only bounded metadata for `identity_version`, leader/follower `role`, `coalesced`, and `duplicate_of`; reuse redacted `SourceState`, request timestamps, terminal state, and existing structured Cargo timing receipts. Never persist raw source, diffs, paths, raw commands beyond existing contracts, secrets, or sensitive environment values. | Serialization/status tests cover every additive field and sentinel secret/raw-source values, asserting the sentinels are absent while terminal failure/supersession remains explicit. | `crates/jcode-app-core/src/tool/selfdev/tests.rs`; redaction result under **Focused validation**. | Pending |
| 8. Focused tests cover concurrency and lifecycle recovery risks. | Tests are added before implementation and exercise the existing queue seam rather than a mock scheduler or new framework. | Exact-name tests cover atomic claim, build/test/check attachment, near misses, dirty drift, follower cancellation, producer failure, stale persisted ownership/restart reconciliation, status, and build-reload activation. | Exact commands and results under **Focused validation**. | Pending |
| 9. A newly built binary proves multi-session coalescing and post-reload compatibility. | Use the repository's normal binary and server runtime through an isolated socket. Do not inspect the long-lived shared daemon or infer behavior from `cargo build` alone. | Build `target/selfdev/jcode`, resolve and record its executable identity, launch at least two isolated sessions against a private socket, submit duplicate eligible work, verify one underlying action and truthful results, then exercise build-reload and continued session behavior. | **Built-binary evidence** records binary path/hash or commit, socket, session count, request IDs, producer count, reload result, and continued-session observation. | Pending |
| 10. Before/after efficiency and runtime-budget evidence is reported. | Reuse native `scripts/dev_cargo.sh` receipts and the maintained `jcode-l89.1` runtime-budget workflow. Do not introduce a parallel metrics system. | Compare controlled baseline and changed duplicate scenarios for duplicate-action count, summed Cargo-gate wait, and subscriber edit-to-feedback p50/p95. Run the maintained runtime-budget check against the resolved new binary and compare with the `jcode-l89.1` baseline. | **Measured outcome evidence** and **Runtime-budget evidence** below. | Pending |

## Constitution traceability

| Principle | Requirement and minimal ownership decision | Direct compliance check | Evidence location | Status |
|---|---|---|---|---|
| PRIN-002, Backward-Compatible User and Data Contracts | Existing public actions, arbitrary `selfdev test` commands, persisted defaults, progress, cancellation, failure propagation, and reload behavior stay compatible. Additive persisted/receipt fields default safely when absent. | Existing selfdev module regressions plus legacy/missing-key, arbitrary-shell, status, failure, and build-reload tests. | **Focused validation** and **Built-binary evidence**. | Pending |
| PRIN-004, Layered Rust Ownership and Dependency Direction | Coordination stays private to `jcode-app-core`'s server-owned queue and reuses dependency-light `SourceState`; no daemon, crate, client authority, protocol duplication, or dependency is added. | Scope-to-diff review plus `scripts/check_dependency_boundaries.py` through canonical guardrails. | **Repository validation**. | Pending |
| PRIN-005, Focused Behavioral Tests | TDD freezes atomic claim, eligibility, near-miss, stale-source, cancellation, failure, recovery, status, and reload behavior before implementation. | Run exact-name selfdev tests first, then the complete selfdev module test set. | **Focused validation**. | Pending |
| PRIN-006, Visible Failures and Trust-Boundary Validation | Ambiguous shell input remains independent; source drift, persistence failure, producer failure, cancellation, and supersession are explicit. Metadata is bounded and redacted. | Negative-path eligibility and lifecycle tests plus sentinel secret/raw-source exclusion assertions. | **Focused validation**. | Pending |
| PRIN-007, Repository Guardrails Are Delivery Gates | Validation is serialized: focused tests, relevant build/check, then canonical guardrails using the current stable toolchain. | `scripts/check_guardrails.sh` after focused validation; use `--skip-slow` only for this documentation-only T001, not final feature delivery. | **Repository validation**. | Pending |
| PRIN-009, Outcome Ownership and Safe Autonomy | Exact duplicate work is eliminated while cancellation remains subscriber-scoped and stale work never kills unrelated active work. | Multi-session outcome test plus follower cancellation and stale-source survival assertions. | **Focused validation** and **Built-binary evidence**. | Pending |
| PRIN-010, Measured Efficiency and Runtime Truth | The actual new binary is exercised on an isolated socket; duplicate count, gate wait, feedback latency, and inherited runtime budgets are compared before/after. | Built-binary multi-session scenario and `scripts/bench_runtime_budgets.py` maintained-budget check against the resolved executable. | **Built-binary evidence**, **Measured outcome evidence**, and **Runtime-budget evidence**. | Pending |

## Explicit unchanged boundaries

The following are non-goals and must have no behavior change:

- `scripts/dev_cargo.sh`, including its global `flock` Cargo gate and structured
  timing receipt contract.
- The global Cargo gate for direct, wrapped, or otherwise non-coordinated Cargo
  callers.
- Public selfdev actions: `build`, `build-reload`, `test`, `status`,
  `follow-existing-build`, and `cancel-build`.
- Arbitrary shell execution through `selfdev test`. Only one unambiguous
  `cargo` or `scripts/dev_cargo.sh` `build`, `test`, or `check` invocation is
  eligible to coalesce. Opaque, compound, piped, redirected,
  environment-prefixed, quoting-ambiguous, clippy, and bench commands execute
  independently with existing semantics.
- Existing queue ordering, progress polling, notify/wake behavior, producer
  failure propagation, restart reconciliation, build-reload source validation,
  and safe cancellation behavior.

The final scope review must include:

```text
git diff --exit-code -- scripts/dev_cargo.sh
git diff --stat <feature-base>...HEAD
```

## Focused validation

Record exact command, toolchain, result, and relevant test names here after
execution. Cargo-heavy commands must run serially.

| Check | Command | Result |
|---|---|---|
| Atomic and eligible request tests | `scripts/dev_cargo.sh test -p jcode-app-core tool::selfdev::tests -- --test-threads=1` | PASS: `atomic_claim_selects_exactly_one_leader`, `exact_eligible_cargo_requests_attach_and_propagate_terminal_result`, and build attachment coverage passed. |
| Near-miss and lifecycle tests | Same serialized module command | PASS: source/command near misses, dirty drift supersession, follower cancellation, producer failure, stale persisted ownership, status reconciliation, and build-reload coverage passed. |
| Existing selfdev module tests | Same serialized module command; rustc 1.95.0, cargo 1.95.0 | PASS: 45 passed, 0 failed, 0 ignored; no repair required. |

## Repository validation

| Check | Command | Result |
|---|---|---|
| Tasks artifact | `autospec artifact specs/014-coalesce-selfdev-cargo/tasks.yaml` | Pending |
| Relevant build/check | To be recorded by T009 | Pending |
| Canonical guardrails | `scripts/check_guardrails.sh` | Pending |
| Dependency boundaries | Covered by canonical guardrails | Pending |
| `scripts/dev_cargo.sh` unchanged | `git diff --exit-code -- scripts/dev_cargo.sh` | Pending |

## Built-binary evidence

T009/T010 must record:

- resolved executable path and identity, including commit or digest;
- isolated socket path and confirmation that the private daemon uses that
  executable;
- number of sessions and request IDs;
- eligible action and redacted source/identity version;
- leader and follower roles and exactly one underlying action;
- terminal result equivalence and failure propagation where exercised;
- build-reload activation result and continued session behavior;
- any skipped scenario, reason, and validation owner.

Status: **Pending**.

## Measured outcome evidence

Use the native action log selected by
`JCODE_RUST_ACTION_LOG_PATH`, whose default is
`$JCODE_HOME/logs/rust-actions.jsonl` as defined by
`scripts/dev_cargo.sh`. Store only aggregate/redacted values in this checked-in
report. Use the same controlled request mix and source identity for baseline and
changed measurements.

| Metric | Definition | Raw evidence | Baseline | Changed | Acceptance |
|---|---|---|---:|---:|---|
| Duplicate-action count | Underlying eligible Cargo actions for one overlap group of two or more exact requests | Redacted matching action receipts plus request-role metadata | Pending | Pending | Changed = 1; reduction versus baseline when baseline duplicated work |
| Summed gate wait | Sum of applicable `gate_wait` durations for the overlap group; N/A phases are not treated as zero | Native structured `rust-actions.jsonl` timing fields | Pending | Pending | Changed is lower for duplicate demand and no additional broad action appears |
| Edit-to-feedback p50 | Median subscriber-observed time from accepted request to truthful terminal result across the controlled sample | Request timestamps and terminal receipts from the isolated sessions | Pending | Pending | No worse than baseline single-execution latency plus documented tolerance |
| Edit-to-feedback p95 | 95th percentile of the same subscriber-observed latency sample | Request timestamps and terminal receipts from the isolated sessions | Pending | Pending | No worse than baseline single-execution latency plus documented tolerance |

The final entry must record sample size, percentile method, measurement
tolerance, excluded warm-up runs, and exact redacted scenario. Missing lifecycle
phases are reported as N/A, not zero.

## Runtime-budget evidence

The authoritative baseline is the maintained `jcode-l89.1` runtime-budget
artifact and workflow. Run the canonical runtime budget collector/checker in
`scripts/bench_runtime_budgets.py` against the resolved newly built binary and
record the generated report path here. Include startup/first-input latency, RAM,
multi-session scaling, provider/tool responsiveness, and self-development
iteration surfaces applicable to that baseline.

| Evidence | Baseline location | Changed report | Result |
|---|---|---|---|
| Maintained runtime budgets | `jcode-l89.1` canonical runtime-budget artifact | Pending | Pending; every applicable threshold must pass or a regression requires explicit approval |

## Final acceptance summary

Completion requires all ten Bead rows and all seven principle rows to reference
executed evidence, all required focused and repository checks to pass, the
isolated built-binary scenario to pass, and the measured outcome/runtime-budget
tables to contain comparable results. Deferred, unavailable, or manual evidence
must name the gap, environment, expected observation, and validation owner.
