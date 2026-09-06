# Sidecar usage accounting acceptance evidence

## Scope and authorization

Bead **jcode-cyko**, spec **026-sidecar-usage-accounting**. Assigned checkout:
`/home/ari/repos/jcode/.worktrees/agent/jcode-cyko`, branch `agent/jcode-cyko`.
The current implementation authorization supersedes the preparation-only language
in the bundled spec/plan. It authorizes implementation, not provider or configuration
changes. This evidence was created during **Phase 1**, with setup task T001.
Later phase results are recorded in [validation.md](validation.md). No end-to-end
implementation acceptance is claimed by completing foundation tasks.

Root retains Beads, provider coordination, worktree creation/removal, merge, push,
install, shared-server reload, cleanup, final HTML report and closure. Worker may
edit/test/commit explicit owned files here. No analyze, delegation, persistent
configuration mutation, routing/output-cap changes, live/paid inference, or shared
daemon mutation. Preserve Luna xhigh, two votes and cadence three. Prior cleanup
jcode-5o54 and jcode-lec4 is complete and must not be rerun.

Governance: constitution v1.1.0, PRIN-001 through PRIN-010, read from the bundled
`governance` section of `.autospec/context/phase-1.yaml`. Bundled spec, plan, tasks
and constitution were not separately read. The context reports no checklists, so
no checklist directory was inspected. Current request overrides only the obsolete
preparation-stage ownership restriction, not the frozen acceptance criteria.

## Frozen acceptance matrix, verbatim

Source: `/home/ari/.jcode/scratch/jcode-cyko/comments.json`, acceptance comment
`01a0738b-af58-7bf0-ae8a-89f24f8d3957`, 2026-09-05T21:48:49Z.
The nine rows below retain the complete frozen wording. Current evidence and
limitations are mapped below; original setup wording is not a completion claim.

<!-- frozen-acceptance-start -->
1) Native streamed/nonstreamed provider usage parsing including cached/reasoning and missing/incomplete/error usage -> focused response fixtures.

2) Every supported memory operation and session attribution -> deterministic memory-agent/sidecar integration fixtures; no guessed session identity.

3) Vote/retry/request aggregation and reasoning-output inclusion -> exact total reconciliation and duplicate-event tests.

4) Per-call and per-session user-facing summary including unknown rates/usage and OAuth API-equivalent labels -> built CLI private-fixture smoke/JSON tests.

5) Privacy/local controls -> metrics schema contains no prompt/memory text/secrets, private files, honor existing observability controls, no network telemetry.

6) Existing provider requests/config behavior unchanged -> payload/default regression tests and no persistent-config edits.

7) Hot-path bounded overhead -> focused timing/budget evidence, no synchronous inference or unbounded growth.

8) Relevant focused tests, TUI binary build, guardrails and native swarm routing contract pass; isolated newly built runtime uses fixture/local mock evidence, not paid API tests.

9) Merge-wt handles fresh-base non-rewriting landing/push, install_release --fast and graceful shared-server reload, task-owned cleanup, private HTML report and Bead closure. Narrower deviations require evidence, not silent acceptance shrink.
<!-- frozen-acceptance-end -->

## Requirement-to-task-to-check matrix

Phase 6 reuses the frozen scope and the completed work from phases 1–5.
Exact commands, counts, regression failures/repairs, binary identities and remaining
gaps are retained in [validation.md](validation.md). The final full-feature CLI
lane is `2658475t5f`, native routing lane `070919jnvx`, and complete broad gate
lane `288530j1uy`. Broad gates completed with **six isolated baseline failures**,
not a clean pass or timeout. Root-owned delivery has not occurred.

| Requirement | Frozen rows | Actual tasks / checks | Current result |
|---|---|---|---|
| FR-001 native optional usage | AC-1 | T004–T008, T023: 42 sidecar tests, split/error/partial/duplicate/native parser fixtures | PASS focused fixtures |
| FR-002 authentic attribution | AC-2 | T009–T012, T023/T027: 12 memory-agent tests, 15 interleaved operation observations, production-default two-session/10-send mock workflow | PASS supported-operation fixtures; no complete interactive agent/TUI session replay |
| FR-003 actual attempts once | AC-3 | T004–T012, T023/T027: native physical-send, cancellation, votes, exposed generic segments, duplicated snapshots and per-call/session reconciliation | PASS exposed attempts; hidden provider attempts explicitly unavailable |
| FR-004 subset normalization | AC-1, AC-3 | T002/T003/T004/T017/T023: 9 type tests, parser/price regressions, checked arithmetic, input+output total without extra reasoning | PASS |
| FR-005 honest estimates | AC-4 | T017/T018/T023: 7 price/summary tests, native parser → storage → report, known 355,000 nano-USD with raw creation null, unknown Luna and missing details/rates | PASS; static rates may be stale, not actual bills |
| FR-006 discoverable summaries | AC-4 | T019–T021/T027: 9 default-feature CLI-target tests including child/probe helpers, 8-control production recorder matrix, captured help/text/JSON/filter/invalid selector | PASS Linux fixture workflow |
| FR-007 privacy and controls | AC-5 | T013–T016/T023/T027: private files, schema/sentinel/retention/corruption/controls tests; child and CLI network syscall denial | PASS Linux x86_64; Windows ACL runtime unverified |
| FR-008 behavior compatibility | AC-6 | T012/T023/T028: unchanged config/defaults/reasoning/rates/manifests/scripts, byte-identical native OpenAI request builder and 42 sidecar regressions | PASS scoped checks; no persistent config changes |
| FR-009 visible incompleteness | AC-5, AC-7 | T013–T016/T022/T023: fixed queue/scan/record state, worker failure, saturation, shutdown, rotation/restart and safe warnings | PASS retained-only contract; historical losses remain unknown |
| FR-010 delivery evidence | AC-7, AC-8, AC-9 | T022–T029: full TUI build, all-target/all-feature check, 19 native routing tests, offline fresh binary, complete broad gate | PARTIAL: six isolated baseline gates fail; T030/AC-9 root-only |
| NFR-001 bounded overhead | AC-7 | T022/T026: before/after 20,000 samples, 5 cold starts, unoptimized dev and selfdev profiles, fixed 256 queue and existing pressure ceiling | PASS focused bounds; no optimized/full-daemon budget certification |
| NFR-002 local privacy | AC-5 | T013–T016/T027: all 8 controls, private raw fixture, rendered sentinel absence, network guard positive control | PASS Linux x86_64 |
| NFR-003 ownership and quality | AC-6, AC-8 | T025–T028: no new dependencies/cycles/config, preserved public APIs, changed-surface lint/static checks, full guardrail isolation | Changed surface passes; full broad suite FAILED on baseline findings |

Global acceptance invariant: every summary covers **retained observed requests**,
not complete lifetime usage. Unknown requests/fields/costs remain separate from
known subtotals and actual reported zeros. Native OAuth public API-equivalent
estimates are never invoices or subscription allocations. Generic providers must
state unavailable hidden-attempt detail instead of inventing complete accounting.

## Reused discovery and operation checklist

Source: `/home/ari/repos/jcode/.worktrees/reports/jcode-cyko/worker-sidecar-contracts-20260905.md`.
Read-only discovery at base `4de91e285`, no tests or inference. Line references
below are discovery references, not independently reverified implementation proof.
Verify only the affected seam when editing it rather than repeating broad discovery.

| Operation / caller | Existing seam | Session attribution requirement | Future fixture / accounting boundary |
|---|---|---|---|
| Legacy relevance | `crates/jcode-base/src/memory.rs:1181-1187` | No authentic session available, remain explicitly unattributed | Ownerless relevance send, never last-active-session fallback |
| Session relevance and clones | `memory.rs:1275-1292,1530-1545` | Carry existing sid across clones | Concurrent session relevance isolation |
| Periodic/context extraction | `memory.rs:1115-1126`, `memory_agent.rs:979-1036` | Preserve session through spawned extraction | Incremental extraction, spawned completion and cancellation |
| Final extraction | `memory_agent.rs:171-190` | Retain originating session after close | Detached final extraction and bounded shutdown |
| Agent turn extraction | Agent `turn_execution.rs:1031-1052` | Carry owning agent session | Agent-triggered extraction attribution |
| TUI turn extraction | TUI `turn_memory.rs:218-220` | Carry owning TUI session | TUI-triggered extraction attribution |
| Contradiction check | `sidecar.rs:710-724`, `memory_agent.rs:1124-1125` | Inherit extraction session but use explicit operation kind/identity | Contradiction physical attempt distinct from extraction |
| Rerank and consensus votes | `memory_rerank.rs:249-317,393-413`, `memory_agent.rs:773-798` | Bind session before cloning for votes; existing attributed result names are not session identity | Two concurrent votes with distinct sends, retries and shared logical operation |
| Cluster naming | `memory_agent.rs:1448-1462`, naming entry `:1421` | Thread origin through RetrievalContext `:29`, process_context `:870`, maintenance spawn `:918/:1265`, refine_clusters `:1309` | Naming send retains origin through maintenance spawn, explicit absence when unavailable |
| Generic complete path | `sidecar.rs:300-328`, `provider-core/lib.rs:460-485` | Context when supplied, otherwise explicit absence | Preserve completion API, use existing usage events, disclose unavailable hidden attempts |
| Native OpenAI and resolved fallback | `sidecar.rs:349-520,947-991` | Actual resolved model, one request ID per physical send | Capture optional usage before terminal text return, replacement not summation for cumulative events |
| Native Claude fallback | `sidecar.rs:633-663` | Actual resolved provider/model and existing operation context | Input/output/cache optional usage, failure and incomplete outcome |
| Non-inference work | Maintenance without LLM, skipped/backoff work, credential/reasoning preflight rejection | Do not manufacture session requests | No ledger send until actual dispatch; zero additional inference |

Paths abbreviated after their first mention retain the discovery report's owning
crate/module. Additional live call sites found during surgical edits must be added
to this inventory and fixture coverage before AC-2 can pass.

## Reuse and implementation constraints

- Native OpenAI has no `max_output_tokens` today. `DEFAULT_MAX_TOKENS` is not
  evidence of a native OpenAI 1024 cap. Instrument the send, not preflight.
- Static provider-core pricing supplies optional public API rates. Luna has no
  known rate in the discovery. Do not use another model's price, OAuth monthly
  cheapness, routing reference costs or network-refreshing model-pricing lookup.
- `memory_log.rs` is not a safe unchanged metrics sink: ambient session, memory
  content, HOME path and uncapped append are inappropriate here. RAM usage logs
  are unrelated. Preserve structured logging's redaction and rate limits.
- The approved plan identifies existing lifecycle master/persistence/log controls,
  1 MiB/30-day bounded storage policy and a 256-entry nonblocking worker pattern.
  The discovery's earlier lack of a global opt-out is not proof none exists.
  Verify effective lifecycle controls at the implementation seam. Base cannot
  depend on app-core. Share narrow storage primitives, not a new telemetry system.
- No raw errors, prompt/memory text, credentials, account identifiers, cwd or
  content-derived operation names in records. Keep allowlisted bounded metadata,
  optional usage and safe error categories. Failure must not block inference.

## Setup verification

Phase-one initial Git status was clean and branch was `agent/jcode-cyko`.
The existing `.gitignore` covers Rust target/debug/release, backup/profiling files,
`.env*`, editor files and temporary artifacts. No missing critical pattern for this
phase was found. Bounded root detection found no Dockerfile, ESLint/Prettier config,
package.json/.npmrc, Terraform file or Helm Chart.yaml. This documentation-only
phase introduces none of those technologies, so no ignore files were changed.
No project configuration values or credentials were read into output.

See [validation.md](validation.md) for exact executed checks and outstanding gaps.


## Phase-6 handoff and risk

**Local execution complete through T029. Not MERGE-READY.** T030/AC-9 stays
Pending and exclusively root-owned. Full guardrails ran to completion (765.91s,
exit 1): existing Mermaid Clippy, provenance, code/test-size, panic and swallowed-
error gates failed. Each was isolated from this task's changed surface, without
rebaselining or source allowances. `cargo machete` is unavailable. Default-feature
TUI build, full all-target/all-feature check, focused tests, 19 native routing tests,
onboarding and final 9-test CLI suite passed. Diagnostic root-test lint excluded
only the proven existing too_many_arguments category and is not a full-lint pass.

Current direct capture manifest:
`/home/ari/.jcode/scratch/jcode-cyko/phase6-cli/manifest.json`.
It records the final executed `target/debug/jcode`, SHA-256
`8a9987c0da0fadda7ba58ebf27d7012cf411068746f852c35154109c8554abcb`,
305,242,880 bytes, commands/results and unchanged private fixture state. The
kernel-denied default-feature integration suite independently exercises that
built CLI plus the production-default recorder. No shared daemon was used.

**Remaining boundaries:** complete interactive agent/TUI sessions and automatic
credential-based fallback were not replayed end to end. Direct operation/native
send/model-resolution fixtures and owning-crate checks provide narrower evidence.
Windows ACL/runtime, optimized performance and a complete canonical daemon-budget
report are unverified. Native Luna prices, hidden transport attempts, historical
losses, static-rate freshness and tier/context premiums remain explicitly unknown,
not invented zeros or invoices. The broad baseline failures require root disposition
before claiming full AC-8 acceptance; they were not silently waived by task statuses.

**Risk: Medium.** Blast radius is memory-operation attribution, asynchronous local
accounting/storage and offline cost presentation. Mitigations are bounded validated
metadata, independent controls, private retention, nonblocking fail-open submission,
exact fixtures and known/unknown separation. No inference request/config/routing/cap
changes or added paid calls. Rollback is ordinary reversion of this feature's owned
commits, with retained diagnostics still subject to existing controls and retention.
This worker performed inline scope/privacy review and no delegation. Root retains
final review, fresh-base landing/push/install/reload/cleanup, private HTML report
and closure. No root-owned delivery effect or human approval was claimed.

**Next action:** root reviews the isolated AC-8 failures and remaining evidence
boundaries, then decides readiness for its T030 delivery workflow.

Implementation/test commit: `d8e32604c9ab233bc910d50025c15accc5d298d3`. The following receipt commit
contains only task completion and handoff metadata. Feature progress is **29/30**,
phase 6 **8/9** with root-only T030 Pending. See validation.md for per-phase and
per-story totals.

## Root disposition after fresh-base validation

Root reproduced 95 passing focused/default-CLI tests on current dev plus this
feature. Full guardrails completed with six unchanged baseline failures and 71
independently byte-verified failing paths. Constitution PRIN-007 permits acceptance
with disclosed baseline debt, not a clean global pass. See validation.md.
AC-9 remains pending until installation, activation and task-owned cleanup.


## Root AC-9 delivery receipt, 2026-09-06

Landed and pushed merge adfbe843566d586c2916163a82d62f3fba86282a to dev.
Canonical scripts/install_release.sh --fast finished release build in 2m45s,
exit 0. Launcher/current/stable and shared-server are pinned to installed version
adfbe8435. The installer gracefully reloaded the live server. Follow-up reload
JSON reported had_listener=true, forced=false, already_current=true and
handoff_ready=true. Running ancestor PID 514942 resolves to the same immutable
adfbe8435 executable. No force-stop or credential/config/routing changes.

Installed release SHA-256: 9fae06077830307acf0d55e57d077be5c0680371d0e99c4f44452a9ad6ad5605.
Installed private-fixture CLI smoke passed: session-a has three calls, 4,555,000
known nano-USD and two unknown-cost calls. Fixture paths/bytes/mtimes unchanged,
no PRIVATE_ sentinels. This complements the nine full-feature kernel-denied CLI
tests and the 95 fresh-base focused checks, not a paid inference test.

All source/acceptance evidence and private runtime receipts are retained under
.worktrees/reports/jcode-cyko/. Required static HTML risk report:
.worktrees/reports/session-jcode-cyko-20260906.html. Six unchanged baseline gates
remain disclosed under PRIN-007, not falsely marked as global passes.
The completed read-only worker was stopped; no task controller or Cargo work
remains. The clean merged feature worktree and local branch were safely removed.
Only this documentation-receipt integration staging remains until this commit is
pushed, then the root removes it and records final cleanup/closure on the Bead.
This receipt changes no executable source, so installed code remains adfbe8435.
Installer stale-server cleanup safely refused due to explicit JCODE_SOCKET and
retired no processes. Canonical promotion independently fixed the shared-server
channel pin, and actual daemon identity was verified afterward.

Next action: inspect local accounting with `jcode memory usage --calls`.
