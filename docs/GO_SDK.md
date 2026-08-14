# Go SDK

**Status:** Current implementation and publication boundary

Jcode's Go SDK is the dependency-free module in [`sdk/go/`](../sdk/go):

```text
module github.com/ariel-frischer/jcode-go
```

`sdk/go` is the canonical source. [`github.com/ariel-frischer/jcode-go`](https://github.com/ariel-frischer/jcode-go) is the public destination. Publication copies only declared SDK payload; public-repository governance, specifications, automation, worktree/task state, and maintainer docs remain destination-owned.

## Runtime contract

The SDK is an external protocol-v1 client. It uses the local harness API over newline-delimited JSON; it is not Rust FFI, an ACP client, or a CLI-output parser.

- `Connect` attaches non-owningly to an existing bridge and never stops the shared daemon.
- `Launch`/`LaunchInstance` own an isolated private runtime, readiness, and bounded cleanup.
- `Session.Send` and `Session.Events` remain source-compatible.
- `Session.StartTurn` owns acceptance, ordered events, cancellation, and an immutable typed terminal result.
- Typed streams expose ReasoningDone, ToolExec, terminal events, and an unknown-event compatibility seam.
- Observations are bounded classifications and exclude prompts, credentials, raw frames, private paths, session IDs, and provider content.
- Linux/macOS use Unix sockets. Windows is a package compile boundary; no production Windows transport is claimed.

See [`sdk/go/README.md`](../sdk/go/README.md) for API examples and lifecycle/error details.

## Typed-event semantics

`EventSemanticClass` is closed and explicit. The canonical event inventory
assigns every owned-turn event exactly one class:

| Class | Owned `Turn.Next` policy |
| --- | --- |
| `EventSemanticClassContentProgress` | Publish to the handler. |
| `EventSemanticClassAdvisoryLifecycle` | May be ignored; the first ignored event emits one bounded `Observation` with sanitized `EventKind`, `EventType`, and `Disposition`. |
| `EventSemanticClassTerminal` | Publish and wait for the immutable terminal result. |
| `EventSemanticClassPermission` | Publish and apply an explicit user/application policy. |
| `EventSemanticClassToolEffect` | Publish and handle the tool effect; never treat it as metadata. |

Use `SemanticClassOf` when adapting a `TypedEvent` to an exhaustive handler.
An owned turn never infers a class from an event name or payload. Unknown,
malformed, nil, and semantically unclassified input fails closed as a
`TurnResultProtocolError` whose `CompatibilityError` contains only sanitized
kind/type identifiers and is at most 256 UTF-8 bytes. No prompt, tool argument,
provider content, credential, path, or raw event fields are formatted into the
diagnostic.

The legacy `Session.Events` stream remains compatible: known kinds decode to
their concrete typed values, while an additive unknown kind is returned as
`UnknownEvent` with its kind and fields preserved for callers that explicitly
own forward-compatibility handling. That preservation does not authorize an
owned `Turn` to continue past an unknown or unclassified event.

## Use from source

In another module:

```text
require github.com/ariel-frischer/jcode-go v0.0.0
replace github.com/ariel-frischer/jcode-go => /absolute/path/to/jcode/sdk/go
```

Then import `github.com/ariel-frischer/jcode-go`. Remove the local replacement when using an approved published version.

## Validation

From the Jcode repository root:

```bash
scripts/validate_go_sdk.sh
```

The command is non-mutating and always reports seven categories:

1. formatting;
2. module consistency (`go mod tidy -diff`, then `go mod verify`);
3. vet;
4. build;
5. tests;
6. race tests;
7. Windows amd64 build.

CI runs the same command for Go 1.23.x and 1.24.x. A newer local toolchain is useful supplementary evidence, not a substitute for the supported matrix. Tests and builds require no OAuth, provider credentials, shared daemon, or paid model call.

## Publication preview

Capture the public baseline first, then generate two previews:

```bash
scripts/sync-jcode-go.sh preview \
  --source sdk/go \
  --destination /absolute/path/to/jcode-go > /tmp/jcode-go.manifest
```

Preview is the default, writes its deterministic manifest to stdout, and mutates neither tree. Review the named inclusion/protection rules, fingerprints, exact add/update/remove operations, and retained exclusions. Apply requires the exact reviewed manifest and unchanged inputs:

```bash
scripts/sync-jcode-go.sh apply \
  --source sdk/go \
  --destination /absolute/path/to/jcode-go \
  --manifest /tmp/jcode-go.manifest
```

Apply is a publication action and requires explicit authorization. Reconciliation work must exercise it only on controlled temporary repositories, never on live public `main`. See [`GO_SDK_RELEASE_PLAN.md`](GO_SDK_RELEASE_PLAN.md).

## Examples

- `sdk/go/examples/oneshot`: owned Turn lifecycle and typed terminal result.
- `sdk/go/examples/streaming`: long-lived typed event streaming, including ToolExec and ReasoningDone.
- `sdk/go/examples/private`: isolated launch, detached ownership, and bounded shutdown.

All examples build in the canonical validator. Runtime execution requires the compatible bridge or private Jcode binary described by the example.
