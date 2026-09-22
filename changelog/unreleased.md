# Unreleased custom-fork changes

- Swarm agents now retain their effective model and reasoning effort across busy turns in all TUI swarm layouts and the `swarm list`/`status` output, including attached sessions and visible startup.

- GPT-6 Sol and GPT-6 Luna are now first-class native OpenAI models with curated picker/fallback ordering, published 1.05M-token context limits, model-specific reasoning efforts, exact Standard/Flex/Fast API pricing, and long-context cost accounting.

- `JCODE_OPENROUTER_PROVIDER` and `JCODE_OPENROUTER_NO_FALLBACK` now actually reach OpenRouter request routing: the provider env var is sent as a hard `provider.only` pin (requests fail with a clear OpenRouter 404 instead of silently routing to another upstream when the pinned provider cannot serve the model), and profile/subscription runtime switches no longer wipe these user-supplied env vars. Named OpenRouter-type profiles honor the same pin.

- Optional `[agents].swarm_completion_wake` lets attached coordinators continue after each owned worker finishes, even without a completion wait, and prevents enabled swarm-await wakes from getting stranded at the end of a turn. Disabled by default.

- Optional main-TUI workflow progress and health shows owned work while the model is idle, with an explicit Autospec artifact adapter, session-isolated snapshots and bounded failure/reconnect handling. Disabled by default.

- OpenAI sessions can recover once from a stale websocket chain that reports a missing tool result, reusing saved results without rerunning tools or replaying partial output.

- Fork releases now build and publish only from user-promoted `main` commits, with deterministic calendar tags, exact source provenance, complete CLI artifact validation, and no changes to upstream runtime update defaults.

This is a pending release note, not a released version entry. Promote it into the repository's versioned JSON changelog when cutting the containing release.
