# Workflow observer runtime acceptance

Bead: `jcode-upir`. Implementation remains in progress. This is the requirement-to-check map, not a claim that checks have passed.

| Requirement | Check | Evidence/state |
| --- | --- | --- |
| FR-001/009 disabled behavior and bounds | Config default, TOML, environment and template focused tests | Foundation milestone passed, runtime disabled-work test still pending |
| FR-003 explicit ownership | Registry idempotence, conflicting owner rejection, unobserve ownership, persisted duplicate rejection | Registry identity, duplicate ownership, private persistence and transactional save tests passed |
| FR-005 reconnect and continuity | Registry last-good round trip, live phase transition, opted-in reconnect snapshot | Persistence round trip passed, runtime pending |
| FR-007/008 safe optional artifact input | Missing, malformed, oversized, leaf and ancestor symlink replacement fixtures | Bounded reads, post-registration and concurrent ancestor replacement tests passed on Linux |
| FR-004/006 evidence-based health | Separate progress checkpoint and activity clocks, quiet-not-failed, sticky credit failure, explicit retry recovery | Observer fixtures passed, native event fixtures pending |
| FR-002/NFR-001 idle main model | Built isolated-socket fake producer with model request counter staying zero, no observer command invocation | Pending |
| Protocol compatibility | Legacy Subscribe omits default-false capability, new events only on opted-in connection, resume follows current session, slow-client coalescing | Pending |
| NFR-002 bounded presentation | Actual candidate debug tester frames at 40/80/120 columns and short height | Pending |
| Delivery | Focused checks, scoped format/lint, build, repository guardrails, independent review and HTML risk report | Pending |
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
