# Fresh-session handoff for long-running agent work

Status: research and design proposal. Nothing here is implemented yet.

Date: 2026-08-10

Bead: `jcode-zod`

## Executive summary

Jcode should support an agent-requested transition from the current session into a truly fresh session, optionally carrying a focused handoff prompt and immediately continuing work.

This is a good fit for long-running Beads and multi-task project work. The useful primitive is not a process restart. It is an atomic **fresh-session handoff**:

1. Finish and durably record the current unit of work.
2. Create a child session with an empty conversation history.
3. Preserve only operational metadata such as working directory, profile, model, permissions, and parent lineage.
4. Seed the child with a concise, inspectable handoff prompt or leave it blank.
5. Reattach the current client and optionally begin the next turn.

The strongest case is at a verified task boundary, such as after one Bead is completed and before the next Bead begins. It should not replace compaction during one tightly coupled task, and it should not replace swarm workers when independent work benefits from parallel execution.

This pattern is not unprecedented. Amp has an explicit Handoff feature and, since January 2026, lets the agent initiate it. Anthropic recommends starting a new session when starting a new task. Ralph implementations deliberately run one task per fresh agent instance while persisting state in Git and task files.

Recommendation: implement a first-class `handoff` or `session_transition` tool rather than overloading Jcode's existing `restart` terminology. Make it opt-in, safe-boundary based, lineage-preserving, and bounded against accidental infinite chains.

## Terminology

Several different operations can look like "restart" from the user's perspective but have different context behavior.

| Operation | Process changes | Session changes | Conversation retained | Intended use |
| --- | --- | --- | --- | --- |
| Process restart | Yes | Usually no | Yes | Upgrade or recover Jcode |
| Resume | Maybe | No | Yes | Continue an existing session |
| Fork or spawn | No | Yes | Usually yes | Branch existing work |
| Compaction | No | No | Lossy summary plus recent context | Continue the same task near a context limit |
| Fresh-session handoff | No | Yes | No, except an explicit handoff prompt | Begin the next bounded task cleanly |
| Subagent or swarm worker | No | Yes | Separate worker context | Parallelism, specialization, or context isolation |

The proposed feature is fresh-session handoff. Calling it `restart` would collide with process restart and could lead users and agents to assume the daemon or binary will restart.

## What Jcode supports today

Jcode has most of the infrastructure needed, but it does not currently expose the requested semantic as a model-callable tool.

### Process restart preserves sessions

The restart snapshot path captures connected sessions and restores them across a Jcode process replacement. See:

- `src/cli/commands/restart.rs`
- `crates/jcode-app-core/src/restart_snapshot.rs`

This is operational continuity, not context clearing. It intentionally keeps the same sessions alive.

### Sessions support lineage

`Session::create(parent_id, title)` records an optional parent session ID and starts with an empty message vector. See `crates/jcode-base/src/session.rs:830-885`.

This is a useful foundation for a fresh child session because lineage can be preserved without copying conversation history.

### The TUI can manually launch a blank session in another terminal

Jcode already supports the no-handoff half of the idea as a user action. The
TUI's `new_terminal` keybinding launches `jcode --fresh-spawn` in another
terminal, reusing the current working directory and server socket. The default
binding is Cmd+Shift+; on macOS and Alt+Shift+; elsewhere. See:

- `crates/jcode-tui/src/tui/app/helpers.rs:738-765`
- `crates/jcode-tui/src/tui/app/input.rs:1794-1807`
- `crates/jcode-base/src/config/default_file.rs:86-96`

This creates a genuinely blank session, but it is not a handoff workflow. It is
not model-callable, does not generate or carry a handoff prompt, opens another
terminal instead of replacing the current client's session, and does not
automatically continue work.

The TUI also has a next-prompt-to-new-session gesture using Super+Space or
Alt+Space. Despite the user-facing wording, that path calls
`launch_forked_session_local`, clones the current conversation, stages the typed
prompt, and opens the fork in another terminal. See
`crates/jcode-tui/src/tui/app/input.rs:1810-1853` and
`crates/jcode-tui/src/tui/app/commands_review.rs:700-726`. It is therefore a
prompted fork, not a clean-context handoff.

### The `schedule` tool can resume or spawn later

The current model-callable `schedule` tool supports targets `resume`, `spawn`, and `ambient`. See `crates/jcode-app-core/src/tool/ambient.rs:710-923`.

However, scheduled `spawn` is not fresh. `spawn_session_for_scheduled_item` copies the parent's:

- messages
- compaction state
- provider and model settings
- feature flags
- memory injections
- replay events

See `crates/jcode-app-core/src/ambient/runner.rs:409-480`.

That behavior makes `spawn` a delayed fork. It does not solve context reset.

### Compaction manages one continuing session

Jcode has compaction infrastructure and provider-visible compacted history. This is useful when the current task remains tightly coupled and rereading or restating all context would be wasteful. Compaction is inherently lossy and still carries a synthesized representation of the old session into the next request.

### Swarm workers provide isolated contexts

Swarm and subagent sessions can keep exploration or parallel work outside the root context. That is complementary to fresh-session handoff:

- Use handoff for sequential ownership of one workstream.
- Use swarm for independent parallel tasks or specialist isolation.

A sequential handoff avoids coordinator context, worker summaries, merge coordination, concurrent write conflicts, and the extra token usage of multiple simultaneously active agents.

## External findings

### Amp already implements almost exactly this feature

Amp removed compaction in favor of Handoff in October 2025. Its rationale was that repeated compaction encourages long, meandering threads and summary-on-summary degradation. Handoff asks for the next goal, analyzes the current thread, drafts a prompt and relevant-file list, and opens a fresh thread. The draft remains editable before submission.

In January 2026, Amp added agent-triggered handoff. A user can ask the current agent to hand off and continue in a new thread.

This is the closest known precedent to the proposed Jcode feature and directly validates the UX concept.

Sources:

- [Amp: Handoff (No More Compaction)](https://ampcode.com/news/handoff)
- [Amp: Handoff, Please](https://ampcode.com/news/ask-to-handoff)
- [Amp: Context Management](https://ampcode.com/guides/context-management)

### Anthropic recommends task-aligned session boundaries

Anthropic's April 2026 Claude Code guidance states a general rule of thumb: when starting a new task, start a new session. It distinguishes:

- continuing the same session
- rewinding
- clearing and writing a fresh brief
- compacting
- delegating to a subagent with a clean context

It describes `/clear` as more work than compaction but gives the user direct control over what is considered relevant. It also warns that automatic compaction may happen when the model is least capable because the session is already affected by context rot.

Claude Code does not appear to expose the same atomic model-triggered continuation UX as Amp. Its documented primitives nevertheless support the same context-management principle.

Sources:

- [Anthropic: Using Claude Code, session management and 1M context](https://claude.com/blog/using-claude-code-session-management-and-1m-context)
- [Claude Code subagents](https://code.claude.com/docs/en/sub-agents)
- [Claude context editing](https://platform.claude.com/docs/en/build-with-claude/context-editing)

### Codex favors same-chat continuity for one outcome and separate chats for independent work

OpenAI's long-running-work guidance recommends keeping related work in the same chat so the agent can reason about completion, while using separate chats for independent tasks and worktrees for parallel writers.

Its subagent documentation explicitly identifies context pollution and context rot as reasons to move noisy intermediate work outside the main thread. It also notes that subagents consume more tokens than a comparable single-agent run and that parallel write-heavy workflows add coordination overhead.

This supports a hybrid rule:

- Keep one coherent task in one session when continuity matters.
- Start a fresh session when the next task is independently specifiable.
- Use subagents when isolation or parallelism produces enough value to justify overhead.

Sources:

- [OpenAI: Long-running work](https://learn.chatgpt.com/docs/long-running-work.md)
- [OpenAI: Codex subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents.md)
- [OpenAI: Codex CLI](https://learn.chatgpt.com/docs/codex/cli.md)

### OpenCode uses durable checkpoint compaction

OpenCode V2 compaction replaces older active model context with a generated checkpoint while retaining durable historical messages. Its checkpoint records the objective, important details, completed and active work, blockers, next moves, and relevant files, plus a recent context tail.

This is a strong implementation of compaction, but it still solves continuation within a session rather than a deliberate task boundary. It is a useful model for the shape of a generated handoff artifact.

Source:

- [OpenCode: Compaction](https://opencode.ai/v2/docs/compaction)

### Ralph loops use fresh instances and durable external state

A common Ralph implementation runs one task per loop iteration in a fresh coding-agent instance. Memory persists through:

- Git history
- a structured task list such as `prd.json`
- an append-only progress file
- repository instructions such as `AGENTS.md`

The core insight is that the repository and task tracker are durable memory, while the model context is disposable working memory. Feedback loops and right-sized tasks are mandatory because mistakes otherwise compound across iterations.

Jcode's Beads integration is a better structured durable-state layer than a generic `progress.txt`. A fresh Jcode session could bootstrap from the active Bead, its comments, Git state, and a concise transition prompt.

Sources:

- [Geoffrey Huntley: everything is a ralph loop](https://ghuntley.com/loop/)
- [snarktank/ralph](https://github.com/snarktank/ralph)

### Long context capacity does not guarantee effective use

"Lost in the Middle" found that model performance can degrade as input context grows and that relevant information in the middle of a long context is often used less reliably than information near the beginning or end.

This paper is not a direct benchmark of modern coding agents, so it should not be treated as proof that every long coding session degrades by a specific amount. It does establish that a larger nominal context window does not remove the need to curate context.

Source:

- [Liu et al.: Lost in the Middle](https://arxiv.org/abs/2307.03172)

## Is this a good idea?

Yes, with a narrow contract and strong lifecycle guardrails.

### Expected benefits

#### Better task focus

A fresh session contains the next task, current repository state, project instructions, and selected durable context. It excludes stale debugging branches, rejected approaches, old logs, and unrelated tool output.

#### More predictable context composition

A handoff prompt is inspectable and can be edited. This is easier to reason about than an automatic compaction summary generated near a context limit.

#### Potentially lower inference cost

Every subsequent turn avoids resending the full prior conversation. There is an up-front cost to generating or writing the handoff, and the new session may reread some files. Savings therefore depend on task shape and must be benchmarked rather than assumed.

The largest likely savings are for multi-Bead sequences where each completed Bead produced substantial logs and tool output that are irrelevant to the next Bead.

#### Cleaner failure recovery

Each task boundary can correspond to a commit, Bead checkpoint, validation record, and parent-child session edge. A failed next task does not contaminate the completed task's transcript.

#### Less orchestration than sequential subagents

For a single sequential workstream, one agent handing off to itself is conceptually simpler than keeping a root coordinator alive while spawning one worker per task.

### Costs and risks

#### Handoff omission

The old agent may fail to include an important constraint. Generated handoffs are lossy just like compaction, although they are goal-directed and inspectable.

Mitigation: bootstrap from durable sources, not only prose. Include the Bead ID, Git commit, working directory, validation evidence, relevant files, and explicit next outcome.

#### Premature resets

An agent could reset during a tightly coupled debugging task and force the new session to rediscover expensive context.

Mitigation: instruct the model to hand off at verified task boundaries, not merely when context usage is high.

#### Infinite continuation and cost runaway

An autonomous agent could repeatedly start new sessions and continue indefinitely.

Mitigation: enforce a chain budget, surface cumulative cost and transition count, and default automatic continuation to user opt-in.

#### Unsafe lifecycle transitions

A tool cannot safely kill its own session before its tool result and terminal state are persisted. Background tasks, permission requests, queued user messages, or uncommitted results may still be active.

Mitigation: treat handoff as a server-managed transition admitted during the current turn and executed only at a safe drain boundary after the turn reaches a terminal event.

#### Ambiguous ownership

If both old and new sessions continue accepting work, two agents may edit the same checkout.

Mitigation: atomically mark the old session closed or handed off before activating the child. Keep the old transcript read-only and provide navigation in both directions.

#### User disorientation

Silently replacing the active session can make history appear to disappear.

Mitigation: show a visible transition event, parent link, child link, chain position, and a one-key way to inspect the source session.

## Recommended product contract

### Name

Prefer one of:

- `handoff`
- `session_transition`
- `fresh_session`

Recommendation: user-facing `/handoff`, model-callable `session_transition`.

Avoid `restart` because Jcode already uses restart for binary and server lifecycle continuity.

Product direction recorded 2026-08-10: support both manual handoff and
agent-initiated self-handoff. The model-callable capability should be enabled by
default, with a direct configuration switch to disable agent invocation without
removing the user's `/handoff` command.

### Tool schema

A minimal first version could be:

```json
{
  "action": "handoff | new",
  "prompt": "optional explicit prompt for the fresh session",
  "goal": "optional next-task goal used to generate a handoff",
  "bead_id": "optional durable task identifier",
  "relevant_files": ["optional/path"],
  "auto_start": true,
  "reason": "short model-visible explanation"
}
```

Semantics:

- `action=handoff` creates a fresh child and seeds it with a handoff prompt.
- `action=new` creates a blank fresh child. With `auto_start=false`, it only switches the client to an empty composer.
- `prompt` is used verbatim when provided.
- `goal` requests a generated handoff focused on the next outcome.
- `bead_id` makes the new session load the Bead and recent comments as durable startup context.
- `auto_start=true` submits the prompt after the transition. `false` leaves it editable.

The tool should return an accepted transition ID. It should not terminate the session inside tool execution.

### Safe transition sequence

1. Validate that fresh-session transitions are enabled and within chain limits.
2. Persist a `PendingSessionTransition` with an idempotency key.
3. Let the current tool result and assistant turn finish normally.
4. At the server's safe drain boundary, reject or settle conflicting queued input.
5. Mark the source session `HandedOff { child_session_id }` or closed with equivalent metadata.
6. Create `Session::create(Some(parent_id), title)` with no copied messages or compaction state.
7. Copy only operational metadata:
   - working directory
   - profile and provider selection
   - model and reasoning effort
   - permission mode
   - debug or self-dev mode when appropriate
   - client terminal placement metadata
8. Do not copy:
   - messages
   - compacted conversation history
   - provider conversation IDs
   - replay events derived from the old conversation
   - pending tool calls
   - pending approval requests
9. Build the startup prompt from the explicit prompt, generated handoff, Bead state, and selected files.
10. Atomically reattach the client to the child.
11. If `auto_start=true`, submit the startup prompt as the first user-visible turn.
12. Persist reciprocal parent and child navigation links.

### Handoff artifact

A generated handoff should be structured and bounded:

```text
Outcome completed:
- ...

Validation:
- command and result

Repository state:
- branch
- commit
- dirty files, if any

Next outcome:
- ...

Constraints and decisions:
- ...

Known failed approaches:
- ...

Relevant files:
- ...

Durable tracker:
- Bead ID and current status
```

The handoff should not paste large logs or full file contents. The new agent can read durable artifacts as needed.

### Startup instructions for the model

Jcode should advertise the tool with concise policy instructions similar to:

> You may invoke a fresh-session handoff yourself. Use it after a bounded task is completed and validated, when the next task can be stated independently. For the normal Bead workflow, finish the current Bead, persist its commit and validation evidence, select the next actionable Bead, then hand off to a fresh session with that Bead as the next goal. Before handing off, persist durable state in Git and the active task tracker. Prefer continuing the current session for tightly coupled work. Prefer compaction when the same task must continue but old tool output can be summarized. Prefer swarm workers when independent work benefits from parallel execution. Do not hand off solely because context usage is high. Do not create an unbounded handoff chain.

A configurable instruction field is useful for project-specific policy. For example:

```toml
[session_handoff]
enabled = true
agent_enabled = true
agent_requires_confirmation = false
max_chain_transitions = 8
auto_start = true
instructions_file = ".jcode/handoff-instructions.md"
```

The controls are intentionally independent:

- `enabled=false` disables all handoff behavior.
- `agent_enabled=false` removes the model-callable tool while retaining manual
  `/handoff` for the user.
- `agent_requires_confirmation=true` lets the model prepare a handoff but waits
  for the user before switching sessions.
- `auto_start=false` opens the fresh session with an editable draft instead of
  immediately submitting it.

Target default: manual and agent handoff enabled, no per-transition confirmation,
and automatic submission in the fresh session. Chain, token, cost, and wall-clock
budgets remain mandatory safety boundaries. An initial preview release may
temporarily require confirmation while telemetry and transition recovery are
validated, but that is a rollout gate rather than the intended steady-state UX.

## When to use each mechanism

| Situation | Best default |
| --- | --- |
| Same bug, same causal investigation | Continue current session |
| Same task, context is near limit | Directed compaction |
| Completed Bead, next Bead is separately specifiable | Fresh-session handoff |
| Plan completed, implementation begins | Fresh-session handoff is often useful |
| Noisy research needed for the current task | Read-only subagent |
| Independent tasks can run concurrently | Swarm with isolated worktrees |
| Sequential tasks share only repository state and tracker state | Fresh-session handoff |
| Fully autonomous backlog loop | Bounded handoff chain or external Ralph-style controller |

## Why this can be better than one subagent per Bead

For sequential work, a persistent root plus one worker per Bead leaves the root context accumulating:

- plans
- worker prompts
- worker summaries
- merge and validation state
- coordination messages

That can be appropriate when the root must supervise several workers or preserve a global decision thread. It is unnecessary when the workflow is simply:

1. Finish Bead A.
2. Persist and validate A.
3. Select Bead B.
4. Work on B alone.

A fresh-session handoff lets the durable tracker become the coordinator. Swarm remains better for genuine concurrency and independent judgment.

## Suggested implementation slices

### Slice 1: manual fresh handoff

- Add `/handoff <goal>`.
- Generate an editable handoff draft.
- Create an empty child session with copied operational metadata only.
- Switch the existing TUI client to the child.
- Preserve parent-child navigation.
- No model-callable tool and no automatic submission.

This validates session semantics and UX with low runaway risk.

### Slice 2: model-callable self-handoff

- Register `session_transition` for normal sessions by default.
- Let the model invoke the transition at a completed task or Bead boundary.
- Respect `agent_enabled`, `agent_requires_confirmation`, and `auto_start`.
- Make the steady-state defaults self-handoff enabled, no confirmation, and
  automatic continuation.
- Add Bead-aware bootstrap context.

### Slice 3: budgets and chain controls

- Enforce transition, token, cost, and wall-clock budgets.
- Stop on dirty or failed validation according to configured policy.
- Show chain progress and cumulative usage.
- Add recovery for a transition that was admitted but could not start its child
  turn.

### Slice 4: controller mode

- Let a durable initiative or Bead query select the next task.
- Use one fresh session per task.
- Add explicit completion and no-work stop conditions.
- Keep this as a bounded controller, not an accidental infinite loop.

## Validation and benchmark plan

Before claiming accuracy or cost improvements, compare three workflows on the same multi-Bead benchmark:

1. One long session with automatic compaction.
2. One root session with one sequential subagent per Bead.
3. One fresh session per Bead with structured handoff.

Measure:

- total input and output tokens
- wall-clock time
- reread and rediscovery tool calls
- first-pass test success
- regressions discovered after task boundaries
- human corrections required
- handoff omissions
- context size at task start and completion
- total model cost

Use at least one tightly coupled task sequence and one loosely coupled backlog. The expected result is not that handoff always wins. The hypothesis is that it wins when task boundaries are real and durable repository state contains most of the needed continuity.

## Conclusion

The idea is sound and already validated by Amp's product direction, Anthropic's task-aligned session guidance, and Ralph-style fresh-instance loops.

Jcode's differentiator could be a Bead-aware, server-atomic version that works inside the current TUI and preserves session lineage. The key design choice is to make conversation context disposable while preserving durable engineering state. The feature should begin as an editable manual handoff, then graduate to model-callable and automatic continuation only after safe-boundary behavior and cost controls are proven.
