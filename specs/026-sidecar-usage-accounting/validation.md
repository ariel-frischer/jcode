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


## Phase 4, 2026-09-05 (in progress)

Worker executes only T013–T016 (US-003), using bundled phase-4 governance and
skip-read metadata. Prior group 3 is committed and its 40 sidecar / 12 memory-agent
test checkpoint is preserved. Reused T001 setup/ignore verification, no new
technology or dependencies. Root owns all AC-9 effects and Bead closure.

Storage choice: fixed global `memory-usage/requests.v1*.jsonl` ring, not unbounded
per-session files. Reuses lifecycle 1 MiB / three rotations / 30-day constants and
rotation implementation. Session filtering is validated and applied after bounded
reads. A private fixed lock serializes writers/readers with nonblocking try-lock.
Record bytes, decoded record count and safe warning variants have explicit bounds.
Reports always include retained-window-only and loss-history-unavailable warnings:
no missing marker, restart or empty history is evidence of zero lifetime usage.
Reads do not mutate retention. Writes prune expired files in the fixed ring.

T013 test-first: `rtk proxy bash scripts/dev_cargo.sh test -p jcode-base
session::memory_usage::tests --lib --offline`, lane `610014ptj8`, **0 passed / 6
expected failures** against no-op API stubs, including roundtrip 0 vs 2, invalid
selector acceptance and absent uncertainty. No compile error. T014 implements
those storage contracts, with two additional actual mtime/scan/lock fixtures.
All Cargo actions retain the host gate, offline resolution and serialized lanes.


Storage GREEN: lane `798909cz3l`, same focused command, **6 passed**, 34.26s.
Expanded storage regression run in lane `029279p5cw` passed **8 storage tests**,
including actual expired-file exclusion/pruning and 10 MiB malicious-file scan
capping plus nonblocking writer-lock contention. Recorder stubs produced **5
expected behavioral failures** in the same `memory_usage::tests` filter: missing
control outputs, missing persistence, invalid acceptance, unlimited queue acceptance
and dishonest flush success. Temporary dead-code warnings describe stubs only.

Private creation uses Unix 0700 directories/0600 files, rejects symlinks, hardlinks
and unsafe existing mode bits. Windows code reuses canonical `jcode_core::fs`
owner-only protected ACL helpers before writes, with inheritance on the directory
and reparse-point rejection. Windows runtime/ACL evidence is not available in this
Linux phase and remains explicitly unverified rather than a Linux-mode claim.


Native HTTP-to-storage RED: `rtk proxy bash scripts/dev_cargo.sh test -p jcode-base
sidecar::attempt_tests::native_send_reaches_private_recorder --lib --offline`, lane
`096286o2p0`, **0 passed / 1 failed**, observed 0 stored calls vs 1. The real loopback
request and exact payload assertion passed before the missing accounting assertion.
No live provider or real credentials were used.


T016 implementation uses a standard-library fixed `sync_channel(256)` and
`try_send`, with one detached 2 MiB-stack worker per process, not one worker per
session. Submission validates bounded metadata and enqueues without config/disk/
pricing/logging/inference work. The worker resolves current effective controls
before each output, performs private append and emits only validated metadata plus
a closed persistence category through the existing redacting local logger. Invalid
records never reach either sink. Saturating atomics bound all runtime counters.
Flush and shutdown barriers share the queue and clamp caller timeouts to 250 ms.
They report prior losses/failures, not fsync or lifetime completeness. There is no
inference-thread join and no shared-daemon lifecycle mutation.

Global recorder initialization is outside request finalization, at Sidecar
construction. Test builds require explicit private recorder attachment to avoid
accidental writes into real developer diagnostics. Production default binding must
still be validated by the final new-binary local-fixture workflow. Custom existing
observation senders remain compatible and override the default recorder.

First green-attempt lane `28819566wu` stopped at a compile error: the existing
native OpenAI-to-Claude fallback literal needed the new recorder clone. Repaired
that exact constructor, preserving auth/routing/payload behavior. Adding the session
module also crossed its 1,784-line frozen budget. Moved only the five-line
`current_working_dir_string` helper unchanged to existing `session/storage_paths.rs`
and imported it, instead of rebaselining or unrelated cleanup.


Focused GREEN lane `408094qsgi`: **14 tests passed**. Preliminary unoptimized
20,000-iteration timings were clone-only baseline **93 ns/op**, enqueue + drain
**1,244 ns/op**, saturated submission **1,165 ns/op**. Queue capacity is 256,
with a conservative record-memory upper estimate of **231,424 bytes** excluding
channel/allocator bookkeeping. This is direct microbenchmark evidence, not a
maintained end-to-end latency budget or AC-7 acceptance by itself.

Static changed-surface gates after explicit loss handling: dependency boundaries
PASS. Code-size/test-size/panic/swallowed-error scripts exit nonzero on **45/16/4/21**
pre-existing findings. Every reported source is byte-identical to phase HEAD and
zero task-owned paths are findings. No budget file changed. Logs are retained at
`/home/ari/.jcode/scratch/phase4-static/`. Saturating atomics use a shared explicit
CAS loop, failed acknowledgments increment safe flush counters, and the moved
optional cwd helper preserves None fallback without swallowing raw errors.

Lane `630078mm6j`: formatting passed, expanded storage/recorder tests passed,
**41 sidecar tests passed** including the real HTTP-to-private-storage fixture, and
`clippy -p jcode-base --lib --offline --no-deps -- -D warnings` passed. Native
integration observes exactly one stored request with 14 input+output tokens,
unchanged xhigh request payload and no PRIVATE_ sentinels. After recorder shutdown,
a second real local request still succeeds and increments the dropped counter.

The final scan bound is 4,096 retained decoded requests, independently exercised
against 4,800 valid fixture rows in the fixed four-file ring. Oversized external
artifacts are refused on writes and byte-capped on reads, never rotated into a
new oversized accounting artifact. Otherwise-valid records with extra prompt
fields are rejected without including their sentinel in returned diagnostics.
The fixed worker stack uses the conventional 2 MiB size, not a speculative small
stack for the existing configuration parser/logger. No global config was edited.


### Phase-4 acceptance boundaries and remaining gaps

- AC-5/FR-007/FR-009: direct allowlist/private permission/controls/retention/corruption/
  pressure/worker failure evidence now exists. Storage warnings deliberately never
  imply complete lifetime coverage. No durable exact loss marker is invented, so
  reporting in a later process cannot recover past drop counts. Live counters are
  explicitly process-local. Deleting a session does not surgically rewrite the
  global ring, whose metadata expires independently under the accepted 30-day bound.
- AC-7: lane `630078mm6j` measured baseline **141 ns/op**, normal **1,780 ns/op**,
  saturated **2,694 ns/op**. The two runs show expected scheduling/build variability.
  These unoptimized averages include record cloning and (normal case) receiver
  draining. Final group must compare final-build overhead to maintained budgets,
  including cold/default recorder initialization. No end-to-end latency claim yet.
- AC-4: **offline pricing and per-call/session CLI remain pending, not skipped**.
  Native Luna rates remain unknown. Generic provider coverage stays
  `provider_call_only`, not complete hidden physical retry accounting. Group 5 must
  render effective controls, bounded warnings and loss-history uncertainty, not
  treat an empty retained window or fresh process counters as lifetime zero.
- AC-8: owning-crate tests/lint and loopback HTTP are not the TUI binary build or
  production-default recorder/new-binary CLI/private-fixture workflow. Those checks,
  nonzero native routing tests and appropriately bounded full broad guardrails remain
  final-group requirements. The earlier 240-second broad timeout remains incomplete.
  Previously isolated app/TUI and size/panic/swallowed-error baseline failures remain
  disclosed, not waived or relabeled passing.
- No Windows runtime/ACL or other live/paid inference validation was performed.
  No provider payload, Luna xhigh/votes2/cadence3, output cap, routing or persistent
  configuration change was made. No analyze or delegation occurred.
- AC-9 and the final HTML report remain root-owned. Worker made no merge, push,
  install, reload, new worktree, cleanup, provider or Bead mutation.

**Risk: Medium for phase 4.** Blast radius is the sidecar observation finalizer,
private local diagnostic files and local logging selected by existing controls.
Mitigations: bounded queue/state/files/identifiers, independent control gates,
no-follow/private creation, safe error categories, nonblocking failure isolation,
loopback send/payload regression and explicit unknown historical coverage. Rollback
is a normal revert of the phase commit. Required review action is root's final
inline privacy/accounting review with final-group runtime evidence, not a new
worker approval gate or a claim of Bead closure.


Scope-bound polish: used the repository's focused Cargo and static ratchet
contracts instead of the global skill's generic Make/install steps. The full
feature release entry and broad final checks remain final-group work. Updated the
existing lifecycle observability document with implemented adapter boundaries and
created its required `docs/index.md` entry (no index previously existed). No
instructions/skills, release JSON, dependencies or persistent configuration changed.
The only extra extraction was the required small cwd helper described above.


### Final phase-4 checkpoint

Final-source lane `816662nqmx` exited **0**. Every Cargo command below ran through
`rtk proxy bash scripts/dev_cargo.sh`, serially, offline where applicable, without
bypassing the host gate:

| Exact arguments | Final result |
| --- | --- |
| `test -p jcode-base memory_usage::tests --lib --offline -- --nocapture` | 15 passed, 0 failed (9 storage, 6 recorder) |
| `test -p jcode-base sidecar:: --lib --offline` | 41 passed, 0 failed, including native send-to-storage and dead-recorder result preservation |
| `test -p jcode-base memory_agent:: --lib --offline` | 12 passed, 0 failed, including session attribution and unchanged gating |
| `clippy -p jcode-base --lib --offline --no-deps -- -D warnings` | PASS |
| `fmt --all --check` | PASS |

Final run measurements and test summaries:

```text
    Finished `test` profile [unoptimized] target(s) in 10.41s
accounting submission ns/op: clone baseline=144, enqueue+drain=1575, saturated=1607; queue=256, record memory upper estimate=231424 bytes
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 1512 filtered out; finished in 0.50s
    Finished `test` profile [unoptimized] target(s) in 0.20s
test result: ok. 41 passed; 0 failed; 0 ignored; 0 measured; 1486 filtered out; finished in 0.16s
    Finished `test` profile [unoptimized] target(s) in 0.19s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 1515 filtered out; finished in 0.03s
[stderr]     Finished `dev` profile [unoptimized] target(s) in 5.37s
```

No Rust/Clippy warning was introduced. Existing Cargo unmatched profile-package
warnings remain unchanged. Focused lint does not replace the deferred broad suite.
An explicit 16-path scope assertion, documentation-link check and `git diff --check`
passed. Assigned branch remains `agent/jcode-cyko`. No unrelated files are staged.

**T013–T016 complete: 4/4 phase tasks, 16/30 feature tasks.** Completed tasks per
execution group: phase 1 **1/1**, phase 2 **2/2**, phase 3 **9/9**, phase 4 **4/4**,
phases 5 and 6 **0** by the explicit phase boundary. Story totals are US-001 **9**,
US-002 **0**, US-003 **4**, plus **3** setup/foundation tasks without a story.
No phase-4 task remains failed, blocked or skipped. Expected test-first failures
were repaired and verified. The remaining 14 feature tasks have not been executed
by this invocation. Bead **jcode-cyko: sidecar usage accounting** remains open and
root-owned. This is a phase checkpoint, not a feature-delivery or closure report.

**Next action:** controller executes phase 5 in this preserved assigned worktree,
using the private recorder/storage API for offline honest pricing and usable
per-call/session text/JSON reporting, while retaining all recorded final-group gaps.


## Phase 5, 2026-09-05 (in progress)

Only T017–T021 (US-002) execute in this invocation. Bundled phase-5 metadata
was read first, with no separate skip-list/checklist reads. Prior phase 4 is
committed (`895b7691e`) and clean. Its final tests, ignore/setup verification and
known remaining gaps were reused, not redone as implementation tasks. Root retains
Beads, worktrees, all AC-9 effects and the final HTML report.

T017 behavioral RED: `rtk proxy bash scripts/dev_cargo.sh test -p jcode-base
memory_usage::summary::tests --lib --offline`, lane `278492226t`, **0 passed / 7
failed** against compilable no-op report/pricing shapes, 30.61 seconds. Failures
were missing costs/rounding/sessions/uncertainty, not compilation errors.
T018 GREEN: same command, lane `4381221fj9`, **7 passed**, 21.09 seconds.

The reporter reuses provider-core static standard-tier API rate functions for
actual supported provider/model names, never subscription cheapness or a network
refresh. Unknown Luna, missing cache rates/details and creation rates remain
unknown. Known component numerators use checked u128, are summed before half-up
rounding once per call to nano-USD, then checked into u64. Token and aggregate-cost
overflow preserves representable contributions and explicitly flags an excluded
unknown contribution, never wraps or calls a saturated number an exact subtotal.
Pricing is recomputed at read time, not trusted from stored estimates.

The canonical static table may lag current rates and has no per-call service-tier
or long-context premiums. The output discloses this limitation rather than
claiming a bill. No rates or persistent configuration were changed. Zero-rate
arithmetic is tested through the same calculator with synthetic canonical rate
objects; no production free-model alias or CLI-only rate injection is added.

T019 adds private mixed-session/corrupt fixtures and new-binary CLI tests. Source
inspection found normal startup can migrate/harden config, clean files, and emit
first-run telemetry before dispatch. The usage command therefore requires a
narrow early read-only path before startup effects, verified with byte-for-byte
private data-root snapshots. This is necessary AC-4/5 behavior, not an adjacent
startup redesign. TUI remains compiled even with optional default features off.
The focused root binary test uses `--no-default-features --offline`; final full
feature guardrails remain group 6 work.

T019 behavioral RED: `rtk proxy bash scripts/dev_cargo.sh test -p jcode --test
memory_usage_cli --no-default-features --offline -- --test-threads=1`, lane
`681023i4rt`, **0 passed / 5 failed**, 170.73 seconds including first root/TUI
binary compilation. Expected missing `usage` command/help, no compiler failure.
All invocations used cleared environment, private HOME/JCODE_HOME/XDG roots,
telemetry opt-out and `--no-update`, not real credentials or the shared daemon.

T020 parser checkpoint: `rtk proxy bash scripts/dev_cargo.sh test -p jcode --lib
cli::memory_usage::tests --no-default-features --offline`, lane `996886qfx3`,
**1 passed**, 19.58 seconds. Valid default and explicit options parse; missing
values, spurious positionals and unknown options fail.

Necessary narrow extraction: moved only the touched MemoryCommand enum and
map_memory_subcommand function to the new CLI handler, preserving the original
argument reexport and mapping alias. This avoids growing already oversized
args.rs/dispatch.rs. Moved MemorySubcommand metadata with a compatibility reexport
from commands.rs. MemoryManager is lazy at the existing match boundary, so usage
does not create sidecars or load graphs. Other memory operations keep their
existing methods and behavior. No budget, dependency or payload changes.

T021 first CLI GREEN: lane `224184zb5h`, same five private-fixture tests,
**5 passed**, 6.04 seconds. Deterministic complete JSON, unknown costs/usage,
zero usage, mixed sessions and per-call identities, text labels, invalid selectors,
all eight controls, corrupt stored rows and malformed config pass.

The remaining touched commands.rs ratchets predated this phase (its LOC actually
decreased), but rather than deliver a touched oversized dispatcher, the existing
run_memory_command/for_dir functions now move together to the new memory CLI
module with original public/test reexports. Only the lazy manager and usage match
are new behavior. The moved optional-current-directory `.ok()` becomes an explicit
safe warning plus the same None fallback. Original import tests will be rerun.
This is a bounded extraction of the exact function being changed, not graph logic
cleanup or a budget increase.

Lane `54238598gf` stopped at an introduced test-helper compile error before
executing the guarded tests. Two overlapping edits to the same test file lost
the snapshot signature while retaining its tuple-valued insertion. Restored the
consistent directory/mtime/content snapshot signature, with subsequent edits
serialized per file. Extraction also exposed now-unused imports in commands.rs.
This failed lane is not a validation pass; remaining chained checks did not run.

After full touched-dispatch extraction, static scripts report **43 code-size,
16 test-size, 4 panic and 20 swallowed-error paths**, all byte-identical to
phase-base HEAD. No changed path is a finding. Dependency boundaries pass. Raw
static outputs are in `/home/ari/.jcode/scratch/phase5-static/`. No rebaseline.

Corrected lane `668137i321` passed **7 integration tests** (five CLI scenarios
plus the kernel guard probe/check), **2 parser/routing tests**, and **2 existing
project-import tests**, then failed only at root Clippy. CLI subprocesses now
run under a Linux x86_64 per-child seccomp filter killing socket/connect/send
syscalls before effects. The positive control independently observes SIGSYS on
socket creation. No persistent sandbox or daemon settings change. Successful
CLI tests prove this workflow performs zero guarded network calls, not merely
that opt-out/proxy flags were supplied. Non-Linux-x86_64 runs do not get this
syscall evidence and must not claim it. File/directory/mtime/content snapshots
verify no new state directories or same-content rewrites as well as no changed
record/config bytes.

Root lint command: `rtk proxy bash scripts/dev_cargo.sh clippy -p jcode-base
-p jcode --lib --no-default-features --offline --no-deps -- -D warnings`.
It fails on exactly one existing root error, `src/cli/tui_launch.rs:84`,
`clippy::too_many_arguments` (8/7). That file was compared byte-for-byte to
phase HEAD and is unchanged. No unrelated fix, blanket lint allowance or
relabeling of this command as passing. Full AC-8 lint remains outstanding.

### Final focused accounting and executable evidence

Lane `825194xzhc` passed `test -p jcode-base memory_usage:: --lib --offline
-- --nocapture`: **22 passed** (7 summary/pricing, 9 storage, 6 recorder), 0 failed.
`clippy -p jcode-base --lib --offline --no-deps -- -D warnings` passed. All Cargo
commands used `rtk proxy bash scripts/dev_cargo.sh` and the host-wide gate.
The following root `--test memory_usage_cli` Clippy attempt hit the same unchanged
tui_launch baseline before test-target lint and did not reach its chained fmt.
A separate fmt and diagnostic lint run excludes only that proven baseline category
via invocation-only `-A clippy::too_many_arguments`; it is not a clean full-lint
claim and does not change any source attribute, config or ratchet allowance.

Current unoptimized microbenchmark: clone-only **195 ns/op**, enqueue+drain
**1,643 ns/op**, saturated submission **1,345 ns/op**, 20,000 iterations. Fixed
queue **256**, conservative queued-record upper estimate **231,424 bytes**,
excluding channel/allocator bookkeeping. The reporter adds no work to the
submission path: pricing/aggregation runs only on explicit CLI reads. These
measurements preserve preliminary evidence, not maintained-budget or final
optimized-runtime AC-7 acceptance. Final group still owns that comparison.

Direct captured CLI evidence is retained outside this worktree under
`/home/ari/.jcode/scratch/jcode-cyko/phase5-cli-20260906/`: `manifest.json`,
`help.txt`, `session-json.txt`, `text.txt` and the private synthetic source root.
The manifest records resolved executable identity, byte size, SHA-256 and commands.
Each captured command exited 0 with empty stderr and zero PRIVATE_ sentinels.
The session-a JSON reconciles **3 calls**, **4,555,000 known nano-USD**, and
**2 unknown-cost calls** (unpriced native Luna and missing pro cache rate).
The underlying fixture root is unchanged in paths, mtimes and contents.
These captures are separate from the passing kernel-guarded integration tests.

Resolved binary: `/home/ari/repos/jcode/.worktrees/agent/jcode-cyko/target/debug/jcode`

SHA-256: `77207c0e4a4bdfb58f2d9cbf2ba5ffb050cea80a141f0d2fda2a5b66634edb93`

Size: **232,983,472 bytes**. This is the worktree TUI-enabled debug
binary, not PATH's launcher or the shared-server symlink. Optional PDF/embeddings/
Bedrock default stacks were off for focused CLI iteration; the full-feature build/
guardrail contract remains final-group work. No install or activation occurred.

### Final phase-5 checkpoint and requirement mapping

Lane `956924zqlp` exited 0. `fmt --all --check` passes on final source.
Diagnostic root integration-test Clippy with only the already-proven
`too_many_arguments` baseline category excluded also passes. The original root
Clippy command remains failed, not converted into a clean gate. No new warning
suppression, config or budget was committed. Existing Cargo profile-package
selection warnings remain unchanged.

| Phase-5 task | Acceptance evidence | Result |
| --- | --- | --- |
| T017 / T018, FR-005, AC-3/4 | 7 direct price/aggregation tests: real zeros versus absence, cache split, reasoning subset, unknown Luna/models/rates, mixed sessions/ownerless, duplicates, rounding, invalid/overflow and fixed read bounds | PASS |
| T019, FR-006/007, AC-4/5 | Five new-binary fixture scenarios, eight control combinations, malformed records/config, invalid/missing sessions, help, deterministic JSON/text and content/mtime/directory snapshots | PASS |
| T020, FR-006, AC-4/6 | Two parser/routing tests plus two existing project-import regressions after surgical extraction | PASS |
| T021, FR-006/009, AC-4/5 | Actual captured CLI help, per-session JSON and calls text with resolved binary hash; unknown costs, retained/loss/hidden-attempt warnings and current controls | PASS |
| AC-5 offline enforcement | All CLI fixtures under Linux x86_64 syscall denial; guard positive control SIGSYS; zero provider calls/network sends | PASS on this host |
| AC-7 bounded submission | 22 accounting tests include existing queue/pressure fixtures and preliminary numeric timings above | Direct evidence, final maintained-budget check outstanding |
| AC-8 phase-local quality | Format, base Clippy, unchanged-baseline static ratchets, root TUI-enabled debug CLI build/tests, diagnostic test lint | Focused checks pass; full root/broad gates outstanding |
| AC-9 | Root-only delivery effects | Not attempted, not worker-completed |

Scope-bound polish followed repository Cargo commands, not generic Make/install
steps. Updated the existing operator documentation and its index entry. The
repository changelog is per-release JSON written by the release workflow, not
CHANGELOG.yaml; no release entry/version or new changelog system was fabricated
for this phase. No AGENTS/skill/model/profile/routing/cap change.

#### Remaining gaps, not waived or marked passed

- Final group must exercise the production-default recorder and remaining full
  operation/agent/TUI/fallback integration boundaries identified in groups 3–4.
  Injected/helper tests and stored CLI fixtures do not prove every application
  invocation or live provider. No paid/live inference is authorized.
- Native Luna API pricing is explicitly **unknown**. Hidden provider retries stay
  **provider_call_only**. Historical drops/disabled intervals remain unknowable
  without a durable lifetime ledger, so reports always disclose retained-only
  and loss-history-unavailable coverage. None of these are fictitious zeroes.
- Static provider-core pricing may lag public changes. Its existing Sonnet-5 row
  still describes introductory pricing through 2026-08-31. This phase does not
  update rates or claim current invoices. Output explicitly discloses static
  standard-tier provenance, possible staleness and missing tier/context premiums.
- AC-7 still needs final-build/cold-recorder and maintained-budget comparison,
  not just the preliminary submission microbenchmarks reproduced here.
- AC-8 still needs the final required build and full broad guardrails with an
  appropriate time bound, plus nonzero native swarm-routing contract execution.
  The earlier 240-second broad exit 124 was not a pass. Previously isolated
  app/TUI/Mermaid and static baseline findings remain recorded, with the unchanged
  root tui_launch lint added above. No ratchet was raised to hide them.
- Windows/runtime ACL and other unsupported-platform evidence remains unverified.
  The no-network syscall filter evidence applies only to Linux x86_64.
- AC-9, Medium-risk final review, fresh-base merge/push, install_release --fast,
  graceful shared-server reload, cleanup, private HTML report and Bead closure
  are exclusively root-owned and remain unexecuted.

**T017–T021: 5/5 phase tasks complete, 21/30 feature tasks complete.** Execution
groups: 1 **1/1**, 2 **2/2**, 3 **9/9**, 4 **4/4**, 5 **5/5**, 6 **0/9**.
Completed story tasks: US-001 **9**, US-002 **5**, US-003 **4**, plus **3**
setup/foundation tasks. No phase-5 task is blocked, failed or skipped after the
recorded repairs. The nine phase-6 tasks were not executed by this invocation.
Bead **jcode-cyko: sidecar usage accounting** remains open/root-owned.

**Risk: Medium.** Blast radius is offline memory CLI dispatch, read-only local
metadata rendering and cost comparisons. No inference payload, configured Luna
xhigh/votes2/cadence3, routing, caps or persistent config is changed. Mitigations
are canonical type/pricing/control reuse, bounded private reads, safe error labels,
kernel-enforced offline fixtures, compatibility regressions and explicit unknown
coverage. Rollback is a normal revert of the phase commit. Required review action
is root's final inline accounting/privacy review with the remaining AC matrix,
not a new worker approval gate or a feature-completion claim.

Final lane `228282sacf` exited 0: seven guarded CLI tests pass after setting a
per-child zero core limit for the intentional SIGSYS probe, final formatting
passes, and Autospec validates **21 completed / 9 pending / 0 blocked**. The
captured executable SHA-256 still matches the final binary. An explicit 18-path
ownership assertion and `git diff --check` pass. Only this task's paths are staged.

**Next action:** controller executes phase 6 in the preserved assigned worktree,
finishing frozen acceptance evidence and handing AC-9 delivery back to root.
