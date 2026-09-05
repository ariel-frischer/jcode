# Validation: 026-sidecar-usage-accounting

## Phase 1, 2026-09-05

Bead: **jcode-cyko**, sidecar usage accounting. Phase scope: T001 setup only.
Status: **1/1 phase tasks complete, 1/30 feature tasks complete**. Six task phases
exist. Phases 2–6 have not been executed by this invocation. T001 has no user-story
assignment, so US-001, US-002 and US-003 have no completed implementation evidence.
This is not a completion report for the feature or Bead.

### Executed checks

| Command / check | Result |
|---|---|
| `rtk git status --short` and `rtk git branch --show-current` before mutation | Clean assigned `agent/jcode-cyko` checkout |
| Read `.autospec/context/phase-1.yaml` metadata first | Used bundled spec/plan/tasks/governance, no separate skip-list reads, no checklist scan |
| Read frozen comments and existing worker discovery report | Reused acceptance and operation evidence, no broad source rediscovery |
| `rtk proxy autospec update-task T001 InProgress` | Pending → InProgress, correct feature selected |
| Bounded Python root tooling detection and `.gitignore` inspection | Rust/editor/temp patterns already present, no detected additional root technology requiring ignore changes |
| `rtk proxy python3` frozen-row equality and matrix assertions | PASS: 9/9 verbatim acceptance rows, FR-001 through FR-010 and NFR-001 through NFR-003 present |
| `rtk proxy autospec update-task T001 Completed` | InProgress → Completed |
| `rtk proxy autospec artifact specs/026-sidecar-usage-accounting/tasks.yaml` | PASS: 30 tasks, 1 completed, 29 pending, 0 blocked, 6 phases |
| `rtk git diff --ignore-all-space --stat` | Task update has exactly one changed line ignoring formatter whitespace |
| `rtk git diff --check` | PASS |

The frozen-row assertion reads only the accepted comment source and the new
`evidence.md`, splits the numbered matrix into nine rows and asserts each complete
row occurs unchanged in the evidence. Requirement assertions check all thirteen
matrix IDs. No bundled task/spec/plan/constitution was separately loaded for this
check. Autospec itself necessarily updates and validates its task artifact.

Autospec rewrote YAML indentation while updating status. This is generated CLI
serialization, not a task-plan change. The semantic task diff is only T001 status.
Do not hand-edit generated task serialization merely to hide that tool behavior.

### Polish and outstanding evidence

Loaded `/polish` and applied the repository-specific artifact-validation contract
for a documentation/setup phase. No runtime code, CLI surface, configuration,
dependency or operator documentation changed, so no implemented-feature changelog
entry, build, install or behavioral claim is appropriate. No global skill changes.
`evidence.md` links this record, which retains the phase's exact commands/results.

Full acceptance remains outstanding: native parser and attempt collection,
operation/session propagation, private bounded persistence, offline pricing,
usable CLI per-call/session summaries, deterministic privacy/control/error fixtures,
new-binary local workflow, measured overhead, TUI build, full guardrails and nonzero
swarm-routing contract tests. These are scheduled implementation-phase gates,
not skipped requirements or evidence supplied by this setup document. No Cargo,
network, paid inference or shared-runtime check was run in Phase 1. The inspected
`check_guardrails.sh --skip-slow` still invokes native routing tests, so it is not
misrepresented as a schema-only validator here. PRIN-005's documentation-only
artifact checks are the checks exercised in this phase. AC-8 must pass later.

AC-9 remains exclusively root-owned: fresh-base non-rewriting integration/push,
install, graceful reload, task-owned cleanup, private HTML report and closure.
No root-owned effect was attempted and no Bead was closed.

### Risk and handoff

**Low risk for this phase.** Blast radius is two evidence Markdown files and a
generated task status update in this worktree. No runtime behavior changed.
Mitigation is verbatim acceptance validation and explicit unexecuted status for
all implementation evidence. Rollback is an ordinary revert of the phase commit.
Root's eventual feature review still needs to assess attribution/privacy risk.

**Next action:** execute Phase 2 in the same assigned worktree, using its bundled
context and keeping the frozen matrix in [evidence.md](evidence.md) intact.


## Phase 2, 2026-09-05

Bead **jcode-cyko**, sidecar usage accounting. This invocation executes only
**T002 and T003**, the two phase-2 foundation tasks. No user-story task is assigned
in this phase. The invocation's `--phase 2` boundary takes precedence over the
broader authorization to implement all phases. No later phase was executed.

### Contract decisions and acceptance scope

- Added the public `jcode_session_types::memory_usage` module. Existing lifecycle
  contracts and dependencies are unchanged. Types are pure metadata, with no
  runtime, networking, storage, provider, pricing lookup or configuration work.
- `MemoryCallContext` carries optional authentic session ownership and logical
  operation identity. `MemoryRequestObservation` has a separate physical-send ID,
  version, timestamp, resolved provider/model/effort, closed auth/outcome and
  attempt-coverage enums. Generic hidden transport retries can be marked
  `provider_call_only` rather than claiming a complete physical-attempt ledger.
- Optional `u64` usage distinguishes absence from actual zero. Input already
  includes cache reads and creations. Output already includes reasoning. Checked
  subset and sum validation rejects impossible accounting without coercion.
  `total_tokens` returns `Result<Option<u64>, ValidationError>` so invalid data
  cannot silently become ordinary unknown data.
- Cost values use integer **nano-USD (10^-9 USD)**. `public_api_equivalent` is
  explicitly not a bill. Unknown basis cannot contain an invented cost. A full
  estimate must equal the known subtotal. Pricing calculation/rounding, missing
  model rates and CLI presentation remain later-phase work.
- Session summaries preserve per-field unknown counts, known subtotals, retained
  timestamp bounds and retained/partial/unavailable coverage. They reuse existing
  `LifecycleObservabilityStatus`, with no parallel control resolver.
- New record structs deny unknown JSON fields. Closed enums cannot carry text.
  Identifier validation is bounded to 128 bytes and static errors never echo
  rejected values. Model grammar additionally supports provider/model and tags.
  **Syntax is not a privacy sanitizer or proof of ownership.** Future collectors
  must use authentic IDs and resolved model metadata, never content-derived IDs.
- `validate()` is required before persistence and after deserialization. Serde
  enforces scalar types and field allowlists, not cross-field invariants. Future
  bounded storage reads must cap bytes before JSON allocation and convert raw
  deserializer errors into safe categories. Output summary types are not an
  alternative durable store.

T002/T003 provide direct foundation evidence for FR-001/FR-004 optional/subset
semantics, FR-002/FR-003 identity representation, FR-005 cost representation,
FR-006 summary representation and FR-007 allowlisting/bounds. They do **not** prove
provider parsing, authentic propagation, exactly-once runtime accounting, pricing,
storage privacy or a usable CLI. All nine frozen acceptance rows remain open as
end-to-end feature requirements.

### Executed checks

All Cargo commands used the assigned checkout's `scripts/dev_cargo.sh`, local
`target/`, offline dependency resolution and the host-wide Cargo gate. Background
jobs were bounded to ten minutes, with compile progress and a 60-second stall
notification. Existing gate contention was respected, not bypassed.

| Command / check | Result |
|---|---|
| Read `.autospec/context/phase-2.yaml` first | Used bundled spec/plan/task/governance data. No separate skip-list or checklist reads |
| `rtk git status --short`, branch and local target checks | Clean phase start, assigned `agent/jcode-cyko`, worktree-owned target. T001's ignore-file verification reused, no new technology introduced |
| `rtk proxy autospec update-task T002 InProgress` | Correct feature selected |
| `rtk proxy bash scripts/dev_cargo.sh test -p jcode-session-types memory_usage --lib --offline` before types | Expected missing-contract compilation failure, not counted as behavioral red evidence |
| Same command after minimal type shapes with validation stubs | **Behavioral RED: 6 passed, 3 failed**, specifically invalid usage, unsafe identifiers, unsupported schema |
| `rtk proxy autospec update-task T002 Completed`, then `T003 InProgress` | Test-writing checkpoint recorded before invariant implementation |
| Same focused test command after implementation | **GREEN: 9 passed**, zero failures |
| `rtk proxy bash scripts/dev_cargo.sh test -p jcode-session-types --offline` | **19 passed**, including 3 unchanged lifecycle tests and 7 session-search tests. Zero doctests |
| `rtk proxy bash scripts/dev_cargo.sh fmt --all --check` | PASS. Formatting applied only to the two owned new Rust files |
| `rtk proxy python3 scripts/check_dependency_boundaries.py` | PASS, no dependency changes |
| `rtk proxy bash scripts/check_swarm_routing_contract.sh --self-test` | PASS inventory deletion detection only. This is **not** a nonzero execution of native routing tests |
| `rtk proxy rustc --version` | Distribution stable Rust 1.95.0, 59807616e, 2026-04-14. `rustup` unavailable, so current upstream stable parity is not verified |
| `rtk git diff --check` | PASS at initial implementation checkpoint |

Background evidence IDs: initial unresolved-contract run `729212reoh`, behavioral
red `8382656xf7`, first green `905628mi3g`, crate/Clippy lane `944243wxb5`.
These are tool logs, not substitutes for the commands and outcomes above.

### Ratchet diagnosis and polish

Loaded `/polish` and `/chlog`. Repository `changelog/README.md` owns release JSON
entries and explicitly skips internal-only changes. There is no CHANGELOG.yaml.
No user-facing feature or release is claimed here, so no release entry or new
changelog system was added. API documentation lives with the types and this
tracked spec evidence. No tracked `docs/` document was added, so no docs index
entry is needed. No global skill/config edit, install or runtime reload occurred.

Initial swallowed-error guard identified `validate().ok()?` in the new total
helper. Replaced it with an explicit `Result<Option<u64>, ValidationError>` and
strengthened tests to assert invalid totals return errors. No budget rebaseline.

After that repair, code-size, test-size, panic and swallowed-error scripts still
return nonzero for **46, 16, 4 and 21 existing paths**, respectively. A bounded
Python check captured each script's diagnostic path list and compared every
reported file byte-for-byte to `git show HEAD:<path>`. All are unchanged from
phase-base HEAD `e55fbfa7a`, and no new accounting path remains in their findings.
The full existing-path diff is empty. These baseline violations were not repaired
because they are unrelated work, and are not represented as passing gates.

### Outstanding feature evidence and ownership

Phases 3–6 still owe native response/attempt collection, live-operation context
propagation, bounded private diagnostics, offline price arithmetic, usable CLI
text/JSON summaries, controls/privacy/retention/failure fixtures, measured hot-path
overhead and new-binary deterministic runtime evidence. Full TUI build and nonzero
native-routing checks remain required before root-owned delivery. No paid/live
inference, sensitive config reads, provider/routing/output-cap changes, new
requests, shared daemon mutation, merge, push, install or cleanup was performed.
Luna xhigh, votes2 and cadence3 were not changed.

**Risk: Low for phase 2.** Blast radius is an additive dependency-light metadata
module and tests, plus task/evidence updates. No caller uses the new module yet.
Mitigations are explicit boundary validation, error propagation, regression tests
and unchanged lifecycle serialization. Rollback is a normal revert of the phase
commit. Root retains final feature risk review and all AC-9 effects, including
HTML report and Bead closure.


### Final phase-2 checkpoint

- **T002 and T003 complete: 2/2 phase tasks, 3/30 feature tasks.** Phase 1 has
  1/1 complete. Phases 3–6 remain unexecuted, with 27 pending tasks. US-001,
  US-002 and US-003 each have zero completed story tasks. No phase-2 task is
  blocked, failed or skipped. The Bead remains open and root-owned.
- Final `rtk proxy bash scripts/dev_cargo.sh test -p jcode-session-types --offline`
  after the error-propagation repair passed **19/19** (background `250698abr8`).
- `rtk proxy bash scripts/dev_cargo.sh clippy -p jcode-session-types --all-targets
  --offline -- -D warnings` passed (background `944243wxb5`). Cargo itself emits
  existing unmatched profile-package warnings for cosmic-text, swash,
  unicode-linebreak and yazi. Cargo.toml is unchanged. No Rust/Clippy warning was
  produced by the new contracts.
- `rtk proxy autospec update-task T003 Completed` and
  `rtk proxy autospec artifact specs/026-sidecar-usage-accounting/tasks.yaml`
  passed: six phases, 30 tasks, 3 completed, 27 pending, zero blocked/in progress.
- Full `scripts/check_guardrails.sh` was actually invoked, with
  `CARGO_NET_OFFLINE=true`, `JCODE_REMOTE_CARGO=0`, and `timeout 240` after the
  final tests. A shell-level Cargo function forwarded each invocation to this
  worktree's `scripts/dev_cargo.sh`, while `JCODE_IN_DEV_CARGO=1` bypassed that
  function with `command cargo` inside the wrapper to avoid recursion. The
  host-wide gate stayed enabled. No persistent config or script changed.
- **Full guardrails incomplete, exit 124 at 240 seconds**, during
  `cargo check --all-targets --all-features`. Disk preflight (83.5 GiB free,
  10 GiB reserve), module resolution and full formatting passed first. No full
  all-feature check, broad Clippy, onboarding or native-routing execution pass is
  claimed. This bounded attempt is not AC-8 acceptance. The separately verified
  baseline ratchet failures above also remain open. Later feature validation
  must run the full suite serially with a build-appropriate bound.

No standalone HTML report was created because the worker is not closing the Bead
and root explicitly owns the final report. No merge/push or task-worktree cleanup
was performed. The unchanged frozen matrix remains the feature acceptance target.

**Next action:** execute Phase 3 in this same assigned worktree using its bundled
context, preserving these contracts and finishing the remaining runtime outcome.


## Phase 3, 2026-09-05 (in progress)

Bead **jcode-cyko**, sidecar usage accounting. Invocation boundary: **T004–T012
only**, all US-001. The explicit `--phase 3` flag governs this invocation despite
broader authorization of subsequent phases. Used the bundled context first,
including governance and skip-read metadata. No bundled artifact was separately
read and no checklist scan occurred. Reused T001 ignore verification and frozen
operation discovery. No new technology or dependency has been introduced.

### Test-first collection and attribution evidence

All Cargo commands below were prefixed with `rtk proxy bash scripts/dev_cargo.sh`,
run in this assigned worktree with offline resolution and the host-wide Cargo gate,
as bounded background jobs. No shared daemon, paid inference, external provider,
persistent configuration, route, output cap, votes or cadence was changed.

| Exact Cargo arguments / check | Result |
|---|---|
| `test -p jcode-base sidecar::usage_tests --lib --offline`, normalization stubs | Behavioral RED: 1 passed, 5 failed on missing usage. Job `689602un8a`, 365s including gate wait and first compile |
| Same after normalization | GREEN: 6 passed. Job `111601w8lr` |
| `test -p jcode-base sidecar::attempt_tests --lib --offline`, before send instrumentation | Initial run exposed expected terminal-close BrokenPipe in fixture server. Fixed fixture to accept only BrokenPipe/ConnectionReset |
| Same corrected fixture, before instrumentation | Behavioral RED: all 3 failed on missing observations. Job `303905lu4w` |
| `test -p jcode-base sidecar:: --lib --offline`, OpenAI instrumentation | GREEN: 36 passed, including unchanged payload/default/route tests. Job `414015a5a3` |
| `test -p jcode-base sidecar::provider_attempt_tests --lib --offline`, before Claude/generic instrumentation | Behavioral RED: 2 failed on missing observations. Job `528276x4qr` |
| `test -p jcode-base sidecar:: --lib --offline`, both native backends and generic adapter | Initial compile required explicit `Result<String>` on generic async block. After repair GREEN: 38 passed. Job `726674t1ju` |
| `test -p jcode-base memory_agent::usage_tests --lib --offline`, before owner propagation | Behavioral RED: 1 passed, 1 failed because final extraction lost its owner. Job `894322ked7` |
| `test -p jcode-base memory_agent:: --lib --offline`, after propagation | GREEN: 12 passed, including cadence/gating regressions and 15-record interleaved reconciliation. Job `0222718psy` |
| `test -p jcode-base memory_agent::usage_tests --lib --offline`, after memory.rs call-site bindings | GREEN: 2 passed. Job `111792adpt` |

Native transport fixtures use loopback-only ephemeral listeners, fake credentials,
private sentinel text and clients with proxy disabled. They exercise actual HTTP
sends, not paid endpoints. Manual failed/success/resolved-model sends reconcile
four observations and 56 tokens, including usage reported with an HTTP failure.
This tests the physical-send boundary, not automatic credential-based fallback
end to end. Existing model-resolution/fallback tests separately remain passing.
SSE fixtures split response bytes, duplicate terminals, include malformed lines,
truncate before terminal completion, and distinguish complete/incomplete/error.
An aborted in-flight request emits exactly one cancelled observation. Unpolled
work and native reasoning preflight rejection emit none.

The operation fixture uses the real relevance/extraction/contradiction helpers,
real concurrent consensus, cluster-naming helper and spawned final-extraction
helper with an injected deterministic Provider. Two sessions each retain seven
observations, ownerless relevance retains one, request IDs are unique, vote IDs
share one operation per session, and reported input+output reconciles to 150.
Empty consensus emits no observation. This is an injected-provider integration
fixture, not a complete agent/TUI session or embedding/maintenance workflow.
Actual direct agent/TUI and memory-manager call-site bindings also need owning
crate checks and final scope review. Do not represent these tests as AC-8
new-binary CLI evidence or full end-to-end acceptance for every caller.

### Collector contracts and remaining feature work

- Optional native usage preserves zero versus absent fields. OpenAI input includes
  cache reads, output includes reasoning. Claude uncached/read/creation components
  are summed with checked arithmetic only when reported. Missing components stay
  unknown. Repeated snapshots replace rather than add. Safe outcomes contain no
  response/error/content strings.
- Native guards are created immediately before send after credential/reasoning
  preflight. Drop submits once and preserves cancellation. Resolved native model,
  effort and auth class are captured, including fallback clones. Pricing stays
  unknown until the existing public-rate adapter in later phases.
- Generic calls use the existing complete_simple message/options and text
  semantics while consuming exposed TokenUsage/RetryRollback events. Segments
  retain one operation, get distinct IDs, and carry `provider_call_only`, unknown
  auth/reasoning and no claim that hidden transport retries are completely
  observed. Retry announcements alone do not create another segment.
- The request-local guard submits via bounded `try_send`; no disk/pricing/logging
  occurs on this path. An atomic loss count covers rejected/unavailable sinks.
  Constructors currently have **no default recorder**. Phase 4 must connect the
  controlled bounded recorder and expose loss/restart/retention uncertainty.
  `with_observation_sender` is the integration seam, not enabled persistence.
- Phases 4–6 still owe controls/private retention/worker fault tests, offline
  pricing, usable per-call/session CLI, new-binary private-fixture workflow,
  numerical hot-path budgets and complete AC-8 evidence. This invocation must not
  skip ahead or claim those requirements completed.

### Scope-bound polish in progress

Loaded `/polish`. No narrower repository polish skill exists. The canonical
`changelog/README.md` uses release JSON and skips internal-only work. No release
entry or CLI capability is claimed at this collector stage. No docs index entry
is needed for tracked spec evidence. Root owns the final HTML report and AC-9.

Initial size ratchets reported growth in memory.rs/memory_agent.rs and a new
oversized sidecar.rs. Only directly touched helpers will be placed in small
owned modules to eliminate new growth. No budget will be raised. Existing
unrelated ratchet findings remain baseline failures until independently checked.

### Resumed phase-3 checkpoint, 2026-09-05

The prior controller's configured 2400-second deadline was not an implementation
failure. This invocation preserved all dirty task-owned sources and resumed T012,
without rerunning completed task implementation or starting execution groups 4–6.
The six execution groups implement the reviewed three-phase plan: this group is
still collection/attribution, not the plan's final delivery phase. No controller,
worker, worktree, configuration, route or shared-runtime mutation was initiated.

Preserved surgical extractions are `memory/sidecar.rs`,
`memory_agent/sidecar_calls.rs`, and `sidecar/accounting.rs`. The remaining moved
`list_all().unwrap_or_default()` triggered the swallowed-error ratchet. Replaced
it with an explicit match and fixed safe warning, preserving empty-list fallback
and never printing the raw storage error. No ratchet baseline was changed.

| Exact command / check | Result |
|---|---|
| `rtk proxy bash scripts/dev_cargo.sh test -p jcode-base memory_agent:: --lib --offline` | 12 passed, 0 failed, including the interleaved 15-record ownership fixture |
| `rtk proxy bash scripts/dev_cargo.sh test -p jcode-base sidecar:: --lib --offline` | 40 passed, 0 failed. The preserved source includes two more normalization regressions than the earlier 38-test checkpoint |
| `rtk proxy bash scripts/dev_cargo.sh check -p jcode-app-core -p jcode-tui --offline` | PASS, both direct agent/TUI session bindings compile, lane `164556q4q9` |
| `rtk proxy bash scripts/dev_cargo.sh fmt --all --check` | PASS before the final safe-fallback repair |
| `rtk proxy bash scripts/dev_cargo.sh clippy -p jcode-base -p jcode-app-core -p jcode-tui --lib --offline -- -D warnings` | FAIL, unchanged dependency `jcode-tui-mermaid/src/lib.rs:441`, `clippy::type_complexity`; lane `227187aim3`. Not a lint pass |
| `rtk proxy python3 scripts/check_dependency_boundaries.py` | PASS |
| Code-size, test-size, panic, swallowed-error budget scripts | Nonzero with 45, 16, 4, 21 existing-path findings after repair. Every reported file is byte-identical to `git show HEAD:<path>`. Zero changed-surface findings |
| `rtk proxy autospec artifact specs/026-sidecar-usage-accounting/tasks.yaml` before final status update | PASS: 30 total, 11 completed, T012 in progress, 18 pending |
| `rtk proxy git diff --check` | PASS |

The failed Mermaid dependency was independently compared byte-for-byte to phase
HEAD. The follow-up lint uses `--no-deps` to isolate the three owning crates, not
to relabel the dependency-inclusive command as passing. Initial attempts to use
shortened size-script names failed because those files do not exist. The actual
canonical scripts are `check_code_size_budget.py`, `check_test_size_budget.py`,
`check_panic_budget.py`, and `check_swallowed_error_budget.py`.

#### Acceptance still owed by later execution groups

- AC-1/2/3/6 have direct fixture and source-binding evidence above, not full
  application/runtime acceptance. Automatic credential-driven fallback, full
  embedding/maintenance invocation and actual agent/TUI-session extraction still
  need final integration coverage. The injected helper fixture must not be called
  a full agent/TUI test. Native Claude production remains nonstreaming as before.
- AC-4: offline costs and usable per-call/session CLI are not implemented here.
  Native Luna pricing is explicitly unknown. Do not substitute another model's
  rates. Generic coverage remains `provider_call_only` with hidden physical
  attempts unavailable, even when reported retry segments are observed.
- AC-5: direct fixture sentinel exclusion is passing, but private bounded storage,
  control combinations, corruption/retention/restart/loss/worker-fault tests remain
  required. Production constructors still have no default recorder. Wire the
  existing bounded sender seam to controlled persistence in group 4.
- AC-7: no numerical overhead or maintained-budget acceptance is claimed. Record
  normal/saturated timings, fixed bounds and zero extra inference in final work.
- AC-8: owning-crate checks are not a TUI binary build or new-binary CLI/runtime
  validation. Full broad guardrails and nonzero native routing execution are
  reserved for the final group. The earlier 240-second exit 124 remains incomplete
  and requires an appropriate-bound rerun. Baseline failures remain disclosed.
- AC-9 remains root-only. No merge, push, install, reload, cleanup, HTML completion
  report or Bead closure is authorized to this worker.

**Risk: Medium for the collected phase.** Blast radius is native/generic response
accounting and memory operation attribution, with existing completion results and
payloads preserved. Mitigations are loopback/injected fixtures, bounded submission,
safe metadata, explicit unknown coverage and unchanged-configuration audit.
Rollback is a normal revert of the phase commit. Root's final inline review must
include the remaining storage/privacy and runtime acceptance evidence.

### Final phase-3 checkpoint

- `rtk proxy bash scripts/dev_cargo.sh clippy -p jcode-base --lib --offline
  --no-deps -- -D warnings`: **PASS**, lane `351957eckd`, 26.83 seconds.
- `rtk proxy bash scripts/dev_cargo.sh test -p jcode-base memory_agent::usage_tests
  --lib --offline` after the safe-fallback repair: **2 passed**, 0 failed, lane
  `259375l1pi`. The following `fmt --all --check` passed on the final source.
- `rtk proxy bash scripts/dev_cargo.sh clippy -p jcode-base -p jcode-app-core
  -p jcode-tui --lib --offline --no-deps -- -D warnings` failed with three
  pre-existing app-core errors: `server/client_lifecycle.rs:3605`
  (`too_many_arguments`), `server/state.rs:343`
  (`needless_borrows_for_generic_args`), and `tool/bash_foreground.rs:211`
  (`result_map_or_into_option`). All three files are byte-identical to phase HEAD.
  No unrelated repair or warning suppression was applied. Full app/TUI lint
  acceptance is still outstanding, not silently replaced by a narrower pass.
- `rtk proxy autospec update-task T012 Completed`, then
  `rtk proxy autospec artifact specs/026-sidecar-usage-accounting/tasks.yaml`:
  **PASS**, 12 completed, 18 pending, no in-progress/blocked tasks, six groups.
- Completion counts: group 1 **1/1**, group 2 **2/2**, group 3 **9/9**.
  Groups 4–6 remain pending by the explicit phase boundary. US-001 has **9**
  completed tasks, US-002 and US-003 have **0**. Three completed foundation/setup
  tasks have no story assignment. No phase-3 task is failed or skipped.
- Bead **jcode-cyko: sidecar usage accounting** remains open and root-owned.
  This is a phase checkpoint, not a completed-feature or delivery report.

**Next action:** execute group 4 in this preserved assigned worktree, connecting
the bounded controlled private recorder before pricing/CLI and final validation.
