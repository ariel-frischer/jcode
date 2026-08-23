# Named Session Profiles

Named session profiles bundle reusable settings for the interactive TUI,
`jcode run`, `jcode repl`, or `jcode serve`. They do not change ACP, SDK clients,
child sessions, swarm workers, or cloud-provider profile flags.

## Configure a profile

Add a profile under `[profiles.<name>]` in `~/.jcode/config.toml`:

```toml
[profiles.review]
provider = "openrouter"
model = "openai/gpt-5.6"
reasoning_effort = "high"
tool_profile = "minimal"
tools = ["read", "agentgrep"]
disabled_tools = ["bash"]
skills = ["pr-reviewer"]
instructions = "Review correctness and regression risk."
```

All fields are optional. Supported fields are `provider`, `model`,
`reasoning_effort`, `provider_profile`, `tool_profile`, `tools`,
`disabled_tools`, `skills`, and `instructions`.

Use either `provider` for a built-in provider or `provider_profile` for a named
`[providers.<name>]` configuration. They are competing provider selectors, not
independent fields.

## Validate this feature

Add this minimal profile to `~/.jcode/config.toml`:

```toml
[profiles.profile-demo]
instructions = "Always end responses with PROFILE_DEMO_OK."
```

Then validate the public TUI and one-shot entry points:

```bash
jcode --profile profile-demo run "Reply with one short sentence"
jcode --profile profile-demo
```

The one-shot response should end with `PROFILE_DEMO_OK`. In the TUI, send the
same prompt and expect the same suffix. If a server is already running, stop it
with `jcode server stop` before the interactive validation so the selected
server-owned provider and tool settings can be applied.

## Select a profile

`--profile` is a session option:

```bash
jcode --profile review
jcode --profile review run "Review this change"
jcode --quiet --profile review run --json "Review this change"
jcode --quiet --profile review run --ndjson "Review this change"
jcode --profile review repl
jcode --profile review serve
```

Use `jcode --help` for the current command-line contract. The unrelated
`--provider-profile` flag selects a named provider configuration, and cloud
commands may define their own provider-specific `--profile` options.

## Precedence

Supported settings resolve in this order:

1. explicit invocation option
2. environment variable
3. selected session profile
4. unprofiled persisted configuration
5. built-in default

An explicit `--provider auto` remains an explicit override. Lower-priority
sources fill only settings omitted by higher-priority sources.

`provider` and `provider_profile` participate in that ordering as one provider
selection. A higher-priority `provider` clears a lower-priority
`provider_profile`, while a higher-priority `provider_profile` selects the final
runtime provider required by that named configuration. Supplying both selectors
at the same precedence is rejected instead of silently changing providers.

## Tools, skills, and instructions

Tool settings compose through the normal tool-profile and enabled/disabled tool
rules. Tool aliases and documented wildcard values use the canonical tool
registry.

Selected skills are loaded from the effective launch working directory. Profile
instructions and skill prompts apply only to the new agent session. Normal
global and project guidance comes first, followed by profile instructions and
then selected skill prompts.

## Validation and isolation

Profile names are exact and case-sensitive. Unknown names, malformed selected
settings, unavailable selected tools, and unavailable selected skills fail before
the provider request. Diagnostics identify the setting without reproducing
profile instructions or credentials.

Selecting a profile does not rewrite `config.toml` or persist profile state into
another launch. Omitting `--profile` preserves the existing unprofiled session path.
