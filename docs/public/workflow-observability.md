# Experimental workflow progress and health

Status: implemented in this custom fork, disabled by default. Runtime acceptance is tracked in `specs/026-workflow-observability/acceptance.md` until the feature is landed.

The main TUI can show a small **Workflows** box beneath its session header while the main model is idle. A server-owned observer supplies counts, current stage/activity, health, and separate activity/checkpoint ages. Rendering does not read artifacts, run commands, or call a model.

## Enablement

These are optional settings in Jcode's canonical configuration:

```toml
[workflow]
enabled = true
show_panel = true
max_visible = 3
poll_seconds = 2
quiet_seconds = 300
terminal_retention_seconds = 300
autospec_enabled = false
```

All defaults are shown except `enabled`, which defaults to `false`. Native owned subagents do not require the Autospec adapter. Set `autospec_enabled = true` only to observe explicitly registered artifact-producing worktrees. Enabling the adapter does not launch Autospec or attach an existing controller.

Environment overrides use the `JCODE_WORKFLOW_` prefix with these suffixes: `ENABLED`, `AUTOSPEC_ENABLED`, `SHOW_PANEL`, `MAX_VISIBLE`, `POLL_SECONDS`, `QUIET_SECONDS`, and `TERMINAL_RETENTION_SECONDS`. Valid environment values override persisted configuration. Invalid environment values produce a warning and retain the persisted value. Invalid effective configuration disables the observer.

Bounds are 1–8 visible workflows, 1–3600 seconds between polls, 30–86400 seconds before quiet suspicion, and 0–86400 seconds terminal retention. Disabled/unset configuration starts no monitor and performs no workflow registry I/O. `show_panel = false` hides the panel and opts the TUI connection out of workflow events, but does not stop an enabled server observer. Server and TUI resolve their own canonical configuration at startup. Reconnect/restart the applicable process after changing these settings. No automatic restart or provider change occurs.

## Optional artifact registration

Registration is an explicit, one-time call to the existing `bg` tool, not a shell command or repeated monitoring prompt:

```json
{
  "action": "observe",
  "observation": {
    "working_dir": "/absolute/path/to/owned-worktree",
    "tasks_file": "specs/example/tasks.yaml",
    "status_file": "workflow-status.json",
    "label": "Example implementation"
  }
}
```

`status_file` and `label` are optional. Artifact paths must remain inside the registered worktree. The caller's actual session owns the registration. A model cannot supply a different owner, and two sessions cannot claim the same worktree. Repeating the same registration is idempotent. To remove it, call `bg` with `{"action":"unobserve","task_id":"<returned ID>"}` from the owning session. Removal does not terminate a process or delete producer files.

The adapter reads the fork's `tasks.yaml` shape: a `phases` sequence containing task IDs, titles and statuses. Known task counts are displayed directly, never as an invented percentage. Completing task checkboxes alone does not prove the controller completed successfully.

A producer may optionally write a small structured controller status file:

```json
{"state":"failed","error_code":"insufficient_quota"}
```

Supported states are `running`, `waiting`, `retrying`, `blocked`, `failed`, `completed`, and `stopped`. Known quota, rate-limit and authentication codes become bounded observer-authored explanations. Raw `message` fields are ignored. **This adjunct is an optional producer contract, not a claim that Autospec currently emits it.** Missing or malformed artifacts produce an observer warning and preserve last-good counts and confirmed failures.

## Interpreting the box

- **Running** is authoritative running work, not proof of recent progress.
- **Quiet?** is a silence suspicion, never an inferred failure.
- **Waiting**, **Blocked**, **Failed**, **Completed**, and **Stopped** reflect explicit lifecycle evidence.
- **Observer error** means monitoring evidence is unavailable or invalid, not that the underlying work failed.
- **activity** measures the age of actual activity evidence. **checkpoint** measures the age of semantic progress changes. Duplicate activity does not reset a checkpoint. `?` means unknown, not zero.

Explicit failures, blocked work and observer errors appear before ordinary running work when space is limited. The title reports how many entries are hidden. The panel takes at most half the available post-header height and preserves at least eight rows for transcript/input. Empty, disabled, very narrow and very short layouts keep the original transcript layout.

Native child ownership follows its explicit parent first. New parentless phase sessions can associate only with an exclusive registered worktree, and only when started after registration. There is no global process or saved-session scan and no implicit attachment of preexisting sessions. The registered workflow keeps its identity and artifact counts across phases. A successful phase alone does not complete the controller. Explicit failures persist across reconnect and disappear only according to confirmed lifecycle/retry and bounded retention rules.

## Safety, persistence and compatibility

The server is the only observer. It uses a single awaited background filesystem poll with skipped missed ticks, bounded native activity state, and a coalescing per-connection snapshot feed. A slow client cannot enqueue unlimited tick snapshots or block observation for other clients. Workflow updates do not wake the model, restart work, cancel processes, change providers, or generate monitoring turns.

Only connections sending the additive `Subscribe.workflow_progress = true` capability receive `WorkflowStatus` events, and only when the server observer is enabled. Legacy clients receive no unknown event variant. Snapshots are scoped to the connection's current session. The TUI ignores stale-owner events and retains a same-session snapshot across history refresh.

Absent native members produce stale-evidence warnings rather than invented failure. Their retained records expire after the configured retention window, except the latest registered phase remains available to prevent old failure becoming running. Capacity warnings do not freeze unrelated observations. Per-session snapshot truncation is explicit and never silently discards a different owner before filtering.

Newly observed explicit controller terminal/retry evidence can supersede an older native phase outcome. The recovery epoch is retained through later Running metadata. An unchanged Running file or completed task checkboxes cannot erase a native failure. Same-second conflicting evidence conservatively keeps the native adverse outcome.

Private state lives under `JCODE_HOME/workflow/registry.json`, protected by a stable `registry.lock` sidecar held for the observer lifetime. A second daemon sharing that home fails safely rather than overwriting stale state. **Never delete a live lock file.** If the registry is damaged, preserve it before repair and restart only after resolving the cause. Isolated test daemons should use their own Jcode home as well as their own socket.

Limits include 64 explicit registrations, 256 retained native records/snapshots, 512 KiB artifact and persisted-registry bounds, 256 phases, 4096 tasks, and bounded display strings. Files are read as regular contained files with no-follow traversal. Linux safety tests cover ancestor replacement races. Windows traversal/locking implementation requires its separate platform validation before a Windows runtime guarantee can be made.

The generic display DTO and renderer contain no Autospec schema. Configuration, protocol capability, server observation, optional artifact parsing, and TUI presentation remain separate seams for upstream synchronization.
