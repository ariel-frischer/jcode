# Feature request: configurable memory-sidecar reasoning effort

Status: local draft, not yet submitted upstream

## Summary

Jcode lets users select the memory sidecar model with `agents.memory_model`, but it does not expose the reasoning effort used for memory relevance, reranking, contradiction checks, or extraction.

Add an optional `agents.memory_reasoning_effort` setting and `JCODE_MEMORY_REASONING_EFFORT` environment override. This would let users deliberately run a memory model such as GPT-5.6 Luna at `xhigh`, while preserving the current low-cost default for users who do not opt in.

## Current behavior

Verified against Jcode v0.64.2 and upstream source on 2026-08-02.

### GPT-5.6 Luna

For both of these configurations:

```toml
[agents]
# memory_model is unset
```

and:

```toml
[agents]
memory_model = "gpt-5.6-luna"
```

Jcode sends:

```json
{
  "model": "gpt-5.6-luna",
  "reasoning": {
    "effort": "none"
  }
}
```

The Luna effort is explicitly hardcoded to `none`. It does not inherit the main session's effort, `openai_reasoning_effort`, or the selected model-picker effort.

### GPT-5.6 Luna OAuth fallback

When Luna is unavailable for the ChatGPT OAuth account, Jcode retries with:

```json
{
  "model": "gpt-5.4",
  "reasoning": {
    "effort": "low"
  }
}
```

Therefore the fallback is explicitly `low`, not `max` and not inherited from the main session.

### Other configured OpenAI memory models

When `agents.memory_model` is an OpenAI model other than `gpt-5.6-luna`, Jcode does not add a `reasoning` field unless an internal caller constructs the sidecar with an explicit override. Normal configuration has no path to provide that override.

In that case, the OpenAI backend chooses its own default. Jcode cannot guarantee that the effective effort is `low`, `medium`, `high`, `xhigh`, or `max`.

## Source of the behavior

`crates/jcode-base/src/sidecar.rs` defines:

```rust
pub const SIDECAR_OPENAI_MODEL: &str = "gpt-5.6-luna";
const SIDECAR_OPENAI_REASONING: &str = "none";
const SIDECAR_OPENAI_OAUTH_FALLBACK_MODEL: &str = "gpt-5.4";
const SIDECAR_OPENAI_OAUTH_FALLBACK_REASONING: &str = "low";
```

`Sidecar::new()` reads only `agents.memory_model` and initializes `reasoning_override` to `None`. `resolve_openai_request_model()` then assigns `none` to Luna or `low` to its OAuth fallback.

Although `Sidecar::with_openai_model(model, reasoning_effort)` supports an explicit effort, it is intended for benchmark code and is not wired to user configuration.

`AgentsConfig` currently contains `memory_model` but no memory reasoning-effort field.

## User impact

- Users cannot trade additional memory-call cost and latency for stronger relevance or extraction judgment.
- The memory sidecar behaves differently from the main model without a visible configuration setting.
- Setting the main session to `gpt-5.6-sol` at `medium`, or Luna at `xhigh`, does not affect memory calls.
- Users selecting another OpenAI memory model cannot know which backend default effort will be used.
- Documentation saying only that the memory model is configurable can lead users to assume its effort is inherited.

## Proposed configuration

```toml
[agents]
memory_model = "gpt-5.6-luna"
memory_reasoning_effort = "xhigh"
```

Environment override:

```text
JCODE_MEMORY_REASONING_EFFORT=xhigh
```

Suggested semantics:

1. When `memory_reasoning_effort` is set, pass it as the memory sidecar's explicit reasoning override.
2. When unset and the selected model is GPT-5.6 Luna, preserve the current `none` default.
3. When unset and Luna falls back to GPT-5.4, preserve the current `low` fallback.
4. When unset for another OpenAI model, preserve the current behavior of omitting the reasoning field.
5. Reject unsupported effort values with a clear configuration error or warning rather than silently ignoring them.
6. Display the resolved memory model and effort in the config summary so users can verify the effective behavior.

## Suggested implementation

Add to `AgentsConfig`:

```rust
pub memory_reasoning_effort: Option<String>,
```

Wire it through:

- `crates/jcode-config-types/src/lib.rs`
- `crates/jcode-base/src/config/default_file.rs`
- `crates/jcode-base/src/config/env_overrides.rs`
- `crates/jcode-base/src/config/display_summary.rs`
- `crates/jcode-base/src/sidecar.rs`
- the TUI memory-agent model/settings UI, if memory effort should be selectable interactively

`Sidecar::with_configured_model()` can initialize `reasoning_override` from the new setting. The existing `self.reasoning_override.as_deref().or(resolved_reasoning)` precedence already makes an explicit override win over Luna's built-in default.

## Acceptance criteria

- `memory_model = "gpt-5.6-luna"` with no effort setting continues sending `reasoning.effort = "none"`.
- `memory_model = "gpt-5.6-luna"` with `memory_reasoning_effort = "xhigh"` sends `reasoning.effort = "xhigh"`.
- `JCODE_MEMORY_REASONING_EFFORT` overrides the TOML setting.
- The memory effort does not inherit from or mutate the main session effort.
- Invalid or model-incompatible values produce an actionable warning or error.
- The config summary reports the configured and effective memory effort.
- Existing Luna-to-GPT-5.4 fallback behavior is explicitly tested when an override is present.
- Default behavior remains backward-compatible and does not increase memory-call cost unexpectedly.

## Related upstream issues

GitHub open and closed issue searches on 2026-08-02 found no issue specifically requesting configurable memory-sidecar reasoning effort.

Related but not duplicative:

- [#679: Configurable memory sidecar backend](https://github.com/1jehuang/jcode/issues/679) concerns choosing `auto`, `openai`, `claude`, or the active provider. It does not add memory reasoning-effort configuration.
- [#708: `reasoning_effort` lacks per-model configuration](https://github.com/1jehuang/jcode/issues/708) concerns main/custom-provider model configuration and gateway compatibility. It does not cover the dedicated memory sidecar.
