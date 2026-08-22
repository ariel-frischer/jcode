# Rust Feedback Tuning Validation

Status: **T033 traceability closed; coordinated reload, runtime budgets, and built-binary checks passed, while the incomplete benchmark matrix and pre-existing full-feature guardrail failures remain explicit delivery blockers**
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
| FR-001 | `python3 -m unittest scripts.test_bench_rust_feedback.BenchmarkMatrixContractTests` verifies all six scenario classes, preparation, policy, validity, and evidence fields. | `scripts/rust_feedback_matrix.json`; `scripts/test_bench_rust_feedback.py`; benchmark receipt index below | PASS (exit 0): included in 25-test benchmark suite |
| FR-002 | Benchmark receipt contract tests validate baseline/candidate identity, revision, dirty fingerprint, lockfile, toolchain, host/resources, configuration, timestamp, matrix version, compatibility disclosure, and redaction. | `scripts/test_bench_rust_feedback.py`; `specs/015-tune-rust-feedback/evidence/*.json` | PASS (exit 0): included in 25-test benchmark suite |
| FR-003 | Receipt contract and runner fixture tests reject missing or inconsistent wall, execution, queue, gate, RSS, swap, cache, exit, retry, and action-count fields while accepting explicit not-applicable values. | `scripts/test_bench_rust_feedback.py`; `specs/015-tune-rust-feedback/evidence/*.json` | PASS (exit 0): included in 25-test benchmark suite |
| FR-004 | Deterministic aggregation fixtures verify retained valid/invalid/retried samples, p50, nearest-rank p95, invalid exclusion, retry accounting, and report reproduction. | `scripts/test_bench_rust_feedback.py`; comparison receipts indexed below | PASS (exit 0): included in 25-test benchmark suite |
| FR-005 | Execute equivalent isolated one-action benchmark lanes for adaptive, explicit 6-job, and explicit 8-job policies, then validate complete comparable sample sets. | `specs/015-tune-rust-feedback/evidence/*jobs*.json`; benchmark receipts section below | N/A: controlled timing collection is an evidence boundary; `job-counts.json` records the incomplete trial and rejection rather than fabricated samples |
| FR-006 | Run the coordinated duplicate scenario and assert coordinator-authorized execution, global-gate ownership, and consistent requested/executed/follower/reused/coalesced counts. | `crates/jcode-app-core/src/tool/selfdev/tests.rs`; `specs/015-tune-rust-feedback/evidence/*duplicate*.json` | PASS (exit 0): 50 exact selfdev tests include coordinator coalescing and ownership regressions |
| FR-007 | `python3 -m unittest scripts.test_rust_validation_scope` verifies narrow eligible package/target selection and conservative fallback for ambiguous, generated, build-script, cross-package, workspace, broad, and release-sensitive paths. | `scripts/test_rust_validation_scope.py`; focused validation receipts | PASS (exit 0): 23 tests |
| FR-008 | Scope resolver and shell integration tests cover explicit feature, no-default-feature, all-feature, inferred, empty, unset, conflicting, and unsupported combinations with explicit input winning. | `scripts/test_rust_validation_scope.py`; `scripts/test_dev_cargo_scope.sh` | PASS (exit 0): Python resolver and shell integration suites |
| FR-009 | Optional-feature regression fixtures prove insufficient feature sets fail and affected paths either receive required features or broaden to full-feature validation. | `scripts/test_rust_validation_scope.py`; full-feature guardrail section below | FAIL (exit 1): focused optional-feature fixtures passed, but the unchanged repository full-feature gate exposed pre-existing all-feature E2E compile failures at task start; focused success was not accepted as delivery evidence |
| FR-010 | Shell integration assertions verify receipts report configured and effective package, target, features, and `explicit`, `inferred`, `default`, or `conservative_fallback` resolution source. | `scripts/test_dev_cargo_scope.sh`; focused action receipts | PASS (exit 0): focused-scope shell integration suite |
| FR-011 | Exact Rust selfdev test proves a current complete discovery snapshot rejects a provably empty package/filter before producer claim, queue wait, Cargo-gate acquisition, or execution. | `crates/jcode-app-core/src/tool/selfdev/tests.rs`; zero-test receipt | PASS (exit 0): `proven_empty_test_is_rejected_before_claim_queue_and_gate` |
| FR-012 | Exact Rust and Python tests cover positive, ambiguous, stale, ignored, integration, library, exact-name, and substring cases and preserve execution when emptiness is not provable. | `crates/jcode-app-core/src/tool/selfdev/tests.rs`; `scripts/test_rust_validation_scope.py` | PASS (exit 0): 50 Rust selfdev tests and 23 Python scope tests |
| FR-013 | Run the bounded nextest-compatible focused-test lane, validate availability, compatibility, timing, resources, disk, reporting, fallback, and final decision. | `specs/015-tune-rust-feedback/evidence/*nextest*.json`; `specs/015-tune-rust-feedback/decisions.yaml` | N/A: bounded trial recorded unavailable/incomplete evidence and retained Cargo test fallback |
| FR-014 | Run bounded non-incremental fresh/cold sccache miss and reusable-hit lanes, including cache failure fallback and proof warm incremental defaults remain unchanged. | `specs/015-tune-rust-feedback/evidence/*sccache*.json`; `specs/015-tune-rust-feedback/decisions.yaml` | N/A: bounded trial recorded unavailable/incomplete evidence and retained warm incremental default |
| FR-015 | Decision validator requires complete baseline/candidate evidence, lower p50 and p95, unchanged failure/retry rate, acceptable RSS/swap/disk, and unchanged maintained runtime budgets before adoption. | `scripts/test_bench_rust_feedback.py`; `specs/015-tune-rust-feedback/decisions.yaml` | N/A: no candidate was adopted; `decisions.yaml` records rejection because comparable measurements and runtime-budget evidence are incomplete |
| FR-016 | Decision-ledger validation accounts for every evaluated adaptive/fixed-job, narrowing, nextest, and sccache candidate and requires evidence, rejection reason, and reconsideration condition. | `specs/015-tune-rust-feedback/decisions.yaml`; rejected experiments section below | PASS (exit 0): all seven inventoried candidates have durable rejected decisions, evidence, fallback, and reconsideration conditions |
| FR-017 | Compatibility fixtures compare explicit full-feature and release command arguments, selected features, targets, exits, and receipt behavior with the pre-change contract. | `scripts/test_dev_cargo_scope.sh`; full-feature guardrail section below | PASS (exit 0): focused-scope shell compatibility suite |
| FR-018 | Missing-feature regression test proves focused success cannot satisfy release delivery, followed by repository all-target/all-feature guardrails. | Focused test receipt; `scripts/check_guardrails.sh` receipt in full-feature section | FAIL (exit 1): focused regression PASS (exit 0), but repository guardrails failed and remain a blocking full-feature gate |
| FR-019 | Run focused Python, shell, and exact Rust tests for benchmark math/contracts, scope/feature resolution, zero-test preflight, compatibility, fallback, and receipt correctness before broad validation. | Exact command/result ledger in the focused-check subsection below | PASS (exit 0): all six focused ledger entries passed serial Cargo validation |
| FR-020 | Run focused checks, `scripts/check_guardrails.sh`, coordinated self-development build/reload, resolve the active executable, and smoke-test the newly built binary through a unique socket. | Delivery sections below; build/reload and isolated-socket receipts | FAIL (exit 1): focused checks, coordinated publish/reload, resolved live identity, runtime budgets, and isolated smoke passed; the unchanged repository guardrail gate failed on seven pre-existing checks |
| FR-021 | Complete the principle matrix below with affected requirements, exact checks, evidence, outcomes, and explicit non-applicability where required. | Principle traceability section below | PASS: all six named principles now cite requirements, exact commands or retained artifacts, terminal outcomes, and explicit failed or non-applicable boundaries |
| NFR-001 | Compare representative baseline/candidate p50 and p95 for every adopted scenario and run every applicable maintained client/daemon runtime-budget check unchanged. | Benchmark comparison receipts; runtime budgets section below | PASS (exit 0): no experiment was adopted, and the unchanged canonical nine-metric runtime collection and baseline comparison both passed |
| NFR-002 | Validate no increase in failures/retries and zero false successes in optional-feature and full-feature regression fixtures. | `evidence/job-counts.json`; `decisions.yaml`; focused and full-feature receipts | PASS: measured job lanes retained zero nonzero exits with matched retry counts, optional-feature fixtures passed, and the failing broad gate remained visible rather than becoming a false success |
| NFR-003 | Receipt/schema validation proves every accepted comparison retains raw samples, percentile method, environment identity, schema version, command intent, and terminal outcome. | `scripts/test_bench_rust_feedback.py`; `specs/015-tune-rust-feedback/evidence/*.json` | PASS (exit 0): receipt and aggregation contracts passed; no incomplete comparison was accepted or presented as complete evidence |
| NFR-004 | Zero-test and scope-resolution tests verify actionable early rejection and visible effective package, target, features, and resolution source in every focused receipt. | Selfdev exact-test receipt; `scripts/test_dev_cargo_scope.sh`; focused receipts | PASS (exit 0): 50 Rust selfdev tests, 23 resolver tests, and the shell receipt matrix passed |
| NFR-005 | Validate every experiment boundary, fallback, resource receipt, and adopt-or-reject decision, plus zero uncontrolled duplicate actions and zero shared-daemon disruption. | Experiment receipts; `specs/015-tune-rust-feedback/decisions.yaml`; coordinated reload and isolated-socket sections | PASS: seven candidates were rejected with safe fallbacks, duplicate counts remained coordinator-owned, isolated smoke left shared sockets untouched, and graceful reload reported `handoff_ready: true` with this session continuing |

## Success criteria traceability

| Success criterion | Exact evidence and outcome | Terminal result |
|---|---|---|
| SC-001 | `evidence/baseline.json` and the benchmark table retain all six required scenario classes but disclose zero complete baseline/candidate pairs. | FAIL: the complete representative comparison target was not met and no completeness claim was made |
| SC-002 | `decisions.yaml` authorizes zero default changes because no candidate proved lower p50 and p95 across its complete intended matrix without regression. | N/A: no tuning choice became a default; established safe defaults remain |
| SC-003 | `proven_empty_test_is_rejected_before_claim_queue_and_gate`, the 50-test selfdev suite, 23 resolver tests, and scope shell matrix passed. | PASS (exit 0) |
| SC-004 | Resolver, optional-feature, explicit-precedence, conservative-fallback, and focused-success/full-feature-failure fixtures passed. | PASS (exit 0): correct narrow-or-fallback behavior and zero accepted optional-feature false successes |
| SC-005 | Canonical runtime collection and comparison passed all nine maintained metrics with unchanged thresholds. | PASS (exit 0) |
| SC-006 | `decisions.yaml` accounts for adaptive, fixed 6, fixed 8, inferred scope, nextest, sccache miss, and sccache hit with evidence and rationale. | PASS: 7 of 7 candidates accounted for |

## Constitution principle traceability

| Principle | Affected requirements | Intended direct check | Evidence path | Terminal result |
|---|---|---|---|---|
| PRIN-002: Backward-Compatible User and Data Contracts | FR-006, FR-007, FR-009, FR-012, FR-017 | Compatibility fixtures cover unset behavior, explicit broad/full-feature commands, Cargo filter semantics, conservative fallback, coordinator ownership, and serde defaults for any additive persisted fields. | `python3 -m unittest scripts.test_rust_validation_scope`; `bash scripts/test_dev_cargo_scope.sh`; `bash scripts/dev_cargo.sh test -p jcode-app-core tool::selfdev::tests -- --nocapture`; full-feature section | PASS (exits 0): 23 resolver tests, shell compatibility fixtures, and 50 selfdev tests passed; the separate broad guardrail failure remains visible and did not change these contracts |
| PRIN-003: Explicit Configuration Precedence | FR-008, FR-010 | Precedence matrix proves explicit request arguments win over inference/defaults and receipts show configured/effective non-secret values plus resolution source. | `python3 -m unittest scripts.test_rust_validation_scope`; `bash scripts/test_dev_cargo_scope.sh`; focused receipts | PASS (exits 0): explicit feature, all-feature, no-default-feature, package, and target inputs retained precedence over inference and fallback |
| PRIN-005: Focused Behavioral Tests | FR-001, FR-009, FR-011, FR-012, FR-018, FR-019 | Run the smallest deterministic Python, shell, and exact Rust checks first and record each exact command and outcome below. | Focused-check command ledger; test sources listed in requirement matrix | PASS (exits 0): 25 benchmark, 23 resolver, shell job/scope, 50 exact Rust selfdev, and focused Rust check validations passed before the broad gate |
| PRIN-007: Repository Guardrails Are Delivery Gates | FR-018, FR-020 | Run the stable-toolchain repository guardrail contract, including formatting, all-target/all-feature checks, Clippy, lockfile, ratchets, dependency boundaries, and maintained budgets. | Full-feature guardrails section below | FAIL (exit 1): seven gates failed; no focused result or ratchet rebaseline was substituted for the unchanged delivery gate |
| PRIN-008: Custom-First, Upstream-Aware Simplicity | FR-006, FR-007, FR-013, FR-014, FR-015, FR-016 | Verify one canonical `dev_cargo` policy, server-owned selfdev coordinator, global gate, and bounded reversible experiments with explicit fallbacks and decisions. | `bash scripts/test_dev_cargo_jobs.sh`; selfdev exact tests; experiment receipts; `decisions.yaml` | PASS (exits 0 for focused checks): existing coordinator and gate remained authoritative, all bounded experiments retained established fallbacks, and 7 of 7 decisions are explicit |
| PRIN-010: Measured Efficiency and Runtime Truth | FR-001, FR-002, FR-003, FR-013, FR-014, FR-015, FR-020; NFR-001 | Retain reproducible benchmark/resource receipts, enforce unchanged runtime budgets, confirm resolved executable identity, and run the newly built binary via an isolated socket or deliberate reload. | Benchmark receipts; runtime budgets; resolved binary, isolated-socket, and coordinated reload sections below | FAIL: runtime budgets, resolved identity, isolated smoke, and live reload passed, but the representative baseline/candidate matrix remains incomplete and FR-020 remains blocked by the full-feature guardrail failure |

## Focused automated checks

Record focused checks before broad validation.

| Check | Exact command | Result | Evidence or notes |
|---|---|---|---|
| Benchmark contract and aggregation tests | `python3 -m unittest scripts.test_bench_rust_feedback` | PASS (exit 0) | 25 tests passed in 0.221s |
| Scope and feature resolver tests | `python3 -m unittest scripts.test_rust_validation_scope` | PASS (exit 0) | 23 tests passed in 0.019s |
| `dev_cargo` job policy shell tests | `bash scripts/test_dev_cargo_jobs.sh` | PASS (exit 0) | Adaptive and explicit job sizing contracts passed |
| `dev_cargo` scope integration shell tests | `bash scripts/test_dev_cargo_scope.sh` | PASS (exit 0) | Focused scope, explicit precedence, fallback, receipt, full-feature, and release contracts passed |
| Zero-test and coordinator exact Rust tests | `bash scripts/dev_cargo.sh test -p jcode-app-core tool::selfdev::tests -- --nocapture` | PASS (exit 0) | 50 passed, 0 failed, 1235 filtered out in 33.65s; includes pre-claim zero-test rejection and coordinator coalescing paths |
| Relevant Rust build | `bash scripts/dev_cargo.sh check -p jcode-app-core` | PASS (exit 0) | Finished `dev` profile in 25.42s; run after the Rust test command so Cargo-heavy checks remained serialized |

## Benchmark receipts

Reserved for reproducible baseline/candidate and experiment evidence.

### Frozen baseline collection

- Receipt: `specs/015-tune-rust-feedback/evidence/baseline.json`
- Collection preflight: `python3 scripts/bench_rust_feedback.py validate-matrix scripts/rust_feedback_matrix.json` -> `PASS (exit 0)`
- Artifact validation: `python3 scripts/bench_rust_feedback.py validate-receipt specs/015-tune-rust-feedback/evidence/baseline.json` -> `PASS (exit 0)`
- Collection result: `INCOMPLETE`. The frozen matrix defines all six scenario intents and bounds, but it does not yet declare executable per-scenario commands. The invalid-zero-test behavior is also owned by dependent tasks T019-T021. The receipt therefore records every missing scenario and the next collection condition rather than fabricating measurements or bypassing the server-owned coordinator.

| Scenario or lane | Baseline receipt | Candidate receipt | Valid samples | Invalid/retried samples | p50 result | nearest-rank p95 result | Resource/action-count result | Terminal result |
|---|---|---|---|---|---|---|---|---|
| Warm touched-file check | `evidence/baseline.json` (incomplete) | N/A: no complete candidate receipt | 0 | 0 | N/A: no controlled sample | N/A: no controlled sample | N/A: scenario command not yet executable | `INCOMPLETE` |
| Warm touched-file build | `evidence/baseline.json` (incomplete) | N/A: no complete candidate receipt | 0 | 0 | N/A: no controlled sample | N/A: no controlled sample | N/A: scenario command not yet executable | `INCOMPLETE` |
| Cold or fresh-worktree build | `evidence/baseline.json` (incomplete) | N/A: no complete candidate receipt | 0 | 0 | N/A: no controlled sample | N/A: no controlled sample | N/A: scenario command not yet executable | `INCOMPLETE` |
| Focused test | `evidence/baseline.json` (incomplete) | N/A: no complete candidate receipt | 0 | 0 | N/A: no controlled sample | N/A: no controlled sample | N/A: scenario command not yet executable | `INCOMPLETE` |
| Invalid zero-test request | `evidence/baseline.json` (incomplete) | N/A: no complete candidate receipt | 0 | 0 | N/A: no controlled sample | N/A: no controlled sample | Zero underlying scenario actions recorded | `INCOMPLETE` |
| Broad validation | `evidence/baseline.json` (incomplete) | N/A: no complete candidate receipt | 0 | 0 | N/A: no controlled sample | N/A: no controlled sample | N/A: scenario command not yet executable | `INCOMPLETE` |
| Coordinated duplicate requests | `evidence/baseline.json` (incomplete) | N/A: no complete candidate receipt | 0 | 0 | N/A: no controlled sample | N/A: no controlled sample | No uncontrolled duplicate action was launched | `INCOMPLETE` |
| Adaptive versus 6 versus 8 jobs | `evidence/job-counts.json` | `evidence/job-counts.json` | 20 across three warm-check variants plus coordinated duplicate samples | 16 retained retry samples | 6 jobs improved measured lane; 8 jobs regressed p50 | 6 and 8 lacked complete representative/duplicate evidence | Zero swap; coordinator retained duplicate ownership | `INCOMPLETE`; all candidate default changes rejected |
| Bounded nextest | `evidence/nextest.json` | `evidence/nextest.json` | 0 executable samples | Availability failure retained | N/A | N/A | Tool unavailable; Cargo test fallback retained | `INCOMPLETE`; rejected |
| Bounded sccache cold miss/reusable hit | `evidence/sccache.json` | `evidence/sccache.json` | 0 executable samples | Availability failure retained | N/A | N/A | Tool unavailable; warm incremental sccache remained disabled | `INCOMPLETE`; both candidates rejected |

## Rejected experiments

Reserved for durable rejection evidence. Every non-adopted evaluated candidate must also appear in `specs/015-tune-rust-feedback/decisions.yaml`.

| Candidate | Tested configuration and boundary | Evidence | Rejection reason | Safe fallback | Reconsideration condition | Terminal result |
|---|---|---|---|---|---|---|
| Adaptive jobs | Existing host-derived jobs; warm check plus coordinated duplicate lane | `evidence/baseline.json`; `evidence/job-counts.json` | No complete independent comparator or representative baseline | Retain existing adaptive baseline and explicit overrides | Complete representative baseline/candidate matrix | REJECTED |
| Fixed 6 jobs | Explicit 6-job isolated warm check | `evidence/job-counts.json` | Measured lane improved, but complete scenario and duplicate evidence are missing | Retain adaptive default; explicit 6 remains available | Complete equivalent matrix including duplicates | REJECTED |
| Fixed 8 jobs | Explicit 8-job isolated warm check | `evidence/job-counts.json` | p50 regressed and complete scenario evidence is missing | Retain adaptive default; explicit 8 remains available | Both p50 and p95 improve across complete matrix | REJECTED |
| Inferred focused scope | Deterministic resolver-only samples | `evidence/focused-scope.json` | Correctness fixtures passed, but paired Cargo feedback p50/p95 were not measured | Preserve broad fallback and explicit request behavior | Paired broad-versus-focused Cargo matrix | REJECTED |
| Nextest focused | Bounded compatible focused-test lane | `evidence/nextest.json` | Nextest unavailable, so compatibility and performance evidence are incomplete | Retain `cargo test` | Re-run when available under the same boundary | REJECTED |
| Sccache cold miss | Non-incremental cold/fresh-worktree lane | `evidence/sccache.json` | Sccache unavailable; cold-miss resource and fallback evidence incomplete | Keep sccache disabled for warm incremental work | Re-run when available with cold/fresh isolation | REJECTED |
| Sccache reusable hit | Non-incremental reusable-cache lane | `evidence/sccache.json` | Sccache unavailable; reusable-hit and disk evidence incomplete | Keep sccache disabled for warm incremental work | Re-run after a valid cold-miss setup | REJECTED |

## Full-feature guardrails

Reserved for the unchanged release-quality gate.

| Validation | Exact command | Toolchain | Result | Evidence or isolated baseline note |
|---|---|---|---|---|
| Repository guardrails with all-target/all-feature validation | `scripts/check_guardrails.sh` | `rustc 1.95.0 (59807616e 2026-04-14)` | FAIL (exit 1) | Revision `ac31f6598b3aaa40387edf1ff4ef0ed69e9d6b97`; 77s; seven failed gates isolated below. |
| Explicit full-feature check/test/build/release compatibility | `bash scripts/test_dev_cargo_scope.sh` | Shell fixture with mocked Cargo argv | PASS (exit 0) | T029 focused ledger proves explicit all-feature and release requests retain broad argv and are never narrowed. |
| Focused-success/full-feature-failure regression fixture | `bash scripts/test_dev_cargo_scope.sh` | Shell fixture with mocked Cargo exits | PASS (exit 0) | T029 focused ledger proves a focused success cannot satisfy a failing full-feature release gate. |

Guardrail isolation at task start:

- Started `2026-08-22T17:21:58Z` and finished `2026-08-22T17:23:15Z` at revision `ac31f6598b3aaa40387edf1ff4ef0ed69e9d6b97`, before T030 changed any Rust or shell source.
- Passed: module declaration resolution, `cargo fmt --all --check`, lockfile consistency, crate dependency boundaries, wildcard re-export ratchet, desktop2 frame budget, and onboarding state-space invariants. `cargo machete` was explicitly skipped because it is not installed.
- Failed all-target/all-feature compilation on pre-existing E2E fixtures: duplicate `active_skill` in `tests/e2e/test_support/mod.rs:393` (`E0062`) and missing `reported_cost_usd` in `tests/e2e/provider_behavior.rs:125` (`E0063`).
- Failed Clippy with three existing errors, including `clippy::unnecessary_sort_by` in `crates/jcode-harness-api-server/src/translate.rs:1621` under `-D warnings`.
- Failed unchanged warning, oversized-file, oversized-test, panic-prone-usage, and swallowed-error-usage ratchets because the branch already exceeded their committed baselines. No baseline was regenerated and `--fix` was not used.
- Conclusion: the broad delivery gate remains failed. The failures were reproduced and isolated rather than attributed to T030 documentation changes or replaced with focused success.

## Runtime budgets

The maintained thresholds and tolerances in `docs/RUNTIME_PERFORMANCE_BUDGET.md` were used unchanged. The candidate was rebuilt first, then the canonical collector measured only private runtimes and the canonical comparator evaluated the report against the reviewed Linux reference. Both commands exited 0 and all nine stable metric IDs passed.

Exact workflow:

```bash
python3 -m venv "$JCODE_SCRATCH_DIR/t031-runtime-venv"
"$JCODE_SCRATCH_DIR/t031-runtime-venv/bin/python" -m pip install -r scripts/requirements-runtime-benchmarks.txt
cargo build --profile selfdev
"$JCODE_SCRATCH_DIR/t031-runtime-venv/bin/python" scripts/bench_runtime_budgets.py collect \
  --binary "$(realpath target/selfdev/jcode)" \
  --output "$JCODE_SCRATCH_DIR/t031-runtime-budget.json"
"$JCODE_SCRATCH_DIR/t031-runtime-venv/bin/python" scripts/bench_runtime_budgets.py compare \
  --report "$JCODE_SCRATCH_DIR/t031-runtime-budget.json" \
  --baseline docs/runtime-performance-baselines/linux-reference.json
```

| Maintained metric | Reviewed Linux reference | Candidate | Unchanged threshold/tolerance | Result |
|---|---:|---:|---|---|
| `first_visible_ms` median / p95 | 14.795 / 18.577 ms | 13.138 / 13.829 ms | baseline + `max(15%, 20 ms)` | PASS |
| `input_ready_ms` median / p95 | 84.492 / 98.040 ms | 76.737 / 78.429 ms | baseline + `max(15%, 20 ms)` | PASS |
| `daemon_ready_ms` median | 108.773 ms | 50.978 ms | deterministic ceiling 80 ms | PASS |
| `idle_cpu_percent` median | 0.000% | 0.000% | baseline + 0.5 percentage points | PASS |
| `idle_rss_mib` median | 89.070 MiB | 92.867 MiB | baseline + `max(10%, 8 MiB)` | PASS |
| `session_scaling_mib_per_session` median | 0.001 MiB/session | 0.001 MiB/session | baseline + `max(15%, 2 MiB/session)` | PASS |
| `frame_update_work_count` exact | 0 | 0 | unchanged frame must perform zero relayouts | PASS |
| `protocol_round_trip_ms` median / p95 | 0.088 / 0.348 ms | 0.058 / 0.250 ms | baseline + `max(15%, 1 ms)` | PASS |
| `tool_round_trip_ms` median / p95 | 3.586 / 4.357 ms | 1.804 / 2.038 ms | baseline + `max(15%, 1 ms)` | PASS |

Receipt and identity evidence:

- Collection started `2026-08-22T17:27:54.233026+00:00`; the complete build, collection, and comparison workflow finished in 418.22 seconds.
- Report: `$JCODE_SCRATCH_DIR/t031-runtime-budget.json`, schema `1.0.0`; reviewed baseline: `docs/runtime-performance-baselines/linux-reference.json`.
- Resolved candidate: `target/selfdev/jcode`; SHA-256 `167991ea78d7562bf69b85a48beddb2e3e5da45156a0adea39b255b4c18ba9f0`; reported version/revision `jcode v0.79.625-dev (384c09d5f)`.
- The collector verified the requested/resolved candidate and its private daemon executable. Cleanup recorded `owned_processes_stopped: true`, `private_paths_removed: true`, and no diagnostics.
- No development-feedback experiment was adopted. `decisions.yaml` continues to reject all seven candidates for their recorded incomplete, unavailable, or percentile failures. The passing runtime report removes runtime regression as an open gate but does not authorize any default change.

## Coordinated build and reload

The server-owned lifecycle was exercised in three explicit steps. `JCODE_REPO_DIR` was pinned to this assigned worktree because the parent session inherited a main-checkout repository override. The build/publish phase completed before the command attempted to launch its interactive TUI; the expected non-TTY launch refusal is recorded separately and did not invalidate the already completed build publication.

| Step | Exact command or observation | Result | Evidence |
|---|---|---|---|
| Build and publish candidate through self-development path | `JCODE_REPO_DIR="$PWD" RUSTUP_TOOLCHAIN=stable jcode --no-update --quiet self-dev --build` | PASS for build/publish; N/A for TUI launch in non-interactive harness | Canonical `dev_cargo` acquired the host-wide gate with 8 adaptive jobs and completed the `selfdev` profile in 12.85s. Publication updated `current` to `bd0a995eb-dirty-02de59dfcb73`; the subsequent TUI launch correctly refused non-TTY stdin/stdout. |
| Promote intended shared-server build | `jcode --no-update server promote --json` | PASS (exit 0) | JSON reported `promoted: true`, previous `85f9f5e3b`, candidate `bd0a995eb-dirty-02de59dfcb73`. |
| Gracefully reload shared server | `jcode --no-update server reload --json` | PASS (exit 0) | JSON reported `reloaded: true`, `handoff_ready: true`, and `had_listener: true`; no force-stop was used. |
| Confirm live executable identity | `readlink -f "$HOME/.jcode/builds/shared-server/jcode"`; resolve `/proc/$(pgrep -f '/home/ari/.jcode/builds/shared-server/jcode serve' | head -1)/exe`; `sha256sum` | PASS (exit 0) | Channel and live process both resolved to `/home/ari/.jcode/builds/versions/bd0a995eb-dirty-02de59dfcb73/jcode`; version `jcode v0.79.637-dev (bd0a995eb, dirty)`; SHA-256 `51419de208451131fa58c6be256ec243c65c39c66309f3f1caf9e554853898d6`. |
| Confirm reconnect and stable handoff | Repeat `jcode --no-update server reload --json` after activation and continue this T033 session | PASS (exit 0) | Follow-up reported `already_current: true` and `handoff_ready: true`; this session received the original reload response and continued issuing validation commands after the socket was recreated from inode `1283` to `1755`. |

## Resolved binary identity

Reserved to prove which executable was measured. Resolve symlinks before inspecting identity.

| Item | Exact command | Observed value | Result |
|---|---|---|---|
| Candidate executable path | `realpath target/selfdev/jcode` | `/home/ari/repos/jcode/.worktrees/agent/jcode-l89.4-feedback-tuning/target/selfdev/jcode` | PASS (exit 0) |
| Resolved executable path | `readlink -f target/selfdev/jcode` | `/home/ari/repos/jcode/.worktrees/agent/jcode-l89.4-feedback-tuning/target/selfdev/jcode`; the candidate is a regular worktree executable, not the 70-byte shared-server symlink | PASS (exit 0) |
| Build revision/version identity | `git rev-parse HEAD`; `./target/selfdev/jcode --version`; `sha256sum target/selfdev/jcode` | revision `31ab2cae8fd0f30d346b7552242f04ef1b1b841a`; version `jcode v0.79.636-dev (31ab2cae8, dirty)`; SHA-256 `18a1985f7425dda82d4a4d593d600d9e42582e67d9556e31841a67b86f61adb3` | PASS (exit 0); dirty is expected because T032 was marked InProgress before the build |
| Shared daemon identity after coordinated reload | `readlink -f "$HOME/.jcode/builds/shared-server/jcode"`; resolve the matching live `/proc/<pid>/exe`; `sha256sum` | `/home/ari/.jcode/builds/versions/bd0a995eb-dirty-02de59dfcb73/jcode`; live process resolves to the same path; version `jcode v0.79.637-dev (bd0a995eb, dirty)`; SHA-256 `51419de208451131fa58c6be256ec243c65c39c66309f3f1caf9e554853898d6` | PASS (exit 0) |

## Built-binary isolated-socket validation

Reserved for runtime truth without disturbing the shared daemon.

Canonical command shape:

```bash
./target/selfdev/jcode run --no-update --socket /run/user/1000/jcode-rust-feedback-<unique>.sock '<prompt>'
```

| Step | Exact command or observation | Result | Evidence |
|---|---|---|---|
| Build `target/selfdev/jcode` | `RUSTUP_TOOLCHAIN=stable scripts/dev_cargo.sh build --profile selfdev --bin jcode` | PASS (exit 0) | Canonical `dev_cargo.sh` policy completed the selfdev build in `125.798 s`; only existing profile-spec and dead-code warnings were emitted. |
| Resolve and confirm candidate binary identity | `realpath target/selfdev/jcode`; `git rev-parse HEAD`; `./target/selfdev/jcode --version`; `sha256sum target/selfdev/jcode` | PASS (exit 0) | Resolved worktree executable, revision, version, and digest are recorded in the preceding table. The separately resolved shared-server target and digest differ. |
| Run isolated-socket smoke against candidate | `./target/selfdev/jcode run --no-update --no-selfdev --socket /run/user/1000/jcode-rust-feedback-t032-clean-1350408-1787420633.sock --tool-profile none --max-turns 1 --token-budget 16384 --deadline "$(date -u -d '+5 minutes' +%Y-%m-%dT%H:%M:%SZ)" 'Reply with exactly: T032_SMOKE_OK'` | PASS (exit 0) | The candidate returned the exact line `T032_SMOKE_OK` in `8.84 s`. The bounded single-turn client then reported its expected `max_turns_exceeded` stop reason after the completed response; upload/download were `10249/9` tokens and no tools were exposed. |
| Confirm shared-daemon socket/process was not disturbed | Before and after: `stat -c '%i:%Y' /run/user/1000/jcode.sock /run/user/1000/jcode-debug.sock`; cleanup: `./target/selfdev/jcode --no-update --no-selfdev --socket /run/user/1000/jcode-rust-feedback-t032-clean-1350408-1787420633.sock server stop --force`; `test ! -e /run/user/1000/jcode-rust-feedback-t032-clean-1350408-1787420633.sock` | PASS (exit 0) | Shared socket identities remained `1283:1787403896` and `1284:1787403896`; cleanup returned exit 0, reported no persistent private daemon, and the unique socket was absent. No shared reload or stop command was issued. |

## Final acceptance summary

| Gate | Result | Notes |
|---|---|---|
| FR-001 through FR-021 traced | PASS: 21 of 21 traced | Outcomes: 14 PASS, 4 N/A/rejected-boundary, 3 FAIL (`FR-009`, `FR-018`, `FR-020`) with exact evidence above. |
| NFR-001 through NFR-005 traced | PASS: 5 of 5 traced | All five have exact evidence and outcomes; no incomplete comparison or failed guardrail was presented as success. |
| SC-001 through SC-006 traced | PASS: 6 of 6 traced | Outcomes: 4 PASS, 1 N/A, 1 FAIL (`SC-001`) with exact evidence above. |
| Named constitution principles traced | PASS: 6 of 6 traced | PRIN-002/003/005/008 pass; PRIN-007/010 remain failed for their recorded delivery boundaries. |
| Benchmark evidence complete and comparable | FAIL | The frozen matrix and receipts are reviewable but the six-scenario baseline/candidate comparison remains incomplete. |
| Every experiment adopted or rejected | PASS | `decisions.yaml` accounts for all seven candidates; zero defaults were authorized. |
| Full-feature guardrails passed | FAIL (exit 1) | Seven pre-existing-at-T030-start gates failed at revision `ac31f6598b3aaa40387edf1ff4ef0ed69e9d6b97`; focused success was not substituted and ratchets were not rebaselined. |
| Runtime budgets unchanged | PASS (exit 0) | Canonical collection and comparison both passed all nine maintained metrics with thresholds unchanged; receipt `$JCODE_SCRATCH_DIR/t031-runtime-budget.json`. |
| Coordinated build/reload verified | PASS (exit 0) | Worktree-pinned selfdev build/publish completed; promotion and graceful reload returned `promoted: true`, `reloaded: true`, and `handoff_ready: true`; live `/proc` identity matches the promoted version. |
| Resolved binary identity confirmed | PASS (exit 0) | Candidate path, resolved path, revision, version, and SHA-256 were recorded; the shared-server symlink was resolved separately and was not inspected as if it were a binary. |
| Isolated-socket built-binary smoke passed | PASS (exit 0) | Unique socket returned exact `T032_SMOKE_OK`; cleanup succeeded and shared socket inode/timestamps were unchanged. |

Final delivery decision: **FAIL / not releasable from this evidence set.** T033 closes traceability and proves the coordinated runtime path, but it does not override the incomplete representative benchmark comparison or the unchanged failing full-feature guardrail gate.
