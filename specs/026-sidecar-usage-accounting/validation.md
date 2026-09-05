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
