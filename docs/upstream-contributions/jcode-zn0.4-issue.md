# Issue draft: Named session profiles for headless `jcode run`

## Suggested title

Add named session profiles for repeatable headless `jcode run` configuration

## Summary

Add user-defined named profiles under `[profiles.<name>]` in `~/.jcode/config.toml` or `$JCODE_HOME/config.toml`, selected with the global `--profile <name>` option for headless `jcode run` invocations.

A profile composes provider, model, reasoning effort, provider profile, tool policy, selected skills, and additive instructions into one immutable per-run configuration. Plain, JSON, and NDJSON output modes use the same resolved profile path.

This implements the headless portion of #721 without introducing profile management commands, daemon protocol fields, interactive-session persistence, child-session inheritance, ACP changes, or SDK/schema changes.

## Motivation

Headless callers currently repeat the same provider, model, reasoning, tool, skill, and prompt options across scripts and automation. Named profiles provide a stable local configuration contract while preserving explicit invocation overrides and existing no-profile behavior.

Typical uses include:

- repeatable review or research modes
- wrapper scripts that switch between tool policies
- deterministic provider/model/reasoning combinations
- additive task instructions and selected skill prompts
- consistent plain, JSON, and NDJSON automation

## Proposed configuration

```toml
[profiles.review]
provider = "openrouter"
model = "openai/gpt-5.6"
reasoning_effort = "high"
provider_profile = "review-gateway"
tool_profile = "minimal"
tools = ["read", "agentgrep"]
disabled_tools = ["bash"]
skills = ["pr-reviewer"]
instructions = "Review correctness, security, and regression risk."
```

Usage:

```bash
jcode --profile review run "Review this repository"
jcode --profile review run --json "Review this repository"
jcode --profile review run --ndjson "Review this repository"
```

Global options can appear before or after `run`.

## Resolution contract

Each supported field resolves independently with this precedence:

1. explicit invocation option
2. environment override
3. selected session profile
4. unprofiled base configuration
5. built-in default

An explicit `--provider auto` is distinct from clap's omitted default and therefore remains a real invocation override.

The omitted-profile path returns before strict profile loading, profile lookup, tool composition, skill discovery, prompt overlay construction, or profile-specific provider changes. Existing no-profile `jcode run` behavior is preserved.

## Validation and diagnostics

- Profile names use exact persisted keys.
- Empty names and names with leading or trailing whitespace are rejected.
- Present scalar fields must be non-empty.
- Reasoning and tool-profile enum-like values are validated structurally.
- Provider, model, named provider-profile, installed tool, and installed skill availability are validated only for the selected profile.
- Unknown profiles list configured names without exposing profile contents.
- User-authored instructions are not reproduced in validation diagnostics.
- Tool references use the same canonical alias table as the existing tool configuration, so aliases such as `grep` and `shell_exec` resolve to `agentgrep` and `bash`.
- Invalid selections fail before an agent sends a provider request.

## Prompt and tool isolation

The resolved tool selection and prompt overlay are stored on the one-shot `Agent` instance. They do not mutate global tool or skill state and do not rewrite `config.toml`.

Prompt ordering is:

1. existing global and project system guidance
2. profile instructions
3. profile-selected skill prompts
4. existing turn-specific system reminder
5. user message

Two runs in the same process can therefore select different profiles without profile state leaking between agents.

## Scope

Included:

- `[profiles.<name>]` serde/config contract
- strict static profile validation and exact lookup
- global `--profile` and `--reasoning-effort` parsing
- provider/model/reasoning/provider-profile precedence
- existing `ToolConfig` composition
- selected skill prompt rendering
- additive per-Agent instructions
- plain, JSON, and NDJSON headless run integration
- documentation, examples, changelog entry, tests, and startup budgets

## Non-goals

Not included:

- TUI or interactive startup integration
- daemon/protocol subscription fields
- session snapshot, resume, or persisted profile identity
- child, swarm, or headless child-session inheritance
- profile list/show/current/resolve management commands
- ACP changes
- SDK or harness schema changes
- credentials or secrets inside session profiles
- config-file mutation

## Implementation outline

- `SessionProfileConfig` owns the dependency-light persisted serde contract.
- `jcode-base::config::session_profile` owns static validation, exact lookup, source precedence primitives, tool overlay semantics, and canonical tool-reference normalization.
- `src/cli/profile.rs` owns selected-run environment and invocation composition.
- `dispatch::run_main` resolves profiles only for `Command::Run`.
- `run_single_message_command` applies the resolved tool selection, reasoning effort, and prompt overlay to a one-shot `Agent`.
- `SessionPromptOverlay` appends profile instructions and selected skill prompts without mutating shared state.

## Validation evidence

Focused checks passed on an isolated branch based on upstream commit `f28f02f8d3fb004494b4a6886617bae9ee8b216b`:

- `jcode-config-types` profile serde tests: 2 passed
- `jcode-base` profile/config tests: 23 focused tests passed across the recorded suites
- CLI profile tests, including canonical tool aliases: 6 passed
- CLI global argument parsing test: passed
- profile resolution, precedence, environment precedence, prompt isolation, and invalid-reference focused tests: passed
- affected-package `cargo check`: passed
- selfdev `jcode` binary build: passed
- built-binary fake-provider smoke on a private socket:
  - plain profile run returned the deterministic response
  - JSON profile run returned the deterministic response
  - NDJSON profile run returned the deterministic response
  - canonical tool aliases were accepted
  - unknown and structurally invalid profiles produced no provider request
  - no-profile compatibility passed
- profile resolution and startup efficiency budgets passed; candidate startup median was lower than the recorded baseline in the implementation run

### Final whole-result acceptance matrix

The complete matrix was rerun over final branch commit `414da3a2bb0ec72c2eea2dd5d6895248ee6fd6d6`, not only over individual repair commits:

- all Autospec artifacts remained schema-valid; analysis remained PASS with 0 findings, 23/23 requirements, and all 38 tasks complete
- profile serde, strict loading, validation, canonical aliases, prompt ordering, CLI placement, explicit precedence, environment precedence, Agent-local isolation, and invalid-reference suites passed
- affected-package `cargo check` and the selfdev binary build passed
- public help exposed both global options after clearing ambient profile/reasoning variables
- the corrected built-runtime matrix passed with five captured provider requests across plain, JSON, and NDJSON runs, global-option placement before and after `run`, canonical aliases, selected instructions and skills, immutable config, no-profile compatibility, and negative cases that made no provider request
- documentation, changelog, forbidden-surface, and credential-shaped-diff checks passed

Requirement-to-check traceability covered every frozen requirement:

- FR-001–FR-005: serde/base resolver suites plus captured runtime model, tools, instructions, and skill prompt
- FR-006–FR-009: global parser, exact-profile lookup, invocation precedence, environment precedence, and negative no-request checks
- FR-010–FR-013: prompt ordering, per-Agent isolation, immutable config hash, alias acceptance, and actionable invalid tool/skill/reasoning/profile diagnostics
- FR-014–FR-017: no-profile runtime, unchanged empty-overlay prompt path, docs/changelog checks, all three output modes, and forbidden-surface scan
- NFR-001–NFR-006 and SC-001–SC-005: omitted-profile budgets, redacted diagnostics, strict per-run loading on a private socket, existing ownership seams, complete-profile reproduction, CLI authority, pre-request rejection, and cross-run/config isolation

The repository guardrail helper was also executed. Its process resolved the main custom checkout (`/home/ari/repos/jcode`) rather than this isolated upstream worktree, so its seven failures are recorded as non-branch evidence: existing custom-`dev` all-target test initializers, Clippy/warning drift, and oversized-file/test, panic, and swallowed-error ratchets. Formatting and static packaging checks for this contribution passed, and the focused branch-owned compilation, build, and runtime matrix passed. The guardrail result is not represented as a clean branch-wide guardrail pass.

## Compatibility

- No-profile invocations preserve the existing run path.
- Existing provider, model, provider-profile, and tool options retain explicit precedence.
- Existing tool aliases remain accepted.
- Config files without `[profiles]` deserialize unchanged.
- No protocol or persisted-session schema changes are introduced.

## Risk

**Medium.** The change crosses CLI parsing, config loading, provider initialization, tool selection, prompt construction, and headless output modes. The blast radius is limited by resolving profiles only for `Command::Run`, returning immediately when `--profile` is omitted, and keeping all resolved state local to one Agent. Focused and built-runtime tests cover precedence, isolation, aliases, output modes, negative cases, and compatibility.

## Related work

- Existing issue: #721
- This draft intentionally proposes only the headless named-profile contract. Interactive lifecycle and inheritance can be evaluated separately to keep the initial contribution reviewable.
