# Go SDK architecture

**Status:** Current state
**Date:** 2026-08-11
**Owner:** Ariel Frischer's Jcode distribution

## Ownership

There are three distinct owners:

- `crates/jcode-harness-api` owns server behavior and the serialized protocol-v1 request/event contract.
- `sdk/go` owns Go lifecycle behavior, public Go types, transport/client implementation, private-runtime supervision, tests, examples, and publication payload.
- `github.com/ariel-frischer/jcode-go` is a protected public publication destination. It owns its repository governance, specifications, automation, task/worktree state, and public maintainer docs, but not a competing SDK implementation.

Wire-facing Go changes are checked against the Rust harness enums and schema tests. CLI output, ACP, and the public Go decoder are not alternate wire authorities.

## Layering

```text
crates/jcode-harness-api     serialized protocol-v1 authority
            |
sdk/go/protocol             framing, raw events, unknown preservation
            |
sdk/go transport + client   socket I/O, correlation, bounded subscriptions
            |
Session + Turn              compatible Session API, owned terminal lifecycle
            |
launch + process_*          optional private-runtime ownership and cleanup
```

The module remains dependency-free. It does not embed Rust, parse terminal output, speak ACP, or add a second runtime manager.

## Lifecycle and compatibility

`Connect` is non-owning. Closing its client closes only the transport. `Launch` is owning: it creates or selects isolated state, starts the runtime, waits for readiness, connects, and performs bounded shutdown/cleanup. `DetachInstance` transfers that ownership explicitly.

The established `Session.Send`/`Session.Events` contract remains available. `Session.StartTurn` composes acceptance, ordered events, server cancellation, and immutable terminal outcome without changing legacy behavior. Method contexts bound individual waits; the StartTurn lifecycle context owns local turn lifetime. Turn cancellation is sent at most once, and a cancellation acknowledgement is not terminal until the server emits a terminal event.

Protocol v1 preserves snake-case tags/fields, additive unknown fields, and unknown event kinds. ReasoningDone and ToolExec match `ApiEvent` in the harness. Typed terminal failures expose stable bounded classifications without raw provider messages, frames, prompts, credentials, session IDs, or private paths.

## Typed-event ownership and publication policy

`crates/jcode-harness-api` is the wire authority. `ApiEvent::publication_contract`
is an exhaustive match that assigns each variant one disposition: owned-turn,
outside-owned-turn, or an explicitly reviewed internal filter at the translation
boundary. Every owned-turn variant has exactly one `EventSemanticClass`:
content/progress, advisory/lifecycle metadata, terminal, permission, or
tool-effect. The class is contract metadata, not an inference from a tag or
payload shape.

The canonical `sdk/go` module owns the public concrete `TypedEvent` values,
`SemanticClassOf`, and `Turn` policy. An owned `Turn.Next` publishes
content/progress, terminal, permission, and tool-effect events to handlers. It
filters only advisory/lifecycle metadata and emits one bounded redacted
`Observation` for the first ignored advisory event in that turn. Unknown,
malformed, nil, or unclassified values terminate the owned turn with a bounded
`CompatibilityError` wrapped in `ErrProtocolFailure`.

The legacy `Session.Events`/`TypedEventStream` seam remains additive and
preserves `UnknownEvent{Kind, Fields}` for unknown protocol-v1 kinds. That
diagnostic compatibility does not make unknown events safe to ignore in an
owned turn. The parity gate checks the Rust inventory against Go decoding and
classification; a new owned event must be typed and classified before
publication, while a non-public event must have an explicit filter rationale.

`github.com/ariel-frischer/jcode-go` is only a protected publication projection
of `sdk/go`. There is no reverse synchronization path and no second SDK source
of truth. `scripts/sync-jcode-go.sh` is the reviewed, fingerprint-bound apply
boundary; its preview and fixture checks must pass before any separately
authorized public publication.

## Observability and process safety

Observers receive synchronous bounded lifecycle classifications for connect, turn, launch, cancellation, and shutdown. Implementations must be concurrency-safe and fast. The SDK does not create a telemetry backend.

Linux private-runtime supervision records the owned process group and daemon identity, applies bounded TERM/KILL/reap phases, validates owned paths and symlinks, and never signals an unrecorded group. Windows has separate compile-tested process handling without Unix assumptions. Windows transport remains unsupported in protocol v1.

## Publication boundary

`scripts/sync-jcode-go.sh` is the sole recurring publication path. Its timestamp-free manifest contains:

- ordered include/protect rules;
- source and destination fingerprints;
- exact add/update/remove operations;
- retained destination-only exclusions.

Publishable content is root Go/module/license/README payload plus `protocol/**`, `transport/**`, and `examples/**`. Protected content includes `.git`, `.github`, `.gitignore`, `.autospec`, `.beads`, `.worktrees`, `specs`, `AGENTS*.md`, public `docs/**`, and every unclassified destination path. Removal is possible only inside an include scope.

Preview is read-only. Apply requires an explicit reviewed manifest, destination `main`, correct repository identity, a clean worktree, unchanged fingerprints, and safe unique operations. Reconciliation tests apply only to controlled fixtures.

## Validation boundary

`scripts/validate_go_sdk.sh` is the canonical non-mutating validation entry point. It reports formatting, module consistency, vet, build, tests, race tests, and Windows amd64 build even when one category fails. `.github/workflows/ci.yml` invokes it on Go 1.23.x and 1.24.x so CI and publication evidence cannot drift.
