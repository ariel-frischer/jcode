# Context compaction in Jcode and the case for a Jev hybrid

**Date:** 2026-09-20  
**Status:** Backlog research note. The Jev hybrid is an unapproved idea only. No implementation, experiment, or product change is authorized.

## Executive conclusion

Jcode currently follows the standard recursive-summary pattern used by many coding agents: summarize older history, preserve a recent high-fidelity tail, and prepend the summary to later requests. It also supports OpenAI's provider-native encrypted compaction artifact.

Jev should **not** replace the generative summarizer. Jev is a judgment model and cannot produce a continuation summary, reconcile a trajectory, or synthesize tool evidence. A promising use is a **preserve-only salience coprocessor**: embeddings shortlist older candidate chunks, Jev identifies high-confidence requirements, decisions, unresolved failures, and critical tool evidence, and those excerpts are pinned verbatim for the normal summarizer. Jev should initially be allowed to add items to the keep set, not decide what is permanently discarded.

The existing local Jev relevance benchmark shows attractive cost and latency but insufficient recall for destructive filtering: precision 56.5%, recall 60.5%, F1 58.4%, mean latency 0.32 seconds, and $0.00280 total across 40 queries. It was about 4.1 times cheaper than the Luna judging calls used as labels, but it missed 17 of 43 Luna-selected memories. This benchmark is directional only and did not test compaction.

## How Jcode compaction works

The primary implementation is in:

- `crates/jcode-compaction-core/src/lib.rs`
- `crates/jcode-base/src/compaction.rs`
- `crates/jcode-app-core/src/agent/compaction.rs`
- `crates/jcode-provider-openai-runtime/src/openai_provider_impl.rs`

### Trigger and retention policy

- Default context budget: 200,000 tokens.
- Normal compaction threshold: 80%.
- Critical synchronous hard-compaction threshold: 95%.
- Recent messages kept verbatim: 10.
- Emergency minimum retained messages: 2.
- Reactive mode is the default.
- Proactive mode projects token growth using an EWMA.
- Semantic mode combines projected growth with embedding-based topic-shift and relevance checks.
- Semantic/proactive checks have a default 40% context floor and a ten-turn cooldown.

### Normal summary flow

1. Jcode calculates a cutoff, normally retaining the latest ten messages.
2. It moves the cutoff backward when necessary to avoid separating a tool result from its tool call.
3. Older messages, together with any previous summary, are submitted for summarization.
4. The resulting summary replaces the compacted prefix for future provider requests.
5. The summary is sent as a synthetic user message followed by the recent active messages.
6. Later compactions recursively summarize the previous summary plus newly aged history.

The summary prompt asks for context, completed work, current state, next work, and user preferences. This is a useful general continuation shape, but it is not a typed state contract and has no retained-fact verification stage.

### Tool and reasoning treatment

The compaction transcript includes:

- user and assistant text;
- tool names and complete serialized tool inputs;
- only the first 500 characters of each tool result;
- image placeholders;
- no reasoning or thinking traces.

This exposes a likely quality gap. Important information can exist near the end of a large tool result, particularly compiler output, test summaries, stack traces, diffs, or search results. Tool-only messages also receive weaker treatment in semantic retention because per-message semantic text is derived from text blocks rather than full tool results.

### Emergency behavior

At critical context pressure, Jcode can drop older messages synchronously and construct a lightweight emergency summary. If the remaining active turns are still too large, it truncates large tool outputs and images. This prioritizes request survival over full semantic fidelity.

### OpenAI native compaction

When explicit native compaction is enabled, Jcode calls the OpenAI responses compact endpoint. The response contains opaque encrypted compaction content rather than a readable summary. Jcode persists and replays that artifact, with a text fallback when the encrypted payload is too large. This can preserve provider-specific internal state but is less inspectable and harder to evaluate for factual retention.

## Common approaches in coding agents

Agent implementations generally combine several mechanisms:

| Mechanism | Purpose | Principal tradeoff |
| --- | --- | --- |
| Recursive summary plus recent tail | Preserve continuity within one session | Summaries can omit or distort details |
| Tool-result pruning or observation masking | Remove high-volume, low-value observations | A decisive old result may be lost |
| Provider-native compaction | Let the model provider encode continuation state | Opaque and provider-specific |
| Structured fresh-session handoff | Cross milestone or topic boundaries deliberately | Rediscovery and startup cost |
| External durable state | Preserve task, effects, validation, and ownership outside the transcript | Requires disciplined artifact maintenance |

Examples include bounded chat-history summarization in Aider, compaction or continuation summaries in OpenCode/Cline-style agents, observation masking and summarizing condensers in OpenHands, provider-native compaction in OpenAI integrations, and checkpoint-based fresh sessions in long-running agent harnesses.

The robust architecture is not transcript compression alone. It separates:

1. stable task and acceptance state;
2. durable effect and validation evidence;
3. compressed long-term conversational context;
4. selected verbatim evidence;
5. a recent high-fidelity interaction tail.

## Accuracy and cost

### General compression evidence

The local long-running-agent research records the following directional results from *Context as a Tool*:

- append-only ReAct: 49.8% solved;
- threshold compression: 53.8%;
- structured milestone-aware compression: 57.6%.

This is evidence that compression can improve task accuracy by reducing noise and attention dilution. It is not a direct Jcode benchmark. The important signal is that structured preservation outperformed token-threshold compression by another 3.8 percentage points.

Jcode currently records operational metrics such as pre/post tokens, messages compacted, summary size, duration, and tokens saved. It does not yet measure retained-fact recall, stale-fact carryover, first-action correctness, or accepted task completion after compaction.

### Local Jev relevance benchmark

The existing benchmark under `jev_bench/` used local embeddings to produce candidate memories, then asked Jev to make parallel binary relevance judgments. Results recalculated from the stored artifacts:

| Metric | Result |
| --- | ---: |
| Queries | 40 |
| Precision versus Luna labels | 56.5% |
| Recall versus Luna labels | 60.5% |
| F1 | 58.4% |
| True positives | 26 |
| False positives | 20 |
| False negatives | 17 |
| Jev total cost | $0.00280 |
| Jev mean latency | 0.32 s |
| Luna label-call cost | $0.01151 |
| Luna/Jev cost ratio | 4.1x |

Limitations:

- Luna labels are not objective ground truth.
- The benchmark evaluated memory relevance, not summary quality or downstream coding success.
- The candidate set was already narrowed by embeddings.
- The threshold was a simple Noul probability of 0.5 and was not calibrated on a held-out set.
- Jev's state limit prevents judging an entire 100K-200K transcript in one call; long sessions require chunking and retrieval first.

Therefore, the benchmark supports Jev as a cheap secondary judge, but not as the sole destructive filter.

### Cost model

Normal compaction has two opposing costs:

- an immediate summarization request and loss of some prompt-cache continuity;
- lower input-token cost and less attention noise on every later turn.

The break-even point depends on the model's input price, cache pricing, number of remaining turns, and summary quality. For a long continuation, removing tens of thousands of repeated input tokens generally dominates the one-time summary call. For a session that ends immediately after compaction, the compaction call may produce no financial benefit.

A Jev stage only saves money if it reduces the text sent to the expensive summarizer or reduces future retained context. If the normal summarizer still receives the complete transcript, Jev adds a small cost and should be justified by improved retention quality rather than token savings.

## Recommended hybrid design

### Preserve-only first version

```text
older transcript
    -> deterministic normalization and chunking
    -> local embedding shortlist
    -> Jev atomic salience judgments
    -> high-confidence excerpts pinned as must-preserve evidence
    -> normal generative summary
    -> summary + pinned evidence + recent ten-message tail
```

Useful atomic judgments include:

- explicit user requirement or non-goal;
- architecture or API decision;
- rejected alternative whose rationale still matters;
- unresolved error, blocker, or failed strategy;
- validation result needed to establish current state;
- tool output that should remain verbatim;
- fact that supersedes or contradicts earlier state.

Code should own token budgets, thresholds, chunking, tool-pair integrity, and uncertainty policy. Low-confidence items should be retained or routed to the generative summarizer, not dropped.

### Candidate construction

Do not send arbitrary raw transcript windows. Construct bounded records containing:

- role and timestamp or turn index;
- user or assistant text;
- tool name and normalized arguments;
- exit status;
- the beginning and end of large tool results;
- extracted errors, file paths, symbols, tests, and commands;
- links to durable todos, commits, or validation artifacts.

This specifically addresses the current 500-character tool-result truncation weakness.

### Uses beyond message retention

Jev may also be useful for a bounded semantic-boundary judgment:

- same milestone versus new milestone;
- active progress versus repeated failed strategy;
- compaction versus structured handoff;
- continuation versus human escalation.

Token arithmetic and hard safety thresholds should remain deterministic.

## Evaluation proposal

Compare the current implementation against a preserve-only Jev hybrid at identical compaction boundaries.

Primary outcomes:

- accepted task completion at a fixed budget;
- retained critical-fact recall;
- first useful action and first-action correctness after compaction;
- stale or superseded facts carried forward;
- cost per accepted task.

Efficiency outcomes:

- compaction-call input/output tokens;
- subsequent-turn tokens and cache accounting;
- Jev calls, latency, and spend;
- repository rediscovery calls;
- total wall time.

Critical edge cases:

- essential evidence at the end of a long tool result;
- linked tool call/result pairs across the proposed cutoff;
- multiple recursive compactions;
- topic changes and superseded requirements;
- provider-native encrypted state becoming oversized;
- hard compaction at 95%;
- a single recent tool result large enough to exceed the budget;
- summary generation failure or stale asynchronous completion.

A promising result should improve retained-fact recall without reducing accepted completion. Token savings alone are insufficient if acceptance falls.

## Validation performed for this note

- Confirmed the installed public CLI resolves to Jcode `v0.83.899-dev (ef7f3b92f)` with the OpenAI provider.
- Ran `cargo test -p jcode-base compaction -- --nocapture` through the coordinated self-development test path. It completed successfully with exit code 0.
- The focused suite covers normal API message construction, summaries prepended to active history, 80%/95% threshold behavior, hard compaction, repeated hard compaction, recent-tail preservation, tool-pair integrity, stale background compaction, persisted-state restoration, payload reduction, and reported token savings.
- A live provider compaction was not run because it would invoke an external model and potentially mutate a real session. That leaves provider-reported cost and downstream live-model quality unverified.

## Backlog recommendation

The Jev hybrid described here is a backlog idea, not an approved direction. Do not implement it, start paid experiments, or change compaction defaults without Ariel's explicit approval.

If it is approved later, do not begin with Jev-only compaction or Jev-authorized deletion. Start with an opt-in preserve-only audit mode and a replay benchmark. The hypothesized value is better preservation of critical requirements and tool evidence at very low marginal judgment cost, not replacement of the generative summary call.

## Related material

- [`long-running-agent-workflows-2026-08-21.md`](long-running-agent-workflows-2026-08-21.md)
- [`../MEMORY_ARCHITECTURE.md`](../MEMORY_ARCHITECTURE.md)
- [`../../jev_bench/bench_run.py`](../../jev_bench/bench_run.py)
- [`../../jev_bench/score.py`](../../jev_bench/score.py)
