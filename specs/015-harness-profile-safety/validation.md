# Validation: harness named profiles and run safety

Date: 2026-08-23
Branch: `agent/jcode-8on`
Bead: `jcode-8on`

## Acceptance matrix

| Requirement | Evidence | Result |
|---|---|---|
| Valid named profile resolves to daemon startup object | `named_profile_string_resolves_to_typed_startup` and public Go SDK built-binary smoke | Pass |
| Unknown profile is typed, actionable, and connection remains usable | `unknown_named_profile_returns_typed_error_without_pending_attach`; live smoke printed `unknown_profile=typed_recoverable` before successfully creating the valid session | Pass |
| Object profile and omission remain compatible | harness schema snapshots and existing object-profile bridge fixture | Pass |
| `max_turns`, `token_budget`, and explicit-offset deadline reach daemon run safety | bridge forwarding test, protocol round-trip test, daemon preflight test | Pass |
| Invalid safety fails before provider work | bridge wrong-type test plus daemon preflight cases for invalid counters/deadline | Pass |
| Omitted safety stays legacy and does not leak | schema, bridge, protocol, and controller-clear coverage | Pass |
| Public SDK and built binaries work end to end | disposable free-model smoke through `github.com/ariel-frischer/jcode-go` checkout matching released v0.1.6 API | Pass |

## Commands and receipts

- `./scripts/dev_cargo.sh test -p jcode-harness-api-server`
  - Passed all library tests. Bin target's zero-test result was explicitly allowed with `JCODE_DEV_CARGO_ALLOW_ZERO_TESTS=1`.
- `./scripts/dev_cargo.sh test -p jcode-harness-api`
  - Passed, including numeric safety wire shape and legacy omission.
- `./scripts/dev_cargo.sh test -p jcode-protocol`
  - Passed 86 tests plus doc tests.
- `./scripts/dev_cargo.sh test -p jcode-app-core harness_message_run_safety_preflight_validates_each_control_and_sources -- --nocapture`
  - Passed. Covers max turns, token budget, deadline, combined controls, invalid input, and invocation source.
- `./scripts/dev_cargo.sh check --all-targets --all-features`
  - Passed after repairing two stale E2E fixture constructors required by current protocol/message fields.
- `npm --prefix sdk/typescript ci --no-audit --no-fund --silent`
- `npm --prefix sdk/typescript run build --silent`
- `npm --prefix sdk/typescript test --silent`
  - Passed 40 tests.
- `cargo fmt --all`
  - Passed; subsequent guardrail format check also passed.
- `scripts/check_guardrails.sh`
  - The script resolved the integration checkout instead of this worktree and reported repository-wide ratchet drift unrelated to this branch. Its all-target failures were reproduced and repaired in this worktree, then the direct all-target check passed. The remaining global size/panic/swallowed-error ratchets compare the current integration tree against stale repository budgets; this change introduces no new panic-prone or swallowed-error pattern in its diff.
- Autospec artifact validation:
  - `spec.yaml`, `plan.yaml`, and `tasks.yaml` passed `autospec artifact` validation.

## Built-binary public API smoke

Executables:

- `/home/ari/repos/jcode/.worktrees/agent/jcode-8on/target/selfdev/jcode`
- `/home/ari/repos/jcode/.worktrees/agent/jcode-8on/target/selfdev/jcode-harness-api-bridge`

Isolation:

- Private SDK-owned API and daemon sockets under `/home/ari/.jcode/scratch/jcode-8on-smoke/home/run/`
- Disposable `JCODE_HOME` under `/home/ari/.jcode/scratch/jcode-8on-smoke/home`
- Named profile `free-smoke` selected OpenRouter `stealth/ox-alpha` with no tools and a non-secret response marker.
- No credentials or credential values were printed.

Observed output:

```text
unknown_profile=typed_recoverable
named_profile=session_created
max_turns=accepted_value_1
profile_marker=observed
budgeted_turn=completed
```

The smoke initially exposed that released jcode-go v0.1.6 serializes safety counters as JSON integers. The public Rust/TypeScript schema and bridge were corrected to accept integers and convert them to the daemon's canonical raw decimal strings before the successful rerun.

## Risk

**Medium.** This changes a cross-process public harness contract and per-turn provider safety installation. Blast radius is limited to harness `create_session` profile selection and `send_message` requests that opt into safety fields. Omitted fields retain legacy behavior. Rollback is the focused Bead commit. Review should verify public numeric counter compatibility, profile credential redaction, and controller clearing at turn completion.
