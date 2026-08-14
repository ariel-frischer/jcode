# Failed-run event identity

## Reproduction identity

- Run: `run-1c2020f67330`
- Preserved outcome: `unsupported_typed_event`
- Preserved runtime: Jcode `v0.75.187-dev`
- Preserved elapsed time: approximately 16m30s
- Evidence capture: 2026-08-13 (the preserved run artifact records the rejected
  identity as omitted; no raw frame is copied here)
- Correlated immutable local runtime revision: `b6848dec8535ea361b67ff78d5ba9e6beeb07d16`
  (`2026-08-13T16:40:28-0700`)

## Event identity

The authoritative protocol path identifies the rejected event as:

- Wire kind: `connection_phase`
- Rust source: `ApiEvent::ConnectionPhase`
- Go decoder outcome before the compatibility fix: `jcode.UnknownEvent`

The wire kind is emitted by the bridge translation in
`crates/jcode-harness-api-server/src/translate.rs` and is part of the
`ApiEvent` contract in protocol revision `b86833ba2e91f422afc83fc0cb3ca0f038456e37`
(`2026-08-10T16:19:16-0700`), an ancestor of the correlated runtime revision.
The canonical Go decoder recognizes the protocol tag but has no concrete
`connection_phase` case, so the former typed stream represents it as
`UnknownEvent`. The Locus adapter's protocol inventory independently names
`connection_phase` as a known Jcode event. Together these records provide the
payload-free protocol evidence used by the deterministic fixtures.

## Safety boundary

Only the run identifier, outcome code, source revision, wire kind, and concrete
type are retained. Raw frames, event fields, prompts, tool arguments, provider
content, credentials, and paths from the run are intentionally not recorded.

## Phase 4 semantic-policy regression evidence

The reviewed Rust publication contract is exhaustive across all `ApiEvent`
variants. Owned-turn variants cover content/progress, advisory/lifecycle,
terminal, permission, and tool-effect classes; request/reply variants remain
outside the owned-turn stream. The canonical Go decoder now maps
`connection_phase` to `*ConnectionPhase`, while legacy unknown-event
provenance remains available through `Session.Events`.

The deterministic P0 matrix passed with these commands:

- `cargo test -p jcode-harness-api -p jcode-harness-api-server --no-fail-fast`
- `go test ./...` (from `sdk/go`)
- `go test -race ./...` (from `sdk/go`)
- `GOOS=windows GOARCH=amd64 go test -c -o /tmp/jcode-go-windows.test.exe .`
- `GOOS=windows GOARCH=amd64 go test -c -o /tmp/jcode-go-protocol-windows.test.exe ./protocol`
- `GOOS=windows GOARCH=amd64 go test -c -o /tmp/jcode-go-transport-windows.test.exe ./transport`

The matrix covers repeated advisory metadata (one bounded warning), bounded
payload-free diagnostics, unknown and malformed fail-closed turn input,
ReasoningDone continuation, ToolExec delivery, terminal outcomes, cancellation
regressions, protocol-v1 unknown preservation, and unchanged wire snapshots.

## Phase 5 classified-or-filtered coverage evidence

The publication gate compares the canonical Rust `ApiEvent` enum with the Go
protocol inventory and the canonical `sdk/go/session.go` decoder/classification
seams. The current inventory is complete:

| Surface | Count | Disposition |
| --- | ---: | --- |
| Rust `ApiEvent` variants | 31 | Every variant has one exhaustive publication-contract arm |
| Owned-turn Rust events | 16 | Every event has one Go representation and one semantic class |
| Outside-owned-turn Rust events | 15 | No owned-turn class or filter metadata |
| Explicit legacy internal filters | 2 | `notification` (unrelated prose) and `swarm_event` (internal coordination), each with rationale |

The Go parity test treats `EventError` as the terminal representation for the
owned `error` event and the turn acceptance path as the advisory representation
for `message_accepted`; all other owned events must have a concrete decoder case
and exactly one `SemanticClassOf` result. A controlled missing typed entry, an
empty semantic class, and an undeclared synthetic inventory entry each fail the
gate and report the sanitized event identity. The Rust negative gate rejects an
owned entry without a class, a filtered entry without a rationale, and duplicate
wire identities. Translator coverage rejects an unreviewed filter kind and
proves representative content, advisory, terminal, permission, and tool-effect
events remain published.

Deterministic phase-5 checks:

- `cargo test -p jcode-harness-api publication --no-fail-fast` (pass)
- `cargo test -p jcode-harness-api exhaustive_inventory_rejects --no-fail-fast` (pass)
- `cargo test -p jcode-harness-api-server explicit_filter_inventory --no-fail-fast` (pass)
- `go test ./protocol -run 'TestOwnedTurnEventCoverage|TestRustSchemaParity|TestProtocolFixtureParity' -count=1` from `sdk/go` (pass)

All negative fixtures use synthetic identities only. They retain no prompt,
tool argument, provider content, credential, path, or other payload value; the
Go compatibility bound remains 256 UTF-8 bytes.

## Phase 5 protocol-v1 corpus evidence

The full affected corpus was rerun after the phase-5 gates were added:

- `cargo test -p jcode-harness-api -p jcode-harness-api-server --no-fail-fast` — 23 Rust harness tests and 71 Rust bridge tests passed.
- `go test ./... -count=2` from `sdk/go` — the canonical SDK, protocol, transport, and example packages passed twice with identical results.
- `go test -race ./... -count=1` from `sdk/go` — all canonical SDK packages passed under the race detector.

The corpus retains the approved protocol-v1 identities and terminal outcomes,
including nonterminal `reasoning_done`, typed `ToolExec`, success/provider
failure/cancellation outcomes, unknown-event preservation on `Session.Events`,
and fail-closed owned-Turn handling. No provider, model, authentication, or
effort routing code or behavior was changed.

## Phase 6 canonical-to-controlled-public projection evidence

The local Go toolchain was `go version go1.26.3-X:nodwarf5 linux/amd64`.
This is supplementary evidence; the supported publication matrix remains Go
1.23.x and 1.24.x in CI.

Canonical validation passed all seven non-mutating categories with:

```text
scripts/validate_go_sdk.sh
```

The deterministic fixture suites also passed:

```text
scripts/test_sync_jcode_go.sh
scripts/test_validate_go_sdk.sh
```

For a temporary clean Git repository initialized on `main` with the approved
`github.com/ariel-frischer/jcode-go` origin, two previews were byte-identical.
The reviewed manifest fingerprint was
`ebba5705a79474a53110b35664ef171c74d6fc4324bc3ecd97af29c5d0ef30c9`, and the
canonical source revision was
`b6848dec8535ea361b67ff78d5ba9e6beeb07d16`. The baseline destination status was
`## main` before apply. Applying that exact manifest to the temporary
projection, then running equivalent validation, passed:

```text
scripts/sync-jcode-go.sh apply --source sdk/go --destination <controlled-projection> --manifest <reviewed-manifest>
scripts/validate_go_sdk.sh --sdk-dir <controlled-projection>
diff -qr sdk/go <controlled-projection> --exclude=.git
```

The controlled projection reported `projection_validation=pass` and
`projection_equivalence=pass`; its source and destination trees were not
rewritten by validation. The live public repository was not inspected or
modified. Public apply, commit, push, `origin/main` confirmation, and any
downstream version update remain an explicit authorization handoff under
`docs/GO_SDK_RELEASE_PLAN.md`.

## Phase 7 focused compatibility validation

Toolchain versions for this run:

- `rustc 1.95.0 (59807616e 2026-04-14)` / `cargo 1.95.0 (f2d3ce0bd 2026-03-21)`.
- `go version go1.26.3-X:nodwarf5 linux/amd64`.

The required focused checks passed before broader validation:

```text
cargo test -p jcode-harness-api schema_snapshot_tests --no-fail-fast
  13 passed, 0 failed
cargo test -p jcode-harness-api-server tests --no-fail-fast
  71 passed, 0 failed
go test ./protocol -run 'TestOwnedTurnEventCoverage|TestRustSchemaParity|TestProtocolFixtureParity' -count=1
  pass
go test . -run 'Test(TypedSessionAndEventSurface|DecodeTypedToolEvents|DecodeTypedEventPreservesUnknownKinds|FailedRunConnectionPhaseFixtureReproducesUnsupportedTypedEvent|TypedEventSemanticClassesAreExplicitAndClosed|ConnectionPhaseDecodesAsConcreteTypedEvent|CompatibilityDiagnosticIsBoundedAndPayloadFree|KnownTypedEventWithNilOrNonObjectFieldsFailsClosed|DecodeTypedToolExecRejectsMalformedFields|TypedEventStreamSurfacesHarnessError)' -count=1
  pass
go test . -run '^TestTurn' -count=1
  pass
```

The focused fixtures assert exactly one `turn_advisory_ignored` warning for
repeated advisory metadata, preserve the expected terminal outcomes for
success, provider failure, cancellation, and protocol failure, and enforce a
maximum 256 UTF-8-byte diagnostic with no payload-derived values. They use
synthetic frames only and require no live provider, credential, paid Locus run,
or shared-daemon mutation.

## Phase 7 SDK, synchronization, and platform validation

The canonical SDK and deterministic publication fixtures passed:

```text
scripts/validate_go_sdk.sh
  Go SDK validation summary: 7 passed, 0 failed
scripts/test_sync_jcode_go.sh
  test_sync_jcode_go: PASS
scripts/test_validate_go_sdk.sh
  test_validate_go_sdk: PASS
```

`validate_go_sdk.sh` passed formatting, module consistency, vet, build, unit
tests, race tests, and the GOOS=windows GOARCH=amd64 compile boundary. The
controlled public-module projection from the preceding phase remains
byte-equivalent to `sdk/go` under the reviewed manifest and passed the same
validation; the live `github.com/ariel-frischer/jcode-go` repository was not
inspected or modified. No provider, model, authentication, or effort routing
behavior was involved.

## Phase 7 isolated runtime validation

The newly built selfdev binary passed the build gate:

```text
cargo build --profile selfdev --bin jcode
  success (0 errors; existing profile-spec warnings only)
resolved executable: the newly built worktree selfdev binary
```

A deterministic no-provider smoke used an empty private `JCODE_HOME`, a private
`JCODE_RUNTIME_DIR`, a unique Unix socket, and no provider/model/authentication
flags. `JCODE_DEBUG_CONTROL=1` was set solely to permit the existing
`debug server:info` probe. The serving process resolved to the same newly built
worktree executable, and the probe returned:

```text
{"debug_control_enabled":true,"git_hash":"b6848dec8","has_update":false,
 "session_count":0,"spawned_swarm_agent_count":0,"swarm_member_count":0,
 "version":"v0.75.3-dev (b6848dec8, dirty)"}
```

Cleanup was completed through the private socket with the deliberate
`server stop --force` acknowledgment; the process then exited and the private
socket was absent. The private home/runtime directory was removed afterward.
The shared daemon and routing configuration were not touched. No live provider,
credential, paid Locus run, or production-adjacent validation was used.

## Phase 7 guardrails and final scope audit

`scripts/check_guardrails.sh` completed with exit status 1. Passing gates were
module resolution, `cargo fmt --all --check`, Cargo.lock freshness, warning
budget, crate dependency boundaries, wildcard re-export ratchet, desktop2
frame budget, and onboarding state-space invariants. The six reported failures
were:

- `cargo check --all-targets --all-features` and `cargo clippy -- -D warnings`
  both fail in the pre-existing `tests/e2e/provider_behavior.rs:156` fixture,
  which omits the unrelated `StreamEvent::Error.provider_code` field.
- The oversized-file and oversized-test ratchets report existing repository
  growth, including the intentionally expanded
  `crates/jcode-harness-api-server/src/translate_tests.rs` fixture.
- Panic-prone and swallowed-error ratchets report unrelated repository growth
  in provider/TUI files.

No ratchet was rebaselined and no unrelated repair was made. The final diff
contains only the approved harness event semantics/tests, canonical SDK and
publication scripts/docs, the feature Autospec artifacts, and `.gitignore`.
An explicit allowlist check reported `unexpected_changed_file=no`. No changed
file is under provider, routing, profile, config, authentication, or model
implementation paths. Added sensitive-pattern matches are documentation words
only; no secret or payload value was added. Public apply/commit/push and
downstream validation remain unauthorized and unclaimed.

### Requirement-to-check trace

| Requirements | Direct check or evidence |
| --- | --- |
| FR-001 | `FailedRunConnectionPhaseFixtureReproducesUnsupportedTypedEvent` and the payload-free event identity above. |
| FR-002 | `CompatibilityDiagnosticIsBoundedAndPayloadFree`, including control/oversized identifiers and the 256-byte assertion. |
| FR-003 | `TestOwnedTurnEventCoverage` plus the exhaustive Rust publication contract. |
| FR-004 | `TestTurn` advisory-repeat fixture, asserting one bounded warning and continued processing. |
| FR-005 | `TestTurn` required-handler fixtures for content, terminal, permission, and tool-effect classes. |
| FR-006 | Typed API/Turn fixtures for unknown, malformed, nil, and unclassified input. |
| FR-007–FR-008 | Rust inventory negative gates and `TestOwnedTurnEventCoverage`/`TestRustSchemaParity`. |
| FR-009–FR-013 | Typed ReasoningDone, ToolExec, terminal/cancellation, unknown-event, and protocol-v1 fixtures in the focused and corpus checks. |
| FR-014 | `scripts/validate_go_sdk.sh`, `scripts/test_sync_jcode_go.sh`, and `scripts/test_validate_go_sdk.sh`. |
| FR-015 | One-way publication docs, reviewed manifest/projection evidence, and the explicit no-public-mutation boundary. |
| FR-016 | Allowlisted final diff and no provider/routing/profile/config/auth/model implementation paths changed. |
| NFR-001 | Diagnostic fixture payload exclusion and 256-byte bound; added-pattern audit above. |
| NFR-002 | Exhaustive Rust/Go inventory parity and negative coverage gates. |
| NFR-003 | Repeated focused/corpus SDK runs and deterministic sync/projection checks. |
| NFR-004 | Protocol-v1 snapshots, unknown preservation, terminal, cancellation, ReasoningDone, and ToolExec corpus checks. |
| SC-001–SC-005 | Respectively FR-001/002 evidence, semantic-policy matrix, inventory gates, legacy corpus, and canonical-to-controlled projection evidence. |

US-001 through US-004 are covered by the same four evidence groups: failed-run
identity/diagnostics, semantic-policy fixtures, inventory parity gates, and
canonical publication projection validation.

## Repair lane after merge review

The synchronization provenance guard was tightened after review identified that
checking only the source repository's `origin` remote allowed renamed or missing
`origin` configurations to bypass the reverse-sync protection. The repaired
script now requires a Git worktree, ignores inherited Git repository selectors,
and checks every fetch and push URL on every configured remote for the protected
public jcode-go repository.

The regression fixture covers Git-less sources, ambient `GIT_DIR`, renamed
public remotes, and public remotes without `origin`. The SDK validation fixture
now hashes every file and relative path in the SDK tree. The focused repair
checks passed:

```text
scripts/test_sync_jcode_go.sh: PASS
scripts/test_validate_go_sdk.sh: PASS
scripts/validate_go_sdk.sh: Go SDK validation summary: 7 passed, 0 failed
```
