# Unreleased custom-fork changes

- Optional main-TUI workflow progress and health shows owned work while the model is idle, with an explicit Autospec artifact adapter, session-isolated snapshots and bounded failure/reconnect handling. Disabled by default.

- OpenAI sessions can recover once from a stale websocket chain that reports a missing tool result, reusing saved results without rerunning tools or replaying partial output.

- Fork releases now build and publish only from user-promoted `main` commits, with deterministic calendar tags, exact source provenance, complete CLI artifact validation, and no changes to upstream runtime update defaults.

This is a pending release note, not a released version entry. Promote it into the repository's versioned JSON changelog when cutting the containing release.
