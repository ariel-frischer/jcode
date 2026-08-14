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

- With no argument, review the current session. Supply its already-visible ID as `current_session_id` and include it in `visible_session_ids`.
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

Use concise summaries and stable bundle-local references. Include a relevant path or content hash only when it is already visible. A short failure excerpt may be included, but never bulk output.

Do not load, reconstruct, summarize, or transmit:

- full or partial session transcripts beyond an already-visible bounded failure excerpt
- repeated startup or system instructions
- raw edits, patches, or diffs
- images, binary data, or base64 content
- bulk successful tool output
- complete skill or instruction corpora
- repository content read merely to enrich the evidence

Do not perform broad repository reads or filesystem scans. Do not require or invoke a session librarian or `jcode-zor`. The deterministic fallback input is sufficient on first run.

## Invoke the copy-local entry point

Resolve only the two supported exact installation locations. Do not search elsewhere:

1. `.jcode/skills/session-feedback/__main__.py`
2. `~/.jcode/skills/session-feedback/__main__.py`

Invoke the selected helper with Python 3. Send exactly one UTF-8 JSON object on standard input with these fields:

```json
{
  "current_session_id": "already-visible-current-session-id",
  "visible_session_ids": ["already-visible-current-session-id"],
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

The directory is self-contained and must work unchanged in either supported installation location.

## Report the structured outcome

Parse the helper's JSON result and report:

- `session_id`
- `evidence_source`
- `status`
- `proposal_count`
- `accounting.serialized_bytes` and `accounting.estimated_tokens`
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

Return the review result to the user and stop.
