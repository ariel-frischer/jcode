# Lifecycle observability

This document describes Jcode's built-in, local lifecycle observability contract. It
records privacy-safe operational metadata for one session or run at a time. The
per-session JSONL files are internal persistence details, not transcripts, replay
events, or a user-facing file format.

## Schema and versioning

Each accepted record is a `LifecycleEventEnvelope` with these fields:

| Field | Meaning |
| --- | --- |
| `schema_version` | Lifecycle event schema version. The current version is `1`. |
| `session_id` | The validated opaque identifier of the owning session or run. |
| `sequence` | Recorder-assigned monotonic order within that session stream. |
| `recorded_at` | Informational UTC timestamp. Sequence is authoritative for ordering. |
| `event` | One of the typed categories described below. |

The recorder owns sequence assignment, privacy filtering, enablement, and delivery
to local outputs. Callers do not write JSONL records directly.

### Categories and fields

`policy_snapshot` records the effective policy used by the session. Its `snapshot`
contains:

- `policy_version` and a safe `fingerprint`.
- `compaction`: `mode` (`reactive`, `proactive`, `semantic`, or `native`),
  `context_window_tokens`, `threshold_ratio`, and `native_compaction`.
- `handoff`: `enabled`, `agent_enabled`, `confirmation_required`, `auto_start`,
  `max_chain_transitions`, and `copy_todos`.

A policy snapshot is emitted at session setup and when the effective policy
fingerprint changes. Duplicate fingerprints are not emitted again.

`compaction` contains `decision_type`, `semantic_reason`, optional
`suppression_reason`, optional `context_usage` (`used_tokens`,
`context_window_tokens`, and bounded `ratio`), and an optional opaque
`process_manifest_id`.

`handoff` contains the same decision fields plus a `payload` with:

- `chain_depth` and `generated_prompt_bytes` as numeric metadata only.
- `todo_carryover_count`, never todo text.
- `parent_session_id` and optional `child_session_id`.
- Optional `startup_acknowledged` and `startup_outcome`.

`startup_outcome` is one of `pending`, `started`, `completed`, `failed`, or
`incomplete`. A missing acknowledgment is represented as `null`, not as an
invented result.

`retry` contains the decision fields plus numeric `attempt` and `max_attempts`,
and an optional opaque `process_manifest_id`.

`strategy_switch` contains the decision fields and records transparent provider
or model strategy changes without provider response text.

`block` contains the decision fields and an optional opaque
`process_manifest_id`. It is used for canonical tool-policy, run-safety,
destructive-command, and handoff-policy block decisions rather than for generic
error capture.

The closed decision vocabulary is `attempted`, `accepted`, `suppressed`,
`started`, `completed`, `failed`, `exhausted`, and `pending`. Semantic reasons
are closed values such as `manual`, `automatic`, `native_fallback`,
`context_limit`, `already_within_budget`, `retryable_failure`,
`provider_fallback`, `policy`, `startup`, `child_startup`, and `shutdown`.
Suppression reasons are closed values such as `disabled`,
`already_within_budget`, `confirmation_required`, `chain_limit`, `policy_denied`,
`no_child_session`, `queue_full`, and `unsupported`.

## Privacy guarantees

The event model accepts only approved enums, counts, sizes, timestamps, policy
values, validated session identifiers, and opaque process-manifest identifiers.
Lifecycle persistence, local structured logs, and query results exclude:

- raw prompts, generated handoff prompt content, and command text;
- command output, environment values, credentials, and secret values;
- raw provider or filesystem error details; and
- unnecessary todo content.

An active long-running process is correlated only by its durable opaque
`process_manifest_id`. The identifier does not include a PID, command, output
path, output content, environment, or secret. Examples in this document use
placeholders such as `synthetic-session-001` and `manifest-7f3a` only.

## Configuration and effective status

The `[lifecycle_observability]` section has one master switch and two independent
local output switches:

```toml
[lifecycle_observability]
enabled = true
persist_session_events = true
emit_structured_logs = false
```

The effective defaults are `enabled = true`, `persist_session_events = true`,
and `emit_structured_logs = false`. The master switch wins: when `enabled` is
false, both output values are effective `false`, even if their configured values
are true. Persistence and local structured logging can otherwise be selected
independently.

Configuration precedence is, from lowest to highest: built-in defaults, the
persisted config file, then environment overrides. The supported environment
variables are:

- `JCODE_LIFECYCLE_OBSERVABILITY_ENABLED`
- `JCODE_LIFECYCLE_OBSERVABILITY_PERSIST_SESSION_EVENTS`
- `JCODE_LIFECYCLE_OBSERVABILITY_EMIT_STRUCTURED_LOGS`

Boolean environment values use Jcode's existing boolean parser. An invalid
boolean leaves the persisted value unchanged. There is no per-session override.
The existing `/config` status output reports configured and effective values,
including master-switch suppression, for example:

```text
**Lifecycle observability:**
Enabled: configured=true effective=true
Persist session events: configured=true effective=true
Emit structured logs: configured=false effective=false
```

These local settings neither enable remote usage telemetry nor change telemetry
consent.

## Query usage

Use the built-in command to resolve a session ID or memorable short name and
retrieve one typed stream. `--json` emits stable structured JSON; without it,
Jcode renders the same typed result as a human-readable event list.

```text
jcode session lifecycle synthetic-session-001 --json
```

The query result is a `SessionLifecycleStream` containing:

- `session_id`;
- effective `status` with `enabled`, `persist_session_events`, and
  `emit_structured_logs`;
- ordered `events`; and
- bounded `warnings`.

The internal protocol exposes the same result without direct sidecar access:

```json
{"get_lifecycle_events":{"id":41,"session_id":"synthetic-session-001"}}
```

The response is an additive `lifecycle_events` variant correlated by `id`:

```json
{
  "lifecycle_events": {
    "id": 41,
    "stream": {
      "session_id": "synthetic-session-001",
      "status": {
        "enabled": true,
        "persist_session_events": true,
        "emit_structured_logs": false
      },
      "events": [],
      "warnings": []
    }
  }
}
```

An existing session with no persisted lifecycle history returns a successful
empty stream and status. An unknown session or an invalid session reference is a
command/protocol error. Operators and automation should use this command or the
protocol response rather than opening internal files.

## Retention and rotation

When persistence is enabled, lifecycle artifacts live beside the canonical
session snapshot under the sessions directory:

- active: `<session-id>.lifecycle.jsonl`;
- rotations: `<session-id>.lifecycle.1.jsonl` through
  `<session-id>.lifecycle.3.jsonl`.

The active file rotates before appending a complete record would exceed 1 MiB.
At most three rotations are retained, and artifacts older than 30 days are
pruned. Pruning runs at append, query, and maintenance boundaries. A single
event is never split across files; an event larger than the 1 MiB record boundary
is rejected before persistence. These failures are diagnostics only and do not
change the lifecycle decision or stop session execution.

## Compatibility warnings and failure isolation

Readers process retained rotations and the active file, then sort accepted
records by recorder sequence. A crash-torn incomplete final line is skipped with
a `torn_tail` warning so earlier valid records remain queryable. Malformed
records and schema versions newer than the reader produce bounded
`malformed_record` or `unsupported_schema_version` warnings rather than being
silently reinterpreted. Recorder queue pressure, an unavailable worker, or a
persistence failure can add `dropped_event` or `persistence_unavailable`
indications to the query result.

Recording uses a bounded non-blocking submission path and a serialized worker.
Persistence and local structured-log sinks are best effort. Queue, serialization,
filesystem, logging, and query diagnostics are surfaced through warnings or
local diagnostics, but they never determine compaction, handoff, retry,
strategy-switch, block, or session continuation outcomes.

## Cleanup and compatibility

Lifecycle files are part of the canonical session artifact set. Removing a
session removes its active sidecar and all retained rotations without touching
neighboring sessions. Prepared-spawn and takeover rollback use the same cleanup
boundary.

Lifecycle sidecars use a distinct `.lifecycle.jsonl` suffix. Existing session
picker, search, productivity, crash recovery, import, export, transcript, and
replay workflows continue to recognize their established artifacts and do not
treat lifecycle files as sessions, transcripts, prompts, or user content.

## Scope and non-goals

This feature intentionally does not add:

- a global SQLite database or aggregate event store;
- remote telemetry enablement or changes to telemetry consent;
- benchmark campaigns, Locus integration, or a workflow engine; or
- general transcript, prompt, command-output, or todo-content capture.


## Memory-request accounting (experimental local adapter)

Memory sidecar requests use a separate version-1, content-free accounting record.
The adapter reuses the effective lifecycle master/persistence/structured-log controls
above. The worker checks current effective controls before each output. Disabling
persistence does not enable logging, and persistence failures cannot bypass the
structured-log switch. No remote telemetry or additional inference is involved.

The fixed global ring under the resolved Jcode data root is
`memory-usage/requests.v1.jsonl` plus `.v1.1.jsonl` through `.v1.3.jsonl`, with one
fixed `writer.lock`. This bounds storage across sessions rather than allocating an
unbounded file set per session. It retains at most four 1 MiB files for 30 days.
Read-only queries exclude expired files/records, and subsequent writes prune expired
files. Records are at most 4 KiB, scans read at most 4 MiB and decode at most 4,096
unique requests. Readers reject malformed, torn, oversized or invalid records with
bounded static warning categories, deduplicate request IDs and sort by timestamp/ID.
Opaque session filters are validated and never used as paths.

Files are created owner-only (Unix directories 0700, files 0600). Unix links,
hardlinks and unsafe existing permissions are refused. Windows writes reuse the
existing protected owner-only ACL helpers and reject reparse points. Native Windows
runtime/ACL verification remains outstanding. Reporting never changes ACLs.

One worker per process consumes a fixed 256-entry nonblocking queue. Submission
performs bounded metadata validation and `try_send`, with no filesystem, pricing,
configuration reload or inference work. Failed queues and workers do not change
completion results. Process-local saturating counters expose loss, invalid records,
failed writes/log serialization and flush timeouts. Flush/shutdown waits are capped
at 250 ms and acknowledge queued writes, not fsync or complete historical coverage.
An abrupt process exit may lose queued records.

Every retained history includes `retained_window_only` and
`loss_history_unavailable`. An empty history, missing loss marker or zero current
process counter must never be presented as zero lifetime consumption. Ownerless
calls remain explicitly unattributed. Provider-hidden retry coverage remains
`provider_call_only`. Native Luna pricing is unknown, not another model's rate.
Pricing calculation and the operator per-call/session CLI are subsequent delivery
steps, not capabilities claimed by this adapter checkpoint. The accounting ring is
separate from lifecycle session deletion and expires independently under this
bounded retention contract. It contains no prompts, memories, credentials or raw
provider errors.
