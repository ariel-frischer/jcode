# Validation evidence

## Root cause and TDD

- Red: `cargo test -p jcode-app-core --lib handoff_reminder -- --nocapture`
  failed because `HANDOFF_GOAL_42N` occurred zero times in the provider-visible
  request when an explicit system prompt override was active.
- Green: the same focused test passes after appending the current-turn reminder
  before returning the override split.

## Focused and changed-surface checks

- `cargo fmt --all -- --check`: pass.
- `cargo test -p jcode-app-core --lib handoff`: 13 passed.
- `cargo test -p jcode-tui --lib session_handoff`: 6 passed.
- `cargo clippy -p jcode-app-core --lib --tests -- -D warnings`: pass. Cargo
  emitted existing unmatched profile-package warnings, but no app-core lint
  errors.
- `cargo test -p jcode-app-core --lib`: 1207 passed, 12 failed, 7 ignored.
  The failures are outside the changed behavior and include existing
  environment/config-sensitive profile overlay, swarm timing, debug reload,
  tool-description budget, and todo-schema assertions.

## Repository guardrails

`scripts/check_guardrails.sh` was run. Format, module declarations, Cargo.lock,
dependency boundaries, wildcard exports, desktop frame budget, and onboarding
invariants passed. Seven gates failed on unrelated current-base debt:

- root `jcode` tests missing the recently added `file_mentions_ignore` field,
- unrelated `jcode-tui` Clippy failures,
- warning budget,
- oversized file and test ratchets,
- panic-prone usage ratchet,
- swallowed-error usage ratchet.

No guardrail failure implicated either changed app-core file.

## Built-binary runtime

- Built from this worktree with
  `scripts/dev_cargo.sh build --profile selfdev -p jcode --bin jcode`.
- Resolved executable:
  `/home/ari/repos/jcode/.worktrees/agent/jcode-42n/target/selfdev/jcode`.
- Version: `jcode v0.78.446-dev (3c62e1d3f, dirty)`.
- Ran the built binary through an isolated socket and a deterministic local
  OpenAI-compatible fake provider, with no Luna or external API call.
- The source provider response called `session_transition`; the destination
  provider request contained `HANDOFF_RUNTIME_MARKER_42N` exactly once and the
  destination response was exactly `HANDOFF_RUNTIME_MARKER_42N_ACK`.
- Request log summary: 2 provider requests, marker counts `[0, 1]`.
