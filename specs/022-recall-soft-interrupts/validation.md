# Alt+Q server soft-interrupt recall validation

Branch: `agent/jcode-c9u-alt-q-soft-queue`
Base: `340a8db36`
Bead: `jcode-c9u`

The shared daemon and installed launcher remain on the pre-fix binary until integration. Runtime acceptance must use a newly built binary on a private socket before landing, then verify the installed revision after reload.

## Acceptance matrix

| Requirement | Direct check |
|---|---|
| FR-001 empty composer only | TUI tests with text and with pending images assert no request or mutation |
| FR-002 newest one at a time | Server queue test and repeated TUI result test return newest then next-newest |
| FR-003 ownership/source/order | Mixed-client, unowned, User/System/BackgroundTask queue test |
| FR-004 text and images | Protocol, persistence, server result, and TUI composer image assertions |
| FR-005 authoritative result only | Pending, error, stale result, malformed result, and disconnect TUI tests |
| FR-006 one in flight | Repeated Alt+Q test observes one outbound request |
| FR-007 idempotent retry | Same operation replay and duplicate result tests |
| FR-008 legacy fails closed | Persistence legacy fixture and mixed-queue test |
| FR-009 compatibility | Existing local Alt+Q cycle/original-slot tests and CancelSoftInterrupts tests |
| NFR-001 requester-only safety | Lifecycle routing and mixed-owner tests |
| NFR-002 race reliability | Bounded deterministic consumption, replay, duplicate, and disconnect scenarios |
| NFR-003 boundary coverage | Focused crate tests, format/checks, guardrails, isolated built-binary workflow, live installed verification |

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
