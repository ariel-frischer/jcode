---
name: session-feedback
description: Use only when the user explicitly invokes /session-feedback [session-id] to produce one bounded, privacy-safe, review-only assessment of the current or named visible session.
allowed-tools: bash
---

# Session Feedback

Run this workflow only for an explicit human invocation of:

```text
/session-feedback [session-id]
```

This slash skill is the only v1 user entry point. Do not register or simulate a session-end, pre-close, background, scheduled, or other automatic trigger.

## Session selection

- A bare Jcode invocation forwards the trusted in-memory ID as `current_session_id=<id>`. Treat that marker as the current session, pass `<id>` as `current_session_id`, and include it in `visible_session_ids`.
- With one argument, treat the complete forwarded value as the requested session ID. Pass it unchanged as the entry point's single positional argument.
- Include only session IDs already visible in the current interaction or trusted session metadata. Never discover sessions by scanning files, databases, logs, or transcripts.
- If the current session ID is unavailable, or a named ID is unknown or not visible, run no model work and report the entry point's actionable validation error.

## Evidence boundary

Build `visible_items` only from information already present in the active context. Each item must match one of the bounded `evidence-v1` categories:

- `visible_outcome`
- `todo_assessment`
- `tool_invocation_receipt`
- `skill_invocation_receipt`
- `failure_excerpt`
- `validation_receipt`
- `relevant_path`

Use the exact required fields for each category. Do not substitute a prose-only receipt for a structured receipt:

- `visible_outcome` requires `reference`, `category`, and `summary`.
- `todo_assessment` requires `reference`, `category`, `summary`, and `status`.
- `tool_invocation_receipt` requires `reference`, `category`, `name`, and `outcome`.
- `skill_invocation_receipt` requires `reference`, `category`, `name`, and `outcome`.
- `failure_excerpt` requires `reference`, `category`, and `excerpt`.
- `validation_receipt` requires `reference`, `category`, `name`, and `outcome`.
- `relevant_path` requires `reference`, `category`, and `path`.

Tool and skill `outcome` values are `succeeded`, `failed`, `blocked`, or `cancelled`.
Validation `outcome` values are `passed`, `failed`, or `skipped`.
Todo `status` values are `pending`, `in_progress`, `completed`, or `cancelled`.
Optional `summary`, `relevant_path`, and `content_hash` fields must still satisfy `evidence-v1` when present.

Use concise summaries and stable bundle-local references. Include a relevant path or content hash only when it is already visible. A short failure excerpt may be included, but never bulk output.

Do not load, reconstruct, summarize, or transmit:

- full or partial session transcripts beyond an already-visible bounded failure excerpt
- repeated startup or system instructions
- raw edits, patches, or diffs
- images, binary data, or base64 content
- bulk successful tool output
- complete skill or instruction corpora
- repository content read merely to enrich the evidence

Do not perform broad repository reads or filesystem scans. Do not require or invoke a session librarian or `jcode-zor`. The deterministic fallback input is sufficient on first run. When an exact `summary.json` path from a completed `session-summary.v1` librarian invocation is already visible, you may forward that one path as `librarian_summary_path`. Never search for, infer, or auto-generate it.

## Invoke the copy-local entry point

Resolve only the two supported exact installation locations. Do not search elsewhere:

1. `.jcode/skills/session-feedback/__main__.py`
2. `~/.jcode/skills/session-feedback/__main__.py`

Invoke the selected helper with Python 3. Send exactly one UTF-8 JSON object on standard input with these fields:

```json
{
  "current_session_id": "already-visible-current-session-id",
  "visible_session_ids": ["already-visible-current-session-id"],
  "librarian_summary_path": "/exact/already-visible/path/to/summary.json",
  "visible_items": [
    {
      "reference": "outcome-1",
      "category": "visible_outcome",
      "summary": "One concise outcome already visible in this session."
    }
  ]
}
```

For a named session, pass its forwarded ID as one separately quoted positional argument. For the current session, pass no positional argument. Never use `eval`, interpolate the ID into executable shell text, or write the evidence to a repository file. The entry point enforces a 256 KiB standard-input limit and validates unknown fields, session visibility, evidence categories, and aggregate bounds.

Conceptual invocation:

```text
python3 <copy-local-session-feedback/__main__.py> [exact-forwarded-session-id] < bounded-visible-evidence.json
```

Omit `librarian_summary_path` when no exact compatible path is already visible. When supplied, the helper reads only that bounded file, verifies its selected session and `session-summary.v1` format, and converts only structured goal, outcome, decision, unresolved-work, risk, next-step, and relevant-file fields into allowlisted evidence. It intentionally excludes route, usage, fingerprint, generation metadata, and duplicate handoff prose. If the supplied summary is incompatible, the helper uses valid bounded `visible_items`; it fails if neither source is usable.

The directory is self-contained and must work unchanged in either supported installation location.

## Install, first run, and remove

The project-local directory is the complete distributable artifact. To install it globally, copy
the directory unchanged rather than copying individual files or rewriting paths:

```text
.jcode/skills/session-feedback/ -> ~/.jcode/skills/session-feedback/
```

The helper resolves its schemas, fixtures, and Python modules relative to its own installed
directory. It keeps runtime state separate beneath `~/.jcode/feedback/` by default. On the first
explicit invocation it validates configuration and visible evidence, then creates the local
feedback layout and bootstraps `~/.jcode/feedback/.beads` without a remote or replication. Runtime
outputs stay beneath that feedback root:

- `config.json` for optional non-secret operator configuration
- `runs/` for bounded run artifacts
- `proposals/` for reviewable proposal JSON and Markdown
- `.beads/` for the local proposal queue

The project-local copy follows normal skill precedence when both copies exist. Removing only the
project-local directory leaves the unchanged user-global copy available. Removing both
`session-feedback` directories removes only the `/session-feedback` skill. It does not require a
Jcode uninstall, data migration, daemon change, or core-code rollback, and it does not alter other
slash commands or normal session behavior. Existing `~/.jcode/feedback/` review records are user
data and are not deleted automatically.

Bootstrap, configuration, input, or persistence failures are visible: the entry point exits
nonzero and writes a bounded `session-feedback: ...` diagnostic to standard error instead of
reporting success. Correct the named input, configuration, executable availability, or filesystem permission
and invoke the skill again explicitly. Do not recover by broadening evidence, enabling replication,
deleting the feedback store, editing a target, or silently retrying model work.

## Operator configuration

The slash surface supports one optional positional argument, `session-id`. The copy-local
`__main__.py` entry point intentionally exposes no configuration flags. Reusable orchestrators
and tests may supply the same field names below through the helper's `invocation_config` object.
Operators may otherwise use the matching environment variables or the JSON object at
`~/.jcode/feedback/config.json`.

Configuration resolves independently for each field in this exact order:

1. non-empty `invocation_config` value
2. non-empty `JCODE_SESSION_FEEDBACK_*` environment value
3. non-empty field in `~/.jcode/feedback/config.json`
4. built-in default

An empty or whitespace-only string means unset, so resolution continues to the next source.
A present non-empty invalid value fails at that source without falling back. Unknown invocation
or persisted fields, malformed or non-object persisted JSON, and persisted files larger than
65,536 bytes also fail before evidence acquisition or model work. Configuration contains no
credentials, and diagnostics report only configured and effective non-secret values and their
sources.

| Invocation/persisted field | Environment variable | Default | Accepted range or values | Unit |
|---|---|---:|---|---|
| `model` | `JCODE_SESSION_FEEDBACK_MODEL` | `gpt-5.6-sol` | exactly `gpt-5.6-sol` | model ID |
| `effort` | `JCODE_SESSION_FEEDBACK_EFFORT` | `medium` | `none`, `minimal`, `low`, `medium`, `high`, `xhigh`, or `max` | effort level |
| `max_evidence_bytes` | `JCODE_SESSION_FEEDBACK_MAX_EVIDENCE_BYTES` | `262144` | positive integer | exact UTF-8 bytes |
| `max_excerpt_bytes` | `JCODE_SESSION_FEEDBACK_MAX_EXCERPT_BYTES` | `32768` | positive integer | exact UTF-8 bytes |
| `max_input_tokens` | `JCODE_SESSION_FEEDBACK_MAX_INPUT_TOKENS` | `32768` | positive integer | estimated tokens |
| `max_output_tokens` | `JCODE_SESSION_FEEDBACK_MAX_OUTPUT_TOKENS` | `8192` | positive integer | estimated tokens |
| `max_proposals` | `JCODE_SESSION_FEEDBACK_MAX_PROPOSALS` | `8` | positive integer | proposals |
| `max_elapsed_seconds` | `JCODE_SESSION_FEEDBACK_MAX_ELAPSED_SECONDS` | `120.0` | positive finite number | seconds |
| `max_estimated_cost_usd` | `JCODE_SESSION_FEEDBACK_MAX_ESTIMATED_COST_USD` | `1.0` | positive finite number | estimated USD |

Integer fields reject booleans, zero, negative, fractional, and nonnumeric values. Numeric fields
reject booleans, zero, negative, nonnumeric, infinite, and NaN values. Limits are inclusive:
an observed value equal to its configured maximum is accepted, while a larger value fails
actionably. Where bounded excerpt truncation is permitted it is deterministic; other exceeded
limits fail without a second request or partial persistence.

The built-in route is native OpenAI OAuth with model `gpt-5.6-sol`, effort `medium`, and a hard
maximum of one generator request. Model and effort configuration do not select API-key auth or
another provider. Do not add credentials to `config.json` or `JCODE_SESSION_FEEDBACK_*` values.

### Accounting labels

- `accounting.evidence.serialized_bytes`, `accounting.evidence_bytes`,
  `accounting.excerpts.serialized_bytes`, and `accounting.excerpt_bytes` are measured exact
  UTF-8 byte counts of canonical serialized data.
- Every `estimated_tokens` value, including `accounting.request_input.estimated_tokens` and
  `accounting.request_output.estimated_tokens`, is a deterministic estimate labeled as such,
  calculated as the ceiling of UTF-8 bytes divided by four. It is not provider-reported usage.
- `accounting.elapsed_seconds` is measured runner elapsed time.
- `accounting.observed_input_tokens` and `accounting.observed_output_tokens` are provider-reported
  usage when the default `jcode run --json` boundary supplies it; injected runners may report
  `null` when no provider was called.
- `accounting.estimated_cost_usd` is an estimate supplied by the bounded runner receipt, not a
  billed-cost claim. The default native OAuth JSON boundary does not report billed USD, so it
  records `0.0` rather than misreporting the configured maximum as observed spend; token and time
  ceilings remain the authoritative default-run bounds.
- `accounting.proposal_count` is the validated generated proposal count, and
  `accounting.request_count` is measured locally and must be `0` or `1`.

### Bounded configuration examples

Persist non-secret limits only:

```json
{
  "effort": "low",
  "max_evidence_bytes": 131072,
  "max_excerpt_bytes": 16384,
  "max_input_tokens": 16384,
  "max_output_tokens": 4096,
  "max_proposals": 4,
  "max_elapsed_seconds": 90.0,
  "max_estimated_cost_usd": 0.5
}
```

Override one bounded value for a synthetic or injected-fake-generator run:

```text
JCODE_SESSION_FEEDBACK_MAX_PROPOSALS=2 \
python3 <copy-local-session-feedback/__main__.py> < bounded-visible-evidence.json
```

These examples do not require a live or paid validation request. Use the injected fake generator
in tests; never weaken the one-request, privacy, or persistence boundaries to exercise config.

## Report the structured outcome

Parse the helper's JSON result and report:

- `session_id`
- `evidence_source`
- `status`
- `proposal_count`
- `accounting.evidence.serialized_bytes` and `accounting.evidence_bytes`
- `accounting.excerpts.serialized_bytes` and `accounting.excerpt_bytes`
- `accounting.request_input.estimated_tokens` and `accounting.request_output.estimated_tokens`
- `proposal_locations`, when present
- the bounded validation failure, when the helper exits unsuccessfully

`zero_proposals` is a successful, explicit result. State that no sufficiently supported proposal was produced. Do not describe it as a failure and do not invent a proposal.

If the session is invalid or invisible, report the validation error and stop. Do not retry with another session, broaden evidence, or perform model work.

## Review-only guardrails

This workflow may create or append bounded proposal evidence only when the helper supports that later stage. It must never:

- edit an instruction, skill, configuration, hook, SDK, or Jcode target
- apply a patch
- approve a proposal or change proposal lifecycle state
- start implementation
- delete or replace a skill
- perform destructive, external, publishing, or replication actions

The standalone artifact adds no production core-Jcode requirement, dedicated `jcode feedback` CLI,
plugin ABI, session-end trigger, pre-close prompt, Beads replication, proposal approval,
patch application, or implementation behavior. V1 remains an explicit human-triggered review lane.

`orchestrate_feedback(...)` is the reusable callable for the slash flow and a possible future
explicit caller. Reuse that bounded interface rather than duplicating orchestration. Its existence
does not register a hook or automatic caller, relax the review-only boundary, or authorize lifecycle
changes. Return the review result to the user and stop.
