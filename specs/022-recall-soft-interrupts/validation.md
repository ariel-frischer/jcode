# Alt+Q server soft-interrupt recall validation

Branch: `agent/jcode-c9u-alt-q-soft-queue`
Base: `340a8db36`
Bead: `jcode-c9u`

The shared daemon and installed launcher remain on the pre-fix binary until integration. Runtime acceptance must use a newly built binary on a private socket before landing, then verify the installed revision after reload.

## Acceptance matrix

| Requirement | Boundaries | Direct check |
|---|---|---|
| FR-001 empty composer only | TUI, regression | TUI tests with text and with pending images assert no request or mutation; existing nonempty-composer behavior remains unchanged |
| FR-002 newest one at a time | Server, TUI, runtime | Server queue test and repeated TUI result test return newest then next-newest; isolated-socket workflow recalls exactly one message per operation |
| FR-003 ownership/source/order | Server, runtime | Mixed-client, unowned, User/System/BackgroundTask queue test; isolated workflow confirms unrelated queued work survives |
| FR-004 text and images | Protocol, persistence, server, TUI, runtime | Wire round trips, persisted fixtures, server result assertions, TUI composer assertions, and isolated multimodal recall preserve exact text and ordered images |
| FR-005 authoritative result only | Protocol, TUI, runtime | Result contract identifies the operation; pending, error, stale, malformed, and disconnected TUI cases do not mutate; isolated workflow applies only a confirmed result |
| FR-006 one in flight | TUI, runtime | Repeated Alt+Q test observes one outbound request; isolated workflow confirms repeated input while pending does not enqueue another operation |
| FR-007 idempotent retry | Server, TUI, runtime | Same-operation replay and duplicate-result tests show one removal and one application; isolated response-loss retry reconciles the same result |
| FR-008 legacy fails closed | Persistence, server, regression | Legacy persistence fixture loads without ownership; mixed-queue test leaves unowned entries untouched; prior persisted sessions remain loadable |
| FR-009 compatibility | Protocol, server, TUI, regression | Existing request/Ack shapes, local Alt+Q cycle/original-slot tests, and CancelSoftInterrupts tests retain their expectations |
| NFR-001 requester-only safety | Protocol, server, runtime | Lifecycle routing and mixed-owner tests prove only the requester receives its owned payload; isolated multi-client workflow checks requester-only delivery |
| NFR-002 race reliability | Server, TUI, runtime | Bounded deterministic consumption, replay, duplicate, and disconnect scenarios run repeatedly without duplication, unrelated loss, or unconfirmed composer mutation |
| NFR-003 boundary coverage | Protocol, persistence, server, TUI, regression, runtime | Focused crate tests, format/checks, guardrails, isolated built-binary workflow, and live installed verification cover every affected boundary |

## Validation order

1. Observe focused protocol, persistence, server, and TUI tests fail before production code.
2. Iterate each boundary to green without changing existing cancellation semantics.
3. Run established local Alt+Q and soft-interrupt cancellation regressions.
4. Run formatting and focused Cargo checks serially.
5. Run repository guardrails once on the implementation candidate. Record only verified unrelated baseline failures.
6. Build the TUI through the coordinated selfdev path and validate the resolved new binary on a private socket.
7. Obtain independent safety review for cross-client preservation, system/background preservation, and response-loss reconciliation.
8. Integrate through `merge-wt`, install with `scripts/install_release.sh --fast`, gracefully reload, confirm binary identity, and verify live lowercase Alt+Q behavior.

## Known baseline evidence

The initial broad guardrail run on the clean-base/prototype investigation failed existing all-feature check, Clippy/warning budget, quality ratchet provenance, oversized-file/test, panic-prone, and swallowed-error gates. The unsafe prototype was removed. Do not rerun the identical broad workflow until a complete candidate exists. New behavior and tests should use small modules/files instead of growing already oversized `input.rs`, `remote/key_handling.rs`, or `remote_startup_input_02/part_01.rs`.

## T010 focused and regression validation

Completed on 2026-09-01 in the isolated feature worktree.

| Check | Result |
|---|---|
| `cargo test -p jcode-protocol recall` | PASS, 2 focused wire-contract tests |
| `cargo test -p jcode-base soft_interrupt` | PASS, 2 persistence and legacy-default tests |
| `cargo test -p jcode-app-core recall_` | PASS, 6 queue, replay, ownership, and requester-routing tests |
| `cargo test -p jcode-tui soft_interrupt_recall` | PASS, 10 state-machine and remote Alt+Q tests |
| `cargo test -p jcode-tui alt_q` | PASS, 4 established local and remote queue-recall tests |
| `cargo test -p jcode-tui original_slot` | PASS, 2 local and remote original-slot restoration tests |
| `cargo test -p jcode-app-core session_control_handle_does_not_wait_for_busy_agent_lock` | PASS, 2 control-path tests including clear-all soft-interrupt cancellation |
| `cargo fmt --all -- --check` | PASS |
| `cargo check -p jcode-protocol -p jcode-base -p jcode-app-core -p jcode-tui` | PASS |

The first `original_slot` run exposed a feature-caused regression: remote Alt+Q applied the server-recall empty-composer gate before the established local queue helper, so a second local cycle was blocked. The final implementation calls the unchanged local helper first and applies empty-composer gating only before server recall. Both the original-slot regressions and all focused server-recall tests pass after this fix.

The affected-crate check retains three unrelated pre-existing TUI warnings for `animated_tool_color` and `ImageExpandLevel::next`. The feature-specific test-only `is_pending` helper is gated with `cfg(test)` and no longer adds a production warning.

## T011 isolated running workflow

Completed on 2026-09-01 with the coordinated selfdev TUI build and a disposable runtime directory. The shared daemon was not reloaded or used for acceptance.

| Check | Result |
|---|---|
| Resolved executable | `/home/ari/repos/jcode/.worktrees/agent/jcode-c9u-alt-q-soft-queue/target/selfdev/jcode` |
| Executable identity | `jcode v0.81.745-dev (3bea94631, dirty)`; SHA-256 `900974cb354eaa2605ed74942516f12b61ed771ff1cd2493d64fe2fe0ccc1b9c` |
| Private server socket | `/home/ari/.jcode/scratch/t011-runtime-1788258012-3321343/jcode.sock` with sibling debug socket; isolated `JCODE_RUNTIME_DIR`, no shared-daemon reload |
| Real TUI harness | PASS; the built executable ran in the PTY-backed tester and reported the same version and remote persistent connection |
| Newest one-message recall | PASS; while a turn was processing, `T011-FAST-OLDER-PRESERVED` then `T011-FAST-NEWEST-RECALL` were queued and lowercase Alt+Q restored only `T011-FAST-NEWEST-RECALL` |
| One in flight / repeated key | PASS; two immediate lowercase Alt+Q events produced one newest recall and did not skip to or consume the older message |
| Unrelated pending preservation | PASS; after clearing the recalled composer, the next lowercase Alt+Q restored `T011-FAST-OLDER-PRESERVED`, proving it survived the first operation in order |
| Composer blocking | PASS; lowercase Alt+Q with `T011-FAST-OLDER-PRESERVED` still in the composer left the input byte-equivalent |

The PTY debug channel supports text input and key injection but not attaching an image fixture or forcing a transport response-loss result. Exact ordered image restoration, unavailable/stale/duplicate result immutability, and established local fallback are therefore covered by the focused T010 protocol/server/TUI and regression tests rather than duplicated in this runtime probe. Scratch evidence was recorded in `t011-runtime-acceptance.txt` during the run.
