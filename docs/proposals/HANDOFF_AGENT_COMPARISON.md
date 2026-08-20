# Coding-agent session handoff comparison

**Research date:** 2026-08-20

## Question

Which coding agents can deliberately create a new session and carry forward a concise, agent-authored technical briefing, rather than only reopening the old transcript or compacting it in place?

## Findings

| Agent | Native structured handoff to a new session | Relevant behavior |
| --- | --- | --- |
| **Jcode** | **Yes** | `session_transition` stages a fresh session after the current turn is persisted. Jcode records parent lineage, can carry todos, can auto-start with a continuation prompt, and supports Bead and relevant-file carryover. |
| **Oh My Pi (OMP)** | **Yes** | OMP documents a `SessionHandoff` flow that generates a structured handoff, then starts a fresh session seeded with it. Its broader session surface also includes checkpointing, rewind, memory, and compaction-aware recovery. |
| **Pi** | **Yes, through an extension** | `pi-handoff` adds `/handoff`: generate a structured document, review or edit it, then open a new session with the document injected as handoff context. It records the parent session and can auto-save before compaction. |
| **Claude Code** | **Not as a first-class handoff, based on the official session docs** | Supports `--continue`, `--resume`, `/branch`, `/clear`, and `/compact`. These reopen, fork, clear, or summarize sessions. Third-party handoff extensions provide the deliberate briefing-to-new-session workflow. |
| **Codex CLI** | **Not as a documented first-class handoff** | Supports saved-chat resumption through `codex resume`, session branching, worktrees, subagents, and context management. The official material reviewed did not show an agent-authored handoff document that automatically seeds a new session. |
| **Aider, Gemini CLI, Cline/Roo Code, OpenHands** | **Mostly resume, persistence, or checkpoints** | These tools preserve some form of task or conversation state, but the reviewed material did not establish the same deliberate structured handoff primitive as OMP, Pi's extension, or Jcode. |

## The important distinction

There are four related but different capabilities:

1. **Resume:** reopen the same transcript.
2. **Branch or fork:** copy a transcript and continue from a point.
3. **Compaction:** replace old context with a summary in the current session.
4. **Handoff:** deliberately create a structured technical state document, start a genuinely new session, and inject that document as its initial context.

Jcode is in category 4. This is stronger than ordinary resume or compaction because it creates a bounded context reset while preserving the information needed to continue safely.

## Handoff fields worth preserving

OMP and Pi's handoff model suggest that a useful handoff should cover:

- **Goal:** the outcome and current milestone.
- **Constraints and preferences:** user requirements, repository rules, safety boundaries, and non-goals.
- **Progress:** completed work, verification performed, and remaining work.
- **Key decisions:** chosen approach and important rejected alternatives.
- **Critical context:** architecture, APIs, errors, environment assumptions, and context or compaction state.
- **Unresolved risks and questions:** blockers, uncertainties, and likely failure modes.
- **Next steps:** exact actions in dependency order.
- **Relevant files:** paths and why each matters.
- **Durable tracking:** Bead, milestone, or other external state.
- **Lineage:** parent session when it is useful to the next agent. Jcode already stores parent-session tracking structurally.

Jcode also carries the active todo list separately. The handoff prompt should reconcile its narrative with those todos instead of duplicating or contradicting them.

## Jcode improvement

Jcode's built-in handoff guidance now explicitly asks the agent to produce this structured briefing. It also tells the agent to:

- treat the handoff prompt as the next session's source of truth;
- distinguish verified progress from intended work;
- include context or compaction state that cannot safely be inferred;
- state unresolved risks and questions;
- use exact, bounded next steps;
- preserve Bead, milestone, relevant-file, and todo continuity without stale duplication.

## Sources

- [Pi session documentation](https://pi.dev/docs/latest/sessions)
- [Pi handoff extension](https://github.com/jackice/pi-handoff)
- [Oh My Pi repository](https://github.com/can1357/oh-my-pi)
- [Claude Code session management](https://code.claude.com/docs/en/sessions)
- [Codex CLI documentation](https://developers.openai.com/codex/cli/features/)

## Ralph loops and context reset strategies

Ralph implementations are not all doing the same thing. The public examples reviewed fall into three useful designs.

### 1. Same-session stop-hook loop: Claude Code Ralph Wiggum

Anthropic's official `ralph-wiggum` plugin uses a Claude Code **Stop hook**. The user starts `/ralph-loop` once. When Claude tries to stop, the hook:

1. reads `.claude/ralph-loop.local.md`;
2. checks the iteration counter and optional completion promise;
3. extracts the latest assistant output from the transcript;
4. blocks the stop event;
5. feeds the original prompt back into the **same session**;
6. repeats until the promise matches or the iteration limit is reached.

This is deliberately not a handoff. The transcript remains the memory, while the repository and git history provide external persistence. It can automatically benefit from Claude's own compaction behavior, but the loop itself does not create a fresh context at each iteration. The official hook has corruption and missing-transcript escape hatches and atomically updates its iteration state, which are important safety details.

Source: [Anthropic Claude Code Ralph Wiggum plugin](https://github.com/anthropics/claude-code/tree/main/plugins/ralph-wiggum), especially [`stop-hook.sh`](https://raw.githubusercontent.com/anthropics/claude-code/main/plugins/ralph-wiggum/hooks/stop-hook.sh).

### 2. Fresh-process loop with file memory: snarktank/ralph

The popular `snarktank/ralph` implementation runs one story per iteration by spawning a **new Amp or Claude Code instance**. Its loop selects the highest-priority incomplete story, invokes the agent, expects tests and quality checks, commits successful work, marks the story complete in `prd.json`, and appends learnings to `progress.txt`.

Its memory boundary is explicit and file-based:

- `prd.json` stores story status and acceptance state;
- `progress.txt` stores append-only learnings;
- git history stores completed changes;
- `AGENTS.md` can be updated with durable repository discoveries.

This is structurally close to handoff, but the continuation message is mostly a fixed prompt plus files, not a model-generated handoff document. Its README recommends Amp's automatic handoff at 90% context for stories that exceed one window, so it combines fresh iterations with provider-level auto-handoff when needed.

Source: [snarktank/ralph](https://github.com/snarktank/ralph).

### 3. Minimal file-backed fresh loop: iannuttall/ralph

The archived `iannuttall/ralph` project uses a similar fresh-agent model and supports Codex, Claude, Droid, and OpenCode runners. Each iteration reads on-disk state and commits one story. State lives under `.ralph/`, including `progress.md`, guardrails, activity, errors, and run logs. It also supports stale `in_progress` story recovery after a timeout.

Source: [iannuttall/ralph](https://github.com/iannuttall/ralph).

### What this suggests

The common pattern is **not** “let one agent keep going forever.” Mature implementations put a boundary around iteration:

- same-session hooks are simple and cheap, but accumulate transcript state and depend on compaction;
- fresh-process loops reset model context, but require durable external state;
- robust systems use a bounded story, explicit acceptance checks, iteration limits, and a recovery path for stale or corrupted state.

Ralph systems generally do not use a rich generated handoff on every iteration. Instead, they rely on a small task ledger plus repository artifacts. That is efficient, but it can lose nuanced decisions, unresolved risks, and environment context unless the progress file or task artifact is maintained carefully.

## Handoff + Ralph + Beads design direction for Jcode

Jcode can combine the strongest parts of these approaches:

1. **Bound the unit of work:** let Beads or todo groups define the next milestone, not an unbounded global prompt.
2. **Use handoff as the context boundary:** when the group is complete or context pressure crosses a threshold, generate the structured briefing already required by Jcode's handoff guidance.
3. **Persist before reset:** save the Bead/todo state, relevant files, verification evidence, decisions, risks, and next steps before creating the child session.
4. **Start a fresh session:** carry the structured prompt, parent lineage, Bead ID, relevant files, and todos into the next session.
5. **Require acceptance before advancing:** the child must run the relevant checks and only then complete the next Bead/todo group.
6. **Keep escape hatches:** enforce maximum handoff depth and iteration count, stop on repeated failures, preserve a blocked state, and never treat a completion phrase alone as proof.

The likely best Jcode model is therefore a **bounded fresh-session Ralph loop**, rather than an infinite same-session loop:

```text
select next Bead/todo group
        ↓
work until acceptance or context threshold
        ↓
persist durable state and verification evidence
        ↓
generate structured handoff
        ↓
start child session with prompt + Bead + todos
        ↓
run acceptance checks
        ↓
complete, block, or hand off again within bounded limits
```

Automatic compaction remains useful inside one session, but it should not be the only reset mechanism. Compaction is optimized for preserving enough conversational continuity; an explicit handoff is optimized for changing the unit of work and making the next session's contract legible. Combining both gives a useful hierarchy: compact for short-term continuity, hand off for milestone boundaries, and use Beads/git/files as durable truth.

The comparison is based on public documentation and repository material available on the research date. Features change quickly, so claims about absence mean “not found in the reviewed official material,” not proof that no plugin or undocumented workflow exists.
