# Recent Jcode Session Insights and Recommendations

Date: 2026-08-21  
Scope: read-only analysis of the 30 recent primary session files listed by the prepared session set. This excludes Jcode build-speed work, which is tracked separately.

## Executive summary

The sample does not indicate that ordinary built-in tool dispatch, especially `bash`, is inherently slow. The larger opportunity is workflow lifecycle clarity: selecting meaningful sessions for analysis, distinguishing tool-wrapper overhead from the work performed inside a tool, reliably closing todo and landing states, and choosing compaction or a fresh-session handoff at the right boundary.

The recommended operating model is:

1. Use automatic compaction while one tightly coupled task is still in progress.
2. Use a fresh-session handoff at a verified milestone or when the goal materially changes.
3. Keep lifecycle gates flexible and model-visible through todo items rather than implementing a rigid workflow engine in Jcode.
4. Treat Medium risk as a reason for stronger validation, not as a default reason to leave completed work unmerged.
5. Summarize only long, compacted, or otherwise important sessions. Deterministic extraction should run before any cheap-model summary.
6. Prefer `websearch` and `webfetch`; isolate any visible browser research from the user's personal Firefox session.

## Sample findings

The 30-file snapshot contained:

- 2,010 messages.
- 975 top-level tool calls.
- 20 recorded tool errors.
- 18 sessions with no substantive user prompt.
- Only 8 sessions longer than 3 messages.
- The two largest sessions accounted for 77.5% of messages and 75.1% of tool calls.
- 227 `batch` calls containing 723 child calls, with a median batch size of 3.
- Rare exact duplicate tool calls.
- One long workflow with 49 background-task management calls.
- 23 `schedule` calls, of which 6 produced errors in the sampled logs.
- No `session_transition` or swarm calls in this historical sample.
- 11 memory injections, all concentrated in two compacted sessions.
- 28 todo calls, but only one session visibly ended with every todo terminal.
- Duplicate initial-prompt groups of four and two sessions, suggesting restart or retry behavior without a structured handoff.

These figures are directional rather than a benchmark. Session files can continue changing after a pending set is prepared, nested tool timings are not exclusive measurements, and the sample is highly skewed toward two long sessions.

## Tool duration findings

### Is `bash` itself slow?

Probably not. Existing production-path measurements in [TOOL_PERFORMANCE_PROFILE.md](../TOOL_PERFORMANCE_PROFILE.md) found approximately:

- `bash true`: 1.8 ms median wall time.
- `bash` with 64 KiB output: 2.5 ms median wall time.
- Starting a background `bash`: 0.8 ms median wall time.

A `bash` tool call shown as taking seconds or minutes usually means the command inside it ran for that long. Common examples include builds, tests, network requests, broad filesystem traversal, waiting processes, and commands that were allowed to continue after the foreground response timed out.

Potential overhead still exists around process startup, output capture, hooks, queueing, serialization, and daemon transport, but the recent-session logs cannot reliably attribute those components.

### Why nested tool spans would help

A parent `batch` duration currently includes its children. For example, if a batch contains a five-second command, the batch may also appear to take about five seconds. Reading both values as independent costs would double-count the same work.

Useful internal telemetry would associate parent and child calls using span identifiers and distinguish:

- Queue time: waiting before execution can begin.
- Execution time: time spent performing the actual tool operation.
- Serialization time: preparing and transmitting arguments or results.
- Inclusive time: the tool plus all nested child work.
- Exclusive time: time attributable only to that tool's own wrapper logic.

This would not directly make the model smarter. It would make performance diagnoses trustworthy. Jcode could then report conclusions such as “the command took 18 seconds; Jcode overhead was 7 ms” instead of exposing raw spans to the model. It would also enable meaningful p50, p95, and p99 measurements without blaming `batch` for its children.

Priority is moderate. Workflow correctness appears more valuable than adding detailed telemetry immediately.

### Schedule errors

Six errors in 23 sampled `schedule` calls are high enough to investigate, but the sample does not prove that scheduling execution is slow. The likely actionable classes are request-schema validation and tolerant lifecycle behavior, such as cancellation of an already-terminal or absent schedule.

Recommended improvements:

- Validate required and mutually exclusive fields before dispatch.
- Return targeted repair messages rather than generic schema failures.
- Make cancellation idempotent where it is safe to do so.
- Distinguish model-invalid requests from runtime scheduler failures in telemetry.

Related deferred Bead: `jcode-cdp`.

## Workflow findings

### Add a lifecycle gate before declaring completion

The strongest recurring issue is not task execution. It is ambiguous final state. A session can finish implementation while leaving todos open, validation evidence unstated, a worktree unmerged, or cleanup ownership unclear.

This does not require a rigid workflow engine. A flexible lifecycle contract can be expressed as todo items created at the start of substantial work:

1. Define acceptance criteria.
2. Implement or investigate.
3. Validate each criterion through the real acceptance path where available.
4. Review risk and unresolved failures.
5. Land or explicitly hand off the result.
6. Reconcile todos, branch/worktree state, and the final response.

Before declaring completion, Jcode should verify that every required lifecycle item is terminal or explicitly deferred with an owner and reason.

Related high-level Bead: `jcode-u2v`.

### Medium risk should not block merging by default

The preferred policy is:

- **Low risk:** focused validation, then merge.
- **Medium risk:** stronger or broader validation, explicit rollback notes where useful, then merge if acceptance passes.
- **High risk:** require an approval or independent safety review when effects are destructive, production-adjacent, user-data-sensitive, or difficult to reverse.

“Medium risk” should increase evidence requirements. It should not routinely produce an implemented but stranded branch. A workflow should stop landing only when validation fails, ownership is unclear, the base has diverged, or the effect crosses an explicit safety boundary.

### Reconcile todos automatically

The model benefits from a final reconciliation prompt or lifecycle hook that compares:

- Requested outcomes.
- Current todo states.
- Validation receipts.
- Git/worktree status.
- Landing or handoff state.
- Remaining blockers and their owners.

This is more useful to the model than raw timing telemetry because it directly affects whether the user's outcome was delivered.

### Prefilter sessions before analysis

Eighteen of 30 files had no meaningful user prompt. Bulk insight workflows should deterministically exclude empty, setup-only, or abandoned sessions before spending model tokens.

Recommended pipeline:

1. Freeze or copy the selected session files to prevent live drift.
2. Extract metadata and counts deterministically.
3. Exclude sessions with no substantive user turn.
4. Rank remaining sessions by messages, tool calls, errors, compaction, retries, and unresolved todos.
5. Use a model only for sessions that cross a value threshold.

This would have reduced the analyzed set from 30 to at most 12 before semantic review, and likely to the eight sessions longer than three messages.

## Automatic compaction versus fresh-session handoff

Neither mechanism should replace the other.

| Situation | Prefer automatic compaction | Prefer fresh-session handoff |
| --- | --- | --- |
| Same tightly coupled task remains active | Yes | Usually no |
| The next step depends on detailed recent reasoning | Yes | Only if a sufficient durable brief exists |
| Verified milestone completed | Possible | Yes |
| Goal or subsystem materially changes | No | Yes |
| Context contains large amounts of obsolete exploration | Weak fit | Strong fit |
| Work must continue unattended without a clean boundary | Yes | Riskier if handoff generation is inconsistent |
| Multiple summaries have accumulated | Increasing degradation risk | Yes |
| Durable state exists in Git, Beads, specs, and todos | Optional | Strong fit |

### Advantages of compaction

- Automatic and operationally reliable.
- Preserves continuity during a single long-running process.
- Avoids depending on a perfectly generated handoff prompt.
- Retains recent context and a synthesized representation of older work.
- Better when the task has not reached a clean boundary.

### Disadvantages of compaction

- It is lossy.
- Repeated summary-on-summary compaction can omit constraints or amplify earlier mistakes.
- Obsolete exploration remains represented in the session.
- Compaction may occur near the context limit, when context quality is already degraded.
- It does not force reconciliation of todos, validation, or landing state.

### Advantages of fresh-session handoff

- Restores a clean context window and removes irrelevant history.
- Encourages explicit milestone boundaries and durable state.
- Reduces context pollution from failed hypotheses and noisy tool output.
- Works well when the next task can be expressed using a concise goal, files, decisions, risks, and next steps.

### Disadvantages of fresh-session handoff

- Quality depends on the handoff summary being complete and accurate.
- A transition at the wrong time can discard tacit reasoning still needed for the same task.
- Current model use of `session_transition` is new and historically inconsistent.
- It can create restart loops or duplicate prompts without deterministic lineage and state reconciliation.

### Recommended hybrid policy

Use **compaction-first within a milestone** and **handoff at milestone boundaries**.

A handoff should be suggested, not forced, when one or more conditions apply:

- A tracked milestone or Bead is complete.
- The next goal is materially different.
- The session has compacted more than once.
- Most earlier context is no longer relevant.
- The next phase has durable supporting artifacts.
- Repeated retries indicate context confusion.

Before transition, Jcode should construct a deterministic handoff envelope containing the goal, constraints, verified progress, decisions, unresolved risks, ordered next steps, relevant files, and durable tracking identifiers. The model can refine that envelope, but should not invent it from scratch.

Existing design: [FRESH_SESSION_HANDOFF.md](../proposals/FRESH_SESSION_HANDOFF.md).  
Related evaluation Bead: `jcode-wzo`.

## Memory assessment

Memory is useful for durable preferences, corrections, stable entities, and cross-session decisions. It is not a substitute for task state, acceptance criteria, or a handoff artifact.

The sample's 11 memory injections appeared only in two compacted sessions. That is too little evidence to conclude whether memory materially improved outcomes. The useful next measurement is not injection count. It is whether an injected memory changed behavior correctly or prevented a repeated correction.

Recommended separation:

- **Memory:** stable preferences and reusable facts.
- **Todo or Bead:** current obligations, lifecycle state, and acceptance evidence.
- **Handoff:** bounded continuation context for the next session.
- **Compaction:** lossy continuity for the same active session.

Memory retrieval should remain selective. Injecting every remotely related memory risks recreating context pollution.

## Cheap-model session summarization

Summarizing every session with a model is unlikely to be worthwhile. Most sampled sessions were empty or trivial, and deterministic extraction can handle counts, errors, tool usage, todo state, and compaction markers more cheaply and consistently.

Use a cheap or free strong model only when a session is:

- Long or compacted.
- Error-heavy.
- Repeated or apparently restarted.
- Associated with an unfinished tracked task.
- Selected for a weekly qualitative review.

Recommended pipeline:

1. Deterministic prefilter and feature extraction.
2. Redact or omit secrets and irrelevant raw content.
3. Summarize only selected sessions into a strict schema.
4. Aggregate themes across summaries separately.
5. Verify recommendations against source logs before creating Beads.

During the temporary free-model period through 2026-08-28, `jcode --profile ox-alpha` is suitable for bounded, privacy-aware qualitative summaries. It should not be trusted as the sole source for statistics, lifecycle state, or safety-sensitive recommendations. DeepSeek V4 Flash is another plausible fast summarizer, but model choice matters less than deterministic selection and verification.

## Browser research isolation

Future web research should use `websearch` or `webfetch` first. Jcode must not attach to, focus, alter, or add tabs to the user's existing Firefox session.

When a visible browser is necessary:

1. Launch a separate Firefox process with a dedicated temporary profile, for example `firefox --new-instance --profile <isolated-path> --new-window <url>`.
2. Target it to a separate named Hyprland workspace such as `jcode-research`.
3. Do not reuse the user's authenticated personal profile without explicit permission.
4. Close and clean up only the isolated research instance.

Recommended instruction placement, not implemented by this analysis:

- Put the concrete Firefox and Hyprland procedure in Jcode-local agent instructions.
- Put only a generic “never use an existing personal browser session without permission” boundary in global agent instructions.

## Prioritized recommendations

### High priority

1. Add model-visible lifecycle gates through todos and reconcile them before completion.
2. Change workflow policy so Medium risk strengthens validation rather than blocking a merge.
3. Freeze and prefilter session sets before running insight analysis.
4. Make schedule request validation clearer and safe cancellation idempotent where appropriate.

### Medium priority

5. Adopt compaction-first within milestones and handoff at clean milestone boundaries.
6. Generate handoff envelopes deterministically from todos, files, Git state, and durable tracking.
7. Summarize only selected long or compacted sessions with a cheap model.
8. Track parent/child tool spans so inclusive runtime is not mistaken for Jcode overhead.

### Lower priority

9. Produce p50, p95, and p99 queue, execution, serialization, inclusive, and exclusive timings after the span model is reliable.
10. Measure memory usefulness through corrected behavior and prevented repetition rather than injection counts.

## Scope and limitations

- No Jcode source or configuration was changed as part of the analysis.
- Build-speed optimization is intentionally excluded.
- The session snapshot was skewed and included many empty sessions.
- The findings describe observed patterns and proposed work, not completed product changes.
- Runtime and workflow recommendations should be implemented only through separately approved Beads.
