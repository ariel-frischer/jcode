# Bash execution modes

Jcode's Bash tool separates foreground responsiveness from process termination. A command can complete directly, soft-yield into managed background execution, start in managed background execution immediately, or stop at an explicit hard timeout.

## Mode summary

| Inputs | Behavior |
|---|---|
| `run_in_background` omitted or `false`, command finishes before `soft_yield_ms` | Returns the completed command result directly. No background task is created. |
| `run_in_background` omitted or `false`, command outlasts `soft_yield_ms` | Returns a task ID while the original command continues exactly once under the existing background manager. Soft yield does not terminate or restart the command. |
| `run_in_background: true` | Returns a managed task immediately without waiting for the soft-yield window. |
| Explicit `timeout`, command reaches that deadline | Terminates the command process group and records exit code `124`, whether the command is still foreground or is already managed in the background. |

`run_in_background` takes precedence over the soft-yield wait. An explicit hard timeout remains active in every mode.

## `soft_yield_ms`

`soft_yield_ms` is the foreground wait window in **milliseconds**.

- Omitted: defaults to `10000` ms (10 seconds).
- Positive value: changes only when a still-running foreground command becomes a managed background task.
- `0`: disables automatic soft yield, so the call waits for natural completion or an explicit hard timeout.
- It never kills, restarts, or duplicates the command.

Examples:

```json
{"command":"cargo test -p jcode-app-core"}
```

Uses the default 10-second soft-yield window. A fast test run returns directly. A longer run returns a managed task after approximately 10 seconds and continues running.

```json
{"command":"sleep 2; echo done","soft_yield_ms":250}
```

Returns a managed task after approximately 250 ms while the original command continues.

```json
{"command":"sleep 2; echo done","soft_yield_ms":5000}
```

Allows up to 5 seconds for direct completion, so this command normally returns its completed result without creating a task.

```json
{"command":"sleep 2; echo done","soft_yield_ms":0}
```

Disables automatic soft yield and waits for direct completion because no hard timeout was supplied.

## `timeout`

`timeout` is an optional hard wall-clock deadline in **milliseconds**.

- Omitted: no hard deadline is imposed by the Bash tool.
- Explicit value: terminates the command process group at the deadline and reports exit code `124`.
- Values above `1800000` ms are capped at the effective maximum of `1800000` ms (30 minutes).
- It is independent of `soft_yield_ms`. Soft yield can return control before the hard deadline, but it does not cancel that deadline.

```json
{"command":"sleep 60","soft_yield_ms":500,"timeout":2000}
```

Returns a managed task after approximately 500 ms. If the command is still running at 2000 ms from its original start, the manager terminates its process group and records exit code `124`.

```json
{"command":"sleep 60","soft_yield_ms":0,"timeout":2000}
```

Does not soft-yield. It returns a terminal timeout result with exit code `124` after approximately 2 seconds.

## Immediate managed background execution

Set `run_in_background: true` when the caller should regain control immediately:

```json
{"command":"cargo test --workspace","run_in_background":true,"timeout":1800000}
```

The command enters the same manager used by automatically yielded work. The soft-yield window is bypassed, while the optional hard timeout still applies.

## Managed task lifecycle

Immediately backgrounded and automatically yielded commands share the existing task lifecycle:

- status, bounded output, wait, cancellation, and cleanup operations
- progress parsed from output or emitted as `JCODE_PROGRESS {json}` lines
- optional completion notification and wake behavior
- optional stall wake after the configured inactivity interval
- supported reload persistence and deadline reconciliation

Output returned during soft yield and retained by the manager remains bounded. Use the background task's output operation to retrieve later output within those limits. Cancelling or timing out managed Bash work targets the original process group, including descendants.
