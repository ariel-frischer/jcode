# Lifecycle observability

This document describes the built-in, local lifecycle observability contract.
The feature records privacy-safe operational metadata for one session or run at
a time. The per-session JSONL sidecar is an internal storage detail, not a
transcript or a user-facing file format.

## Schema

Lifecycle records are versioned envelopes with a synthetic or resolved session
identifier, recorder-assigned sequence, timestamp, and one typed event payload.
Queries return the effective local status, ordered events, and bounded
compatibility warnings.

Example identifiers use only non-sensitive placeholders such as
`synthetic-session-001`.

## Supported categories

The contract covers policy snapshots, compaction, handoff, retry,
strategy-switch, and block decisions. Category-specific fields are limited to
approved enums, counts, sizes, policy values, and opaque identifiers.

## Privacy guarantees

Lifecycle observability excludes raw prompts, command text, command output,
environment values, secret values, unnecessary todo content, and raw error
details from persisted, logged, and queried representations. Active process
references use an opaque process-manifest identifier only.

## Configuration

The `[lifecycle_observability]` section has one master switch and two independent
local output switches:

```toml
[lifecycle_observability]
enabled = true
persist_session_events = true
emit_structured_logs = false
```

The master switch makes both outputs inactive when it is false. Environment
overrides use the `JCODE_LIFECYCLE_OBSERVABILITY_*` names and do not change
remote telemetry consent.

## Query usage

Operators retrieve one resolved session through the built-in lifecycle query
interface, for example:

```text
jcode session lifecycle synthetic-session-001 --json
```

The structured result is the supported interface. Users do not need to open a
sidecar directly.

## Retention and rotation

Lifecycle persistence is bounded per session. The active JSONL file rotates
before a complete event would exceed 1 MiB, retains at most three rotations,
and applies a 30-day age limit. A single event is never split across files.

## Compatibility warnings

Readers tolerate an incomplete final JSONL line after an interrupted append.
Malformed records and schema versions newer than the reader produce bounded
warnings rather than being silently reinterpreted. Earlier valid events remain
queryable.

## Cleanup

Lifecycle sidecars and their retained rotations belong to the canonical session
artifact set. Removing a session removes its lifecycle artifacts without
touching neighboring sessions.

## Scope and non-goals

This feature does not add aggregate storage, global SQLite, remote telemetry,
benchmarking, Locus integration, a workflow engine, or general transcript and
content capture.
