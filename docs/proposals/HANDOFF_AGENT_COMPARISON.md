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

The comparison is based on public documentation and repository material available on the research date. Features change quickly, so claims about absence mean “not found in the reviewed official material,” not proof that no plugin or undocumented workflow exists.
