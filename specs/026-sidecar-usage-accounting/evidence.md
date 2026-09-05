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
The nine rows below retain the complete frozen wording. None has passed yet.

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

Only T001 is bundled in this phase. Later task IDs are deliberately not guessed.
The task column gives T001's evidence obligation and the later implementation
work item by name. Later invocations must bind their actual task IDs and exact test
names here or in `validation.md` before marking the corresponding work complete.
Commands below are acceptance targets, **not executed evidence**. Fixture module
names are planned until implemented. All Cargo work must be serialized through the
repository's `scripts/dev_cargo.sh` when selfdev tooling would target root.

| Requirement | Frozen rows | Task / later work item | Planned command or check | Current result |
|---|---|---|---|---|
| FR-001 native optional usage | AC-1 | T001 mapping; native usage parser fixtures | Focused jcode-base sidecar usage tests: streamed/nonstreamed OpenAI and Claude, split chunks, duplicate terminal, missing/malformed/partial/error/truncated usage, reported zero vs absent | Not run, implementation pending |
| FR-002 authentic attribution | AC-2 | T001 inventory; explicit memory call context | Deterministic memory-agent/sidecar integration fixtures for every inventory row below, two concurrent sessions, spawned/cancelled work, explicit ownerless calls | Not run, implementation pending |
| FR-003 actual attempts once | AC-3 | T001 mapping; request-boundary accounting | Exact send-ledger reconciliation across two votes, retries, resolved fallback, failures and cumulative events; preflight/skipped work has no send | Not run, implementation pending |
| FR-004 subset normalization | AC-1, AC-3 | T001 mapping; usage normalization tests | Assert total = normalized input + output, reasoning <= output, no cache double charge, invalid subsets/overflow unknown | Not run, implementation pending |
| FR-005 honest estimates | AC-4 | T001 mapping; offline pricing aggregation | Known/zero/missing rates, missing cache rates, unpriced actual Luna, partial usage, mixed models, integer arithmetic and OAuth labeling fixtures | Not run, implementation pending |
| FR-006 discoverable summaries | AC-4 | T001 mapping; memory usage CLI and integration fixtures | Newly built `jcode memory usage --help`, `--session <fixture-id> --calls --json`, text, empty/invalid selectors, deterministic ordering and null unknowns | Not run, implementation pending |
| FR-007 privacy and controls | AC-5 | T001 mapping; private bounded diagnostic adapter | All enabled/persist_session_events/emit_structured_logs combinations, sentinel absence, private permissions, bounded identifiers, malformed stored records, no uploads | Not run, implementation pending |
| FR-008 behavior compatibility | AC-6 | T001 mapping; exact request/default regression fixtures | Exact native payload and fallback/default tests, omitted native OpenAI max_output_tokens, Luna xhigh/votes2/cadence3 preserved; explicit config/scope diff review | Not run, implementation pending |
| FR-009 visible incompleteness | AC-5, AC-7 | T001 mapping; recorder pressure and failure fixtures | Queue saturation, worker death, unwritable storage, restart, retention/rotation and bounded flush tests preserve results and expose partial coverage | Not run, implementation pending |
| FR-010 delivery evidence | AC-7, AC-8, AC-9 | T001 mapping; final validation and root handoff | Focused tests, TUI binary build, `rtk proxy bash scripts/check_guardrails.sh`, `rtk proxy bash scripts/check_swarm_routing_contract.sh` with nonzero tests, private deterministic new-binary workflow; AC-9 root only | Not run, implementation pending |
| NFR-001 bounded overhead | AC-7 | T001 mapping; hot-path measurements | Before/after normal/saturated submission timings with numeric maintained-budget comparison, fixed queue/state/scan bounds, zero added inference | Not run, implementation pending |
| NFR-002 local privacy | AC-5 | T001 mapping; negative privacy fixtures | No sensitive sentinel in stored/logged/rendered metrics, no network telemetry, private files and every effective control combination | Not run, implementation pending |
| NFR-003 ownership and quality | AC-6, AC-8 | T001 mapping; dependency and compatibility gates | Dependency boundaries and guardrails, no duplicate pricing/telemetry framework, compatible text completion API and no unrelated cleanup | Not run, implementation pending |

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
