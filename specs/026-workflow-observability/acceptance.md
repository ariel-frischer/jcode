# Workflow observer runtime acceptance

Bead: `jcode-upir`. Implementation remains in progress. This is the requirement-to-check map, not a claim that checks have passed.

| Requirement | Check | Evidence/state |
| --- | --- | --- |
| FR-001/009 disabled behavior and bounds | Config default, TOML, environment and template focused tests | Config tests and repaired-candidate disabled runtime passed: no workflow messages or registry directory |
| FR-003 explicit ownership | Registry idempotence, conflicting owner rejection, unobserve ownership, persisted duplicate rejection | Registry identity, duplicate ownership, private persistence and transactional save tests passed |
| FR-005 reconnect and continuity | Registry last-good round trip, live phase transition, opted-in reconnect snapshot | Persistence tests and actual reconnect/live A→B→A ResumeSession ownership passed |
| FR-007/008 safe optional artifact input | Missing, malformed, oversized, leaf and ancestor symlink replacement fixtures | Bounded reads, post-registration and concurrent ancestor replacement tests passed on Linux |
| FR-004/006 evidence-based health | Separate progress checkpoint and activity clocks, quiet-not-failed, sticky credit failure, explicit retry recovery | 37 app-core workflow tests plus fake producer, sticky credits, malformed source and explicit retry/completion runtime passed |
| FR-002/NFR-001 idle main model | Built isolated-socket fake producer with model request counter staying zero, no observer command invocation | Repaired-candidate T006 passed, see evidence below |
| Protocol compatibility | Legacy Subscribe omits default-false capability, new events only on opted-in connection, resume follows current session, slow-client coalescing | Repaired-candidate T006 passed, see evidence below |
| NFR-002 bounded presentation | Actual candidate debug tester frames at 40/80/120 columns and short height | Repaired-candidate T006 passed, see evidence below |
| Delivery | Focused checks, scoped format/lint, build, repository guardrails, independent review and HTML risk report | Focused checks/build/review/runtime passed. T007 guardrails and T008 report/landing remain pending |
| Landing and data safety | Required risk authorization, fresh-base non-rewriting integration, push, install/reload, owned worktree cleanup | Pending. No installed configuration enablement without concrete approval |

## Resolved testing-path hazard

`selfdev test` installs a shell Cargo shim with `JCODE_DEV_CARGO_SCRIPT` rooted at the caller checkout. A nested `cd <worktree> && cargo test` can still invoke the root wrapper, which changes back to its own directory. Use the absolute worktree-owned `scripts/dev_cargo.sh` path for coordinated tests. Confirm the compiler package path is the candidate worktree. The first resumed attempt was cancelled after detecting root paths, and is not candidate validation evidence.

## T002 milestone evidence

Coordinated task `295033dkp3` ran the absolute worktree `scripts/dev_cargo.sh test -p jcode-app-core --lib workflow::` and passed all 15 tests on Linux. Earlier red runs failed on missing persistence, unsafe parent traversal, absent observer progress/credit health, and absent transactional registration before each implementation. Scoped rustfmt and `git diff --check` passed. Windows no-reparse handle traversal is implemented but has not been compiled or runtime-tested in this milestone. No scheduler, tool action, protocol event, renderer, installed config or daemon was changed.

The optional controller adjunct JSON is an explicit producer contract, not a claim that current Autospec already emits it: `state` is one of `running`, `waiting`, `retrying`, `blocked`, `failed`, `completed`, `stopped`; optional `error_code` maps known quota, rate-limit and authentication codes to safe observer-authored text. Other JSON fields, including raw `message`, are ignored. Missing/invalid artifacts retain prior confirmed lifecycle and task progress with a warning. First observation does not invent a recent activity timestamp. Actual later task/lifecycle changes supply activity evidence.

## T003 and partial T004 transport milestone

Final task `685392pxdr` passed all 98 protocol tests (including typed workflow-event round trip) and `check -p jcode --bin jcode`. Scoped formatting and `git diff --check` passed. Unrelated pre-existing formatter drift in three touched fixture files was preserved rather than included as cleanup.

T003 is committed as `61e38fe1b`. Explicit `bg observe` takes an `observation` object with absolute `working_dir`, contained `tasks_file`, optional `status_file`, and optional `label`. Its owner comes only from `ToolContext.session_id`. `bg unobserve` takes the returned `task_id`. Both actions dispatch before `background::global()` and do not register process ownership, notifications or model wakes. One process-local store uses canonical workflow config and private `workflow/registry.json`. Disabled config avoids store initialization. A failed initial registry load stays visible and requires preservation/repair followed by an operator restart, never an automatic restart.

Expected-red task `769187xysk` failed on absent observe schema. Task `928054yx5g` passed all 8 focused `tool::bg::tests`, covering legacy actions, idempotent ownership, unknown nested owner rejection, disabled input, and unchanged artifacts.

Partial T004 adds the additive omitted-when-false `Subscribe.workflow_progress` capability and a typed `WorkflowStatus { session_id, workflows }` event. Only opted-in connections on an enabled server receive it. An in-process watch channel replaces old snapshots rather than adding tick events to the unbounded client queue. Delivery resolves the current connection session, refreshes after a session change, and bounds a slow writer wait. The server independently polls registered artifacts on a skip-missed-tick interval through one awaited blocking observation at a time. Failures retain the last batch with a safe observer warning. Native member/activity integration is still **not implemented**, and the TUI constructor intentionally remains opted out until T005.

Expected-red protocol task `07962990l7` failed because the capability did not round-trip. Task `268540ykc9` passed that wire test and all 17 app-core `workflow::` tests, including owner isolation, bounded snapshots and 10,000 updates coalescing to the latest snapshot across a simulated session switch. Task `4304024ei8` passed all 97 then-current protocol tests but was interrupted by an unrelated server reload during the downstream TUI check. The isolated rerun `565023oyx7` passed `check -p jcode-tui --lib`. These are unit/compile checks, not built-binary idle-model or actual client-frame evidence.

Remaining T004/T006 safety checks include native clocks and lifecycle retention, explicit registered worktree association without stealing another parent-owned session, cross-daemon registry write safety when a private server shares Jcode home, pre-allocation parser limits, persisted-snapshot sanitization, actual connection opt-in/resume isolation, and fake-producer/no-model runtime evidence. Full polish, changelog, guardrails, independent review and installation remain final-phase gates. No feature install, shared-daemon mutation, configuration edit, merge or push occurred in this milestone.

## T004 native and safety implementation milestone

T004 implementation is complete with unit/compile evidence, not yet built-runtime acceptance. Task `788503dhns` passed 29 focused `workflow` tests in `jcode-app-core` and `check -p jcode --bin jcode`, with compiler paths confirming the candidate worktree. Existing profile-package warnings remain. Earlier expected-red checks caught competing registry writers and unsafe persisted display fields (`0517414oly`), oversized YAML strings (`157892h333`), absent native APIs (`3398397je4`), ready metadata clearing explicit failure (`606412p5no`), and exhausted native retention capacity (`6894696ycs`). Each has a focused regression test and repair.

Safety commit `786c9a8` adds a stable private sidecar file lock held for the observer-store lifetime. A second daemon sharing the same Jcode home cannot write stale registry state. It receives an observer-unavailable diagnostic instead. The lock is nonblocking, is released on process exit, and must never be manually deleted while live. Disabled workflow still does no registry I/O. YAML phase processing is streaming, with at most one bounded phase in memory, capped task sequences, scalar limits before cloning, and no producer-controlled collection preallocation. Persisted display strings reject control/bidi injection and oversized text.

Native records are stored with caller ownership, separate activity/checkpoint timestamps, explicit terminal evidence, and optional stable registration association. Native-only configuration works without enabling or reading Autospec artifacts. The passive server samples authoritative live member lifecycle, never transient bus status as lifecycle. Tool, todo, model-activity and streaming bus events supply activity clocks. Only semantic todo ID/status changes advance native checkpoints. Raw streamed output and provider errors are not retained in the workflow snapshot. Known credit failures map to safe authored text.

Only new live sessions in explicitly registered exclusive worktrees can gain parentless phase association. Preexisting/restored parentless sessions cannot be attached merely by matching a directory. Existing persisted associations survive reconnect. A different explicit parent always wins and cannot be stolen. Registered task counts, checkpoint and workflow identity are not replaced by a phase's native todo counts. Latest native failed/blocked/stopped evidence overrides an otherwise-running artifact. A completed child alone does not establish controller completion. Waiting/ready metadata does not clear an explicit native terminal outcome, while authoritative running retry can recover it.

Native records are capped at 256. Expired absent records free capacity, except the latest registered phase evidence is retained to prevent old failure becoming running merely through pruning. Associated expired older phases are removable after supersession and disappearance. Capacity errors preserve the prior registry transaction. Bus lag is visible as a safe warning. Filesystem polling remains one awaited blocking operation per interval with skip-missed ticks, and transport remains per-connection coalescing.

T005 renderer/capability opt-in and T006 actual isolated-socket/no-model/reconnect/credit/slow-client evidence are still pending. Candidate tester frames, cross-process lock runtime evidence, Windows checks, full polish/guardrails and independent review remain acceptance gates. No install, shared-daemon mutation or installed configuration enablement occurred.


## T005 main-TUI implementation milestone

The generic panel now reserves a bounded surface immediately below the top bar, before transcript/input layout. Each visible workflow shows explicit health and known counts, activity/stage or observer-authored detail, and separately labelled activity/checkpoint ages. Unknown ages remain `?`, quiet is suspicion, and explicit failure/blocked/observer errors sort ahead of ordinary work. The panel takes at most half the remaining height, leaves at least eight rows for transcript/input, and suppresses itself when empty, disabled, too narrow or too short. Renderer work is bounded and uses only generic snapshots, with no source I/O or model calls.

The App retains at most 256 remote snapshots, ignores other-session events, clears snapshots on a real session switch, and retains them for same-session History/SessionId refresh. The TUI subscription and renderer use canonical `workflow.enabled && workflow.show_panel` gating. No installed config was changed.

Expected-red task `2311238oc5` caught absent panel layout/content. The compiled test binary separately caught absent owned-session reducer handling. Coordinated task `6086618izn` passed five focused TUI `workflow_` tests, including 40/80/120-column content, overflow, unknown ages, error precedence, control/bidi display bounds, tiny/zero-height rendering, stale-owner rejection, same-session History retention, and History/SessionId switching. Candidate package paths were confirmed. Existing unmatched profile-package warnings remain. Scoped rustfmt ran, with unrelated baseline fixture formatting preserved. Actual candidate tester frames and protocol/no-model runtime acceptance remain T006 work, not claimed by these unit tests.


## Independent review and repaired native boundaries

The required independent read-only review ran through native OpenAI OAuth `openai:gpt-6-astra`, medium, reviewer boar, against T005 commit `e7b03ba`. Retained report: `/home/ari/repos/jcode/.worktrees/reports/jcode-upir/worker-workflow-reviewer-20260905T2044.md`. Reviewer found seven actionable correctness/availability issues, not a demonstrated data leak or command execution path. Root reproduced six in expected-red task `279642yj8l`, then reproduced the batch-truncation finding in `450621po5l`. The reviewer performed no builds or mutations and was stopped after archival.

The repair lane maps all authoritative native lifecycle vocabulary, including crashed/done/queued/spawned/running_stale. Blocked evidence now stays sticky through ready/waiting/unknown/stale metadata without incorrectly treating it as retention-terminal. Missing native records receive a persisted absence timestamp and can retire after bounded retention. Admission overflow produces an owner-scoped warning instead of aborting all artifact/existing-worker updates. Latest registered-phase evidence remains pinned so pruning cannot resurrect a failed controller.

Clock admission now uses the same explicit-parent/session ordering as sampling, evicting lower-priority clocks when necessary. The server reports sampling capacity limits. Global batches preserve the store-bounded population before owner filtering; per-owner limits include an explicit omission warning. Persisted native records reject self-ownership, invalid IDs/sources, impossible counts and terminal-health contradictions.

Source precedence is explicit: newly observed controller terminal or retry evidence establishes a persisted recovery epoch. It may supersede an older native outcome, but an unchanged Running artifact or task checkbox change cannot. The epoch survives later Running metadata so old failures cannot resurrect after explicit recovery. Native lifecycle changes have an independent persisted timestamp. Same-second conflicts conservatively retain native adverse evidence. New controller terminal evidence resets its retention timestamp rather than inheriting an old phase's timestamp.

Final coordinated task `743733otow` passed 37 focused app-core `workflow` tests, five TUI `workflow_` tests, and `check -p jcode --bin jcode`, all using candidate worktree paths. Existing unmatched profile-package warnings remain. Two interim failures after repair were diagnosed as malformed test artifacts (missing phase number/empty task file), corrected to valid fixtures, and rerun. These results include absent Waiting churn across reopen without freezing artifact progress, blocked metadata/retry continuity, owned-clock starvation, late-owner batch visibility, explicit per-owner overflow, controller completion/retry/Running continuity and persisted corruption rejection. Scoped formatting and diff checks passed.

An isolated selfdev executable built successfully in task `70538068ju` at the worktree's explicit `CARGO_TARGET_DIR`, but that binary represents **pre-review-repair e7b03ba**, not the latest repaired source. The initial private smoke correctly refused startup with no credentials. A second smoke used inert OAuth-shaped placeholders under a fresh private HOME/JCODE_HOME, no real credentials, and blocked HTTP(S)/ALL proxy endpoints. Task `778970d8bo` confirmed the executable via `/proc/<pid>/exe`, a real opt-in Subscribe handshake, and typed workflow updates. Runtime fixture root: `/home/ari/.jcode/scratch/wf-_tmn9csd`; probe script: `/home/ari/.jcode/scratch/workflow-smoke-probe.py`. The private server exited via the probe's cleanup. This is only handshake evidence, not full T006 acceptance. Rebuild the repaired candidate before actual fake-producer/credit/reconnect/slow-client/model-counter/tester-frame assertions.


## T006 repaired-candidate runtime acceptance

The repaired `d83ecba` executable was rebuilt by coordinated task `120127y9od`
using the absolute candidate `scripts/dev_cargo.sh` and explicit candidate
`CARGO_TARGET_DIR`. The maintained Linux-only, standard-library harness is
`scripts/test_workflow_runtime.py`. It takes an absolute candidate binary and
requires `JCODE_SCRATCH_DIR`. No installed configuration or shared daemon is used.

The final maintained-harness run exited **0**. Durable evidence, including actual
40×30, 80×30, 120×30 and 80×10 candidate frame JSON, PTY output, snapshot evidence,
exit receipt and binary digest, is retained outside the removable worktree at
`/home/ari/repos/jcode/.worktrees/reports/jcode-upir/runtime-20260905T2118/`.

Verified through real private sockets and processes:

- Caller-owned direct `bg observe`, valid fake producer task progression, safe
  quota failure text, last-good counts across malformed artifacts, explicit retry
  recovery, and controller completion without changing workflow identity.
- Owner B sees no owner A workflow. Reconnect restores failure. Live
  `ResumeSession` A→B→A follows current ownership. Legacy Subscribe receives no
  new workflow variant. Live resume returns `History`, not initial `SessionId`.
- Forty-eight additional registrations saturate an unread socket while a healthy
  client continues receiving 24 producer transitions. The bounded slow writer
  disconnects, and reconnect immediately restores the latest completed snapshot.
- A second daemon sharing only the fixture Jcode home reports observer-unavailable
  health rather than competing for the registry lock. A disabled daemon creates
  no workflow registry directory and emits no workflow events, even to opt-in clients.
- Actual candidate PTYs display failed/count/credit/checkpoint content at
  40/80/120 columns. The 10-row terminal suppresses the panel. All four preserve
  two-line input and remain idle with zero conversation messages. Semantic frame
  JSON does not include workflow text, so PTY output corroborates visible content.
- Rejecting proxy counters stay unchanged during observer-only producer and
  backpressure intervals. Main owner histories stay empty and input/output model
  token counters remain zero. Existing TUI onboarding/usage startup performs
  rejected CONNECT attempts to auth/usage hosts. Those are recorded separately,
  not mislabeled as model requests or allowed to reach the network. All auth data
  is inert fixture text, never real credentials. No model message is submitted.

Harness corrections were test-contract issues, not production repairs: snapshot
count is `completed`, retry health is `waiting`, live resume emits `history`, and
fresh-home onboarding must be dismissed before main-TUI capture. Visual capture
is explicitly enabled. Atomic tester command-file replacement avoids observing
partially written commands. Every private process/tester is cleaned up by its owner.

## T007 guardrail investigation, not a passing gate

Full guardrails were attempted, but are not yet acceptance evidence. An inherited
exported `cargo` function routed an initial run back to root despite a PATH shim,
then mixed root artifacts into the candidate target. The corrected runner removes
`BASH_FUNC_cargo%%`, pins `JCODE_DEV_CARGO_SCRIPT` to the candidate, and uses the
actual `/usr/sbin/cargo` behind the recursion guard. The initial assumed
`~/.cargo/bin/cargo` does not exist on this host. Invalid runs are retained under
`/home/ari/.jcode/scratch/workflow-guardrails/` and are not claimed as candidate passes.

The corrected check found stale background-types artifacts missing `workflow`
although the candidate exports it unconditionally. Debug dep-info includes both
workflow-aware and workflow-absent artifacts after the erroneous root build.
The owned guardrail process group was stopped. Invalidate candidate-local
workspace-package build artifacts or use a clean target before rerunning, and
verify compiler paths. Do not repeat a nested root-shim build or run parallel Cargo.

Static gates also identify feature-owned fallback patterns in
`workflow/observer.rs`, `config/workflow.rs`, and `ui_workflow.rs`, plus small
integration-line growth in already oversized files. Resolve these without hiding
unrelated baseline drift or blanket rebaselining. Clippy also reports an existing
`jcode-tui-mermaid` type-complexity error that needs baseline isolation. Full
formatting passed, but no full compile/Clippy/routing guardrail pass is claimed.
