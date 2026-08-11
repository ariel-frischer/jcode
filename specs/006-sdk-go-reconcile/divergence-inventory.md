# Go SDK Reconciliation Inventory

Status: active migration inventory (2026-08-11)

## Protected public repository baseline (T001)

The public repository is a read-only boundary for this reconciliation.

- Resolved path: `/home/ari/repos/jcode-go`
- Branch: `main` (tracking `origin/main`)
- HEAD: `34fd95cfe08cf9b8de8bb2e0279d367d92d853b6`
- Worktree: clean
- Tracked paths: 48
- Tracked path/mode/blob fingerprint (SHA-256 over NUL-delimited `git ls-files -s`): `5ea31440dc9a17f04645aa4b6ac320ff251d4271cb7e06140cbffcccf76fafb7`

Every tracked path had mode `100644` at baseline:

```text
.autospec/constitution.yaml
.gitignore
AGENTS.md
LICENSE
README.md
client.go
client_api_test.go
client_test.go
client_validation_test.go
connect.go
docs/architecture.md
docs/private-runtime-acceptance.md
docs/sdk-stability-plan.md
examples/README.md
examples/oneshot/main.go
examples/oneshot/main_test.go
examples/private/main.go
examples/streaming/main.go
go.mod
launch.go
launch_test.go
observability_test.go
ownership_test.go
private_runtime_acceptance_linux_test.go
process_unix.go
process_unix_test.go
process_windows.go
process_windows_test.go
protocol/framing.go
protocol/parity_test.go
protocol/protocol.go
protocol/protocol_test.go
session.go
specs/070-typed-terminal-outcomes/plan.yaml
specs/070-typed-terminal-outcomes/spec.yaml
specs/070-typed-terminal-outcomes/tasks.yaml
specs/071-add-tool-exec/checklists/api.yaml
specs/071-add-tool-exec/plan.yaml
specs/071-add-tool-exec/spec.yaml
specs/071-add-tool-exec/tasks.yaml
transport/socket_unix.go
transport/socket_windows.go
transport/transport.go
transport/transport_test.go
turn.go
turn_outcome_test.go
turn_test.go
typed_api_test.go
```

Repeat the baseline with read-only commands:

```bash
realpath /home/ari/repos/jcode-go
git -C /home/ari/repos/jcode-go branch --show-current
git -C /home/ari/repos/jcode-go rev-parse HEAD
git -C /home/ari/repos/jcode-go status --porcelain=v1 --branch
git -C /home/ari/repos/jcode-go ls-files -s
git -C /home/ari/repos/jcode-go ls-files -s -z | sha256sum
```

No command in this baseline switches branches, stages, commits, or writes public repository content.

## Authority and synchronization taxonomy (T002)

### Content categories

- `source`: non-test Go implementation under the module root, `protocol/`, or `transport/`.
- `test`: Go test and acceptance-fixture code.
- `example`: source, tests, and usage guidance below `examples/`.
- `module`: `go.mod` and the module license.
- `documentation`: consumer or maintainer Markdown outside generated specifications.
- `governance`: repository instructions, Autospec/Beads state, specifications, worktrees, and repository-local ignore/configuration files.

### Authorities

- `canonical`: `sdk/go` owns SDK publication content and the recurring publication workflow.
- `wire`: `crates/jcode-harness-api` owns protocol-v1 serialized event names, field shapes, and compatibility limits.
- `public`: established public Go lifecycle, observability, private-runtime, terminal-outcome, process-safety, ToolExec, and ReasoningDone behavior is authoritative within the wire contract.
- `shared`: compatible content is already equivalent or must be merged without regressing either canonical or established public behavior.
- `repository-specific`: content exists to operate or govern one repository and is not SDK payload.

### Dispositions

- `import`: bring authoritative public behavior into `sdk/go` as a one-time reconciliation action.
- `retain-canonical`: preserve compatible canonical behavior where the public tree does not supersede it.
- `merge`: combine compatible behavior or guidance, with the wire owner resolving serialized-contract questions.
- `protect-public`: retain destination-only repository content and never emit an add, update, or remove operation for it.
- `remove-obsolete`: allow removal only inside an explicitly publishable payload scope and only when a reviewed manifest records the exact path.

No established public behavior may be rejected without concrete current architecture evidence and an explicit compatibility-impact record. CLI output is never wire evidence.

### Named recurring rules

| Rule | Treatment | Path scope | Purpose |
|---|---|---|---|
| `include-root-sdk` | include | `*.go`, `go.mod`, `go.sum` when present, `LICENSE`, `README.md` | Publish root SDK source, tests, module metadata, license, and consumer README. |
| `include-protocol` | include | `protocol/**` | Publish the protocol-v1 client implementation and compatibility tests, constrained by the harness wire contract. |
| `include-transport` | include | `transport/**` | Publish dependency-free SDK transports and tests. |
| `include-examples` | include | `examples/**` | Publish buildable SDK examples, their tests, and example usage guidance. |
| `protect-governance` | protect | `.autospec/**`, `.beads/**`, `.worktrees/**`, `specs/**`, `AGENTS.md`, `AGENTS.override.md` | Preserve destination planning, task memory, worktrees, specifications, and agent governance. |
| `protect-repository-config` | protect | `.git/**`, `.gitignore`, `.github/**` | Preserve destination identity, ignore policy, and repository automation. |
| `protect-public-docs` | protect | `docs/**`, all root Markdown except publishable `README.md` | Preserve public-repository architecture, acceptance, and stability history; canonical operator docs remain in Jcode `docs/`. |
| `protect-unknown` | protect | every destination path not matched by an include rule | Fail closed: an unclassified destination path is retained and reported, never implicitly deleted. |

`sdk/go` is the sole publication source. The public repository is a protected destination, not a second canonical implementation. Recurring removals are legal only for paths selected by an `include-*` rule; every other destination path is retained through a named `protect-*` rule.

## Initial tracked-path divergence ledger (T003)

Scope is the union of Git-tracked paths in canonical `sdk/go` (with that prefix removed) and public `jcode-go` at the T001 baseline. Ignored runtime state such as `.git`, `.beads`, and `.worktrees` is covered by protection rules but is not treated as publication content. Blob equality and mode equality establish non-divergence; all paths on both sides use mode `100644`.

Evidence keys:

- `WIRE`: `crates/jcode-harness-api/src/events.rs` owns snake-case protocol-v1 events, including `reasoning_done`, `tool_exec`, `turn_done`, errors, and the `Unknown` catch-all.
- `TURN`: public `turn_test.go` and `turn_outcome_test.go` define owned-turn ordering, cancellation, immutable terminal outcomes, disconnect/bridge failure, redaction, and legacy Session compatibility.
- `OBS`: public `observability_test.go` defines bounded/redacted connect, turn, cancellation, and launch observations.
- `RUNTIME`: public `launch_test.go`, `private_runtime_acceptance_linux_test.go`, `process_unix_test.go`, and `process_windows_test.go` define launch, readiness, bounded shutdown, path/PID safety, reaping, and platform behavior.
- `API`: public `typed_api_test.go` and `protocol/protocol_test.go` define exported typed decoding, ToolExec, unknown-event fallback, and stable wire fields.
- `EXAMPLE`: public example tests/builds exercise the public lifecycle without requiring live credentials.

| Path | Category | Authority | Disposition | Rationale and evidence |
|---|---|---|---|---|
| `.autospec/constitution.yaml` | governance | repository-specific | protect-public | Public governance is not SDK payload (`protect-governance`). |
| `.gitignore` | governance | repository-specific | protect-public | Destination ignore policy remains local (`protect-repository-config`). |
| `AGENTS.md` | governance | repository-specific | protect-public | Destination agent instructions remain local (`protect-governance`). |
| `docs/architecture.md` | documentation | repository-specific | protect-public | Public architectural history remains destination-only; current canonical ownership is documented in Jcode docs. |
| `docs/private-runtime-acceptance.md` | documentation | repository-specific | protect-public | Public acceptance runbook remains destination-only; behavior is imported through `RUNTIME`. |
| `docs/sdk-stability-plan.md` | documentation | repository-specific | protect-public | Public stability history is not publication payload (`protect-public-docs`). |
| `specs/070-typed-terminal-outcomes/plan.yaml` | governance | repository-specific | protect-public | Public planning history remains destination-only. |
| `specs/070-typed-terminal-outcomes/spec.yaml` | governance | repository-specific | protect-public | Public specification remains destination-only. |
| `specs/070-typed-terminal-outcomes/tasks.yaml` | governance | repository-specific | protect-public | Public task history remains destination-only. |
| `specs/071-add-tool-exec/checklists/api.yaml` | governance | repository-specific | protect-public | Public checklist remains destination-only. |
| `specs/071-add-tool-exec/plan.yaml` | governance | repository-specific | protect-public | Public planning history remains destination-only. |
| `specs/071-add-tool-exec/spec.yaml` | governance | repository-specific | protect-public | Public specification remains destination-only. |
| `specs/071-add-tool-exec/tasks.yaml` | governance | repository-specific | protect-public | Public task history remains destination-only. |
| `README.md` | documentation | public + canonical | merge | Import current public Session/Turn/runtime guidance, then align publication commands with canonical scripts. |
| `client.go` | source | public within `WIRE` | import | Adds ordered owned-turn dispatch, bridge-exit handling, and bounded observations required by `TURN`/`OBS`, while retaining existing Session APIs. |
| `examples/README.md` | example | public | import | Current example selection and lifecycle guidance is established consumer behavior (`EXAMPLE`). |
| `examples/oneshot/main.go` | example | public | import | Uses the authoritative owned Turn lifecycle and typed terminal result (`TURN`, `EXAMPLE`). |
| `examples/oneshot/main_test.go` | example | public | import | Adds controlled failure/redaction coverage with no live provider (`EXAMPLE`). |
| `examples/private/main.go` | example | public | import | Uses current private-runtime readiness and bounded cleanup (`RUNTIME`, `EXAMPLE`). |
| `examples/streaming/main.go` | example | public | import | Demonstrates typed streaming, ToolExec, ReasoningDone, and terminal behavior constrained by `WIRE`. |
| `launch.go` | source | public | import | Adds private-runtime lifecycle, bounded diagnostics, ownership, and cleanup behavior proven by `RUNTIME`. |
| `launch_test.go` | test | public | import | Adds readiness, exit fan-out, environment precedence, compatibility credential, and redaction tests (`RUNTIME`). |
| `observability_test.go` | test | public | import | Defines the bounded/redacted observation contract (`OBS`). |
| `ownership_test.go` | test | public + canonical | merge | Import broader ownership/cleanup coverage without removing compatible canonical assertions (`RUNTIME`). |
| `private_runtime_acceptance_linux_test.go` | test | public | import | Adds controlled Linux lifecycle acceptance; the optional real-OAuth smoke remains opt-in and excluded from required validation (`RUNTIME`). |
| `process_unix.go` | source | public | import | Adds bounded process-group supervision, reaping, and owned-path/PID safety (`RUNTIME`). |
| `process_unix_test.go` | test | public | import | Defines Unix termination, reaping, cleanup, symlink, PID, concurrency, and external-instance compatibility (`RUNTIME`). |
| `process_windows.go` | source | public | import | Adds bounded Windows cleanup without Unix assumptions (`RUNTIME`). |
| `process_windows_test.go` | test | public | import | Defines Windows taskkill, timeout, error, and successful termination seams (`RUNTIME`). |
| `protocol/protocol_test.go` | test | wire + public | import | Adds exact `tool_exec` raw-field coverage checked against `ApiEvent::ToolExec`; existing unknown fallback remains (`WIRE`, `API`). |
| `session.go` | source | wire + public | merge | Adds StartTurn and typed ToolExec while retaining existing Session and ReasoningDone behavior; names/fields match `WIRE`. |
| `turn.go` | source | public within `WIRE` | import | Implements the established owned-turn and typed terminal-outcome contract (`TURN`). |
| `turn_outcome_test.go` | test | public | import | Defines stable terminal kinds, first-result immutability, sanitized failures, bridge exit, and detach races (`TURN`). |
| `turn_test.go` | test | public | import | Defines lifecycle ordering, cancellation, overflow, context, provider-error, and Session compatibility (`TURN`). |
| `typed_api_test.go` | test | wire + public | merge | Imports ToolExec, terminal, runtime-control, and public-alias coverage while retaining existing ReasoningDone and unknown-event compatibility (`WIRE`, `API`). |

The remaining 13 tracked SDK paths are byte-identical and retain `shared` authority with `retain-canonical` disposition: `LICENSE`, `client_api_test.go`, `client_test.go`, `client_validation_test.go`, `connect.go`, `go.mod`, `protocol/framing.go`, `protocol/parity_test.go`, `protocol/protocol.go`, and all four `transport/**` files. In particular, ReasoningDone already matches the harness event name and optional `duration_secs` wire shape; ToolExec is the only missing typed event in the divergent protocol/session surface. No authority exception rejects established public behavior.

## Publication-readiness validation (T035)

Documentation, scripts, fixtures, and CI agree on `sdk/go` as canonical source, `github.com/ariel-frischer/jcode-go` as protected destination, the four include rules, four protect rules, reviewed-manifest apply, and seven validation categories. Every referenced script, module path, example, and public lifecycle symbol exists.

Validation evidence on 2026-08-11:

| Command | Result | Evidence class |
|---|---|---|
| `bash scripts/test_sync_jcode_go.sh` | PASS | Deterministic preview, protected retention, invalid/stale refusal, exact controlled-copy apply, file modes, and no live repository mutation. |
| `bash scripts/test_validate_go_sdk.sh` | PASS | All seven command shims, each injected failure, later-category continuation, aggregate failure, and no SDK mutation. |
| `GOTOOLCHAIN=go1.23.12 scripts/validate_go_sdk.sh` | PASS (7/7) | Supported Go 1.23.x matrix evidence. |
| `GOTOOLCHAIN=go1.24.6 scripts/validate_go_sdk.sh` | PASS (7/7) | Supported Go 1.24.x matrix evidence. |
| `scripts/validate_go_sdk.sh` with installed Go 1.26.3-X:nodwarf5 | PASS (7/7) | Supplementary newer-toolchain evidence only. |

Each validator run reported formatting, module consistency (`go mod tidy -diff` and `go mod verify`), vet, build, tests, race tests, and Windows amd64 build. No required check used OAuth, provider credentials, a live daemon, production infrastructure, or paid model access.

## Final reconciliation and delivery evidence (T037)

Two final live previews were byte-for-byte identical and proposed exactly one classified publishable documentation update (`README.md`). They retained the 13 public repository-specific paths through named protection rules and reported no source, test, example, module, or behavioral operation. Preview changed neither repository.

The protected public repository remained exactly at the T001 baseline:

- branch `main`, tracking `origin/main`;
- HEAD `34fd95cfe08cf9b8de8bb2e0279d367d92d853b6`;
- clean worktree and 48 tracked paths, all mode `100644`;
- tracked path/mode/blob fingerprint `5ea31440dc9a17f04645aa4b6ac320ff251d4271cb7e06140cbffcccf76fafb7`.

### Requirement traceability

| Requirement | Final evidence |
|---|---|
| FR-001 | Generated tracked-path/hash comparison and the ledger are set-equal for all 35 initial divergences. |
| FR-002 | Every ledger row records category, authority, disposition, rationale, and evidence/rule. |
| FR-003 | Imported public lifecycle, observability, private-runtime, terminal, protocol-v1, process, ToolExec, and ReasoningDone tests pass under both supported Go lines. |
| FR-004 | Working-tree hash comparison and final preview contain no public-only authoritative code, test, or example. |
| FR-005 | Final preview retains all public governance, specifications, and maintainer docs through protect rules. |
| FR-006 | Manifest and docs expose four include and four fail-closed protect rules; fixture coverage exercises each class. |
| FR-007 | Preview reports exact ordered operations/retentions; repeated public branch/status/fingerprint checks prove no mutation. |
| FR-008 | Fixture apply consumes the reviewed manifest exactly, preserves protected paths, and refuses implicit/default writes. |
| FR-009 | Fixture and two live preview pairs are byte-for-byte identical with no timestamp field. |
| FR-010 | `scripts/validate_go_sdk.sh` reports and aggregates all seven required boundaries. |
| FR-011 | Validator fixtures inject each category failure, show it, continue through Windows, and exit non-zero. |
| FR-012 | SDK README, overview, architecture, release runbook, scripts, fixtures, and CI agree on source/destination/workflow/verification. |
| FR-013 | Final public branch, HEAD, status, tracked count/modes, and fingerprint equal T001. |
| FR-014 | Final preview contains only classified protected retentions plus the classified publishable README update. |
| FR-015 | Full and race suites pass with the reconciled public tests; Windows build passes; legacy Session and unknown-event tests require no consumer changes. |

### Repository gates

- `git diff --check`: PASS.
- `chlog check`: PASS.
- `bash -n` for all four changed shell scripts: PASS.
- Sync and validator fixture suites: PASS.
- Go 1.23.12 and 1.24.6 canonical validator: PASS (7/7 each); installed Go 1.26.3 result is supplementary PASS.
- `scripts/check_guardrails.sh`: formatting, lockfile, warning, dependency-boundary, wildcard re-export, desktop frame, and onboarding gates passed. Six unrelated baseline gates failed: Rust all-target check/clippy (existing `SessionProfileConfig.handoff` initializers and `run_structured_sdk_turn` argument lint) plus existing code-size, test-size, panic, and swallowed-error ratchet growth. `git diff --quiet HEAD` proves all reported Rust/Cargo paths are unchanged by this SDK work; no ratchet was rebaselined and scope was not expanded, per the constitution's proven-pre-existing-failure exception.

The final worktree diff is limited to the approved SDK implementation/tests/examples, sync and validation scripts/fixtures, CI, SDK documentation, and Autospec artifacts. `/home/ari/repos/jcode-go` has no changed file before integration.
