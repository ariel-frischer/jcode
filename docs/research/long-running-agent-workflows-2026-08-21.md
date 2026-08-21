# Long-running coding-agent work: handoff, compaction, Ralph, and durable workflow control

**Research date:** 2026-08-21  
**Tracking:** `jcode-wzo`  
**Status:** Research artifact and benchmark proposal. No live paid-model experiment has been run.

## Executive summary

Ralph and Jcode's fresh-session handoff are related, but they are not the same primitive.

- **Compaction** is a lossy memory operation inside one ongoing session. It preserves conversational continuity and is usually the cheapest default while the agent is still working on one coherent milestone.
- **Handoff** is a deliberate context boundary. It starts a genuinely fresh session and carries a compact, inspectable technical contract. It is best at semantic boundaries, topic changes, role changes, repeated correction loops, or after a milestone is verified.
- **Ralph** is primarily an iteration and retry policy. Its implementations vary:
  - the official Claude Code plugin loops in the **same session** through a Stop hook and feeds the same prompt back;
  - fresh-process variants start a new agent each iteration and use files, git, and a task ledger as memory.
- **Beads** are a stronger execution ledger than a typical Ralph PRD. A PRD usually provides ordered stories and pass flags. A Bead can additionally carry dependencies, ownership, acceptance criteria, comments, labels, durable evidence, and relationships to other work.
- **Locus** should be the durable controller and delivery authority. Jcode should be the inner coding worker. Locus owns run identity, leases, worktrees, retries, effect receipts, validation, integration, and recovery. Jcode owns context management, tool execution, compaction, and session handoff.

The practical recommendation is a **bounded hybrid**:

```text
Locus run manifest + Bead acceptance
    -> Jcode works one bounded milestone
    -> compact within the milestone when context pressure rises
    -> persist files, todos, evidence, decisions, and process ownership
    -> hand off at a semantic boundary or after repeated compaction/failure
    -> deterministic validation and independent review
    -> complete, block, or retry the bounded unit
```

Do not build an infinite same-session loop as the primary long-running architecture. Use Ralph-like iteration as a bounded retry policy around verified work units, not as the source of truth for memory, correctness, process ownership, or completion.

The most important missing evidence is a controlled A/B/C comparison of **compaction-only**, **handoff-only**, and **hybrid** strategies with equivalent checkpoint content. The literature supports structured context management and staged work, but it does not yet establish that a fresh Jcode session is always better than a good compaction summary for the same checkpoint.

## 1. The conceptual model

A long-running agent has four separate problems. Conflating them produces fragile designs.

| Problem | Question | Best owner |
| --- | --- | --- |
| Memory | What information must survive the next context window? | Jcode plus repository artifacts |
| Progress | Which bounded unit should be attempted next? | Bead or Locus task ledger |
| Control | When should the agent continue, reset, retry, pause, or ask for help? | Locus policy plus Jcode thresholds |
| Effects | What actually happened to files, processes, Git, tests, and providers? | Locus receipts and deterministic checks |

A transcript is not a durable execution record. A model-generated handoff is not a completion proof. A PRD is not a lease or an effect receipt. A workflow graph is not automatically a correct recovery system.

A reliable system separates:

1. **Context state:** what the next model needs to reason well.
2. **Task state:** what remains to be done and who owns it.
3. **Effect state:** what has actually happened and how it was verified.
4. **Process state:** which long-running commands, sockets, process groups, logs, and cleanup obligations exist.

For the `jcode-wzo` question, the key distinction is therefore not simply “Ralph versus handoff.” It is:

```text
same transcript
    vs. compacted transcript
    vs. structured fresh-session briefing
    vs. durable external task/effect ledger
```

These can be combined, and usually should be.

## 2. What Ralph actually means

“Ralph” is an overloaded label. Treating all Ralph loops as one design leads to incorrect comparisons.

### 2.1 Official Claude Code Ralph Wiggum plugin

The official plugin implements Ralph with a Stop hook. The user starts a loop once. When Claude tries to stop, the hook:

1. reads `.claude/ralph-loop.local.md`;
2. validates the iteration and maximum-iteration fields;
3. reads the latest assistant output from the transcript;
4. checks an optional completion promise;
5. blocks the stop event when work is incomplete;
6. feeds the **same prompt** back into the current session;
7. repeats until the promise matches or the iteration limit is reached.

This is **not a fresh-session handoff**. The transcript remains the primary context. Files and Git provide external persistence, and the loop may benefit from the provider's own compaction. The plugin does have useful safety details: numeric state validation, transcript corruption escape hatches, atomic state-file replacement, and a maximum-iteration guard.

The design is attractive when:

- the task has a clear automated check;
- another iteration can cheaply inspect the current repository state;
- the same prompt remains valid;
- the task is not dependent on a nuanced design decision;
- retrying is cheaper than coordinating a new session.

Its weaknesses are equally important:

- the transcript grows or depends on compaction;
- the prompt does not evolve with new discoveries;
- a completion phrase is weaker than an acceptance receipt;
- the model can repeat a bad strategy;
- the loop does not inherently distinguish a transient failure from a permanently blocked task;
- process ownership and orphan cleanup are outside the loop unless explicitly implemented.

### 2.2 Fresh-process Ralph variants

The `snarktank/ralph` implementation is materially different. Each iteration starts a fresh Amp or Claude Code instance. The loop selects the highest-priority incomplete story, asks the agent to implement one story, runs quality checks, commits successful changes, marks the story passed in `prd.json`, appends learnings to `progress.txt`, and repeats up to a limit.

Its durable memory is file-based:

- `prd.json`: story status, priority, and acceptance state;
- `progress.txt`: append-only learnings and patterns;
- Git history: completed changes;
- `AGENTS.md`: reusable repository discoveries.

This is structurally close to Jcode handoff, but the continuation mechanism is usually a fixed prompt plus files rather than a generated technical briefing. It is efficient when repository artifacts are sufficient. It can lose subtle decisions, unresolved risks, environment assumptions, and reasons for rejecting alternatives unless those are deliberately written down.

The archived `iannuttall/ralph` design follows the same broad pattern and adds explicit run logs, errors, guardrails, and stale in-progress story recovery.

### 2.3 Ralph versus handoff in one sentence

> **Ralph answers “should I try the bounded unit again?” Handoff answers “what is the smallest trustworthy context with which the next session can continue?”**

Ralph can use handoff. Handoff can be triggered by a Ralph-style iteration boundary. They are orthogonal dimensions, not competing products.

## 3. Jcode's current design and the `jcode-wzo` problem

The existing Jcode implementation already has the major pieces needed for a hybrid policy.

### 3.1 Current compaction behavior

The source-level `CompactionConfig` defines:

- reactive, proactive, or semantic compaction modes;
- token-growth lookahead and EWMA smoothing;
- `proactive_floor = 0.40` by default;
- a minimum sample count;
- a stall window;
- a ten-turn minimum interval between compactions by default;
- semantic topic-shift and relevance thresholds.

The built-in compaction mode is reactive by default, but proactive and semantic modes exist. Compaction is therefore already a first-class continuity mechanism, not a missing feature.

### 3.2 Current fresh-session handoff behavior

The built-in handoff policy has these source defaults:

| Field | Default |
| --- | ---: |
| `enabled` | `true` |
| `agent_enabled` | `true` |
| `agent_requires_confirmation` | `false` |
| `auto_start` | `true` |
| `max_chain_transitions` | `8` |
| `poke_enabled` | `false` |
| `poke_soft_floor` | `0.40` |
| `poke_hard_threshold` | `0.70` |
| `copy_todos` | `true` |

The effective local profile or configuration can enable advisory pokes even though the source default is off. That distinction matters for experiments. Record the resolved policy in every run rather than inferring it from source defaults.

The handoff guidance explicitly asks the agent to preserve:

- goal and current milestone;
- constraints, preferences, safety boundaries, and non-goals;
- verified progress and remaining work;
- key decisions and rejected alternatives;
- critical architecture, API, error, environment, and compaction context;
- unresolved risks and questions;
- exact next steps;
- relevant files;
- Bead or other durable tracking identifiers.

The implementation creates a child session with empty conversation history, preserves operational metadata and parent lineage, optionally carries the compaction state and todo list, and enforces a bounded parent chain. Generated handoff prompts are capped at 32 KiB, with recent source-session messages capped at 8 KiB and four messages.

That is more expressive than a fixed Ralph prompt. It is also more expensive if generated too frequently. A handoff summary should be a milestone artifact, not a second summary after every turn.

### 3.3 The current threshold interaction

The interesting policy conflict is visible in the defaults:

```text
compaction proactive floor: 0.40
handoff advisory soft floor: 0.40
handoff advisory hard threshold: 0.70
```

If the effective configuration enables handoff pokes, a completed todo boundary can become handoff-worthy at the same context level at which proactive compaction is allowed to start. The compaction implementation already suppresses handoff pokes while compaction is active, but a token-only threshold still creates a race in policy terms:

1. context reaches 40%;
2. compaction may be eligible;
3. a todo group closes;
4. handoff becomes noteworthy;
5. the system must decide whether to compact, hand off, or do both.

The simplest policy is not to choose one global threshold. It is to prioritize **semantic state**:

- same milestone and active process: compact first;
- completed milestone or topic change: hand off;
- repeated compactions without progress: hand off or ask for help;
- active background process: preserve ownership and prefer a resumable checkpoint over a blind session reset;
- hard context pressure: make the current unit safe, then hand off.

### 3.4 What `jcode-wzo` should evaluate

The Bead should not ask “which mechanism wins?” in the abstract. It should ask:

1. At an equivalent checkpoint, does a fresh context reduce stale-goal and anchoring errors compared with compaction?
2. Does the quality gain justify repeated repository re-discovery and prompt overhead?
3. Which trigger signals predict a useful handoff better than raw token usage?
4. Does the hybrid policy reduce repeated corrections without increasing unnecessary resets?
5. Can long-running process ownership survive a reset without orphaning or losing the user-visible run?

## 4. Beads, PRDs, and Locus

### 4.1 PRD versus Bead

A Ralph PRD is usually a product/task backlog. It is good at expressing:

- a branch or run target;
- ordered user stories;
- priority;
- pass/fail state;
- story-level acceptance text.

A Bead can express all of those and add execution-specific information:

- dependency edges and readiness;
- owner and assignee;
- status and workflow labels;
- design and non-goals;
- comments and dated evidence;
- related work and blockers;
- acceptance criteria tied to actual checks;
- durable handoff and milestone information.

Therefore:

> A Bead is not merely Jcode's PRD format. It is closer to a task ledger plus ownership, dependency, and evidence contract.

Ralph's `prd.json` remains useful as a portable local input format. Locus should be the authority when a run has dependencies, leases, worktrees, integration, or effect receipts.

### 4.2 Jcode versus Locus

The systems should have different authority boundaries:

| Responsibility | Jcode | Locus |
| --- | --- | --- |
| Provider conversation and tool calls | Yes | Adapter/driver only |
| Same-session compaction | Yes | No |
| Fresh-session handoff | Yes | May request at stage boundary |
| User-visible interactive steering | Yes | Optional controller surface |
| Bead/task readiness | Reads current task | Owns selection and dependency order |
| Worktree ownership | Uses assigned worktree | Owns isolation and lifecycle |
| Retry policy | Local turn recovery | Run/stage retry and lease policy |
| Git integration | Produces changes | Owns merge, conflict, and delivery evidence |
| Process groups and cleanup | Must expose resumable state | Owns run manifest, cleanup, and recovery |
| Completion proof | Reports checks | Verifies receipts and acceptance |

Locus's existing research is right to keep its durable ledger rather than replace it with a generic workflow dependency. The difficult part is not drawing a graph. The difficult part is recovering and proving effects after interruption.

## 5. Evidence from current research and systems

### 5.1 Primary evidence directly about long-running coding agents

#### Anthropic: Effective harnesses for long-running agents

Anthropic's November 2025 engineering article describes a two-part harness:

1. an **initializer agent** that prepares the environment, feature list, progress file, and initial Git commit;
2. a **coding agent** that makes incremental progress in each fresh session and leaves the repository in a clean state.

The article reports two recurring failure modes:

- agents try to one-shot too much, run out of context mid-feature, and leave the next session guessing;
- later agents see partial progress and prematurely declare the project complete.

The proposed mitigation is not “make the context window infinite.” It is a feature ledger, progress artifacts, incremental work, and a clean end-of-session contract. This is strong support for the Jcode plus Bead/Locus direction.

Source: <https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents>

#### Context as a Tool, arXiv:2512.22087

This is the most directly relevant paper found for the current question. CAT treats context management as a callable tool and separates:

- stable task semantics;
- condensed long-term memory;
- high-fidelity short-term interaction history.

The paper reports **57.6% solved on SWE-Bench Verified** for its SWE-Compressor model, outperforming append-only ReAct and static compression baselines under a bounded context budget. The local Jcode Bead records the comparison as:

- append-only ReAct: **49.8%**;
- threshold compression: **53.8%**;
- structured milestone-aware compression: **57.6%**.

These are useful directional numbers, not a direct handoff A/B result. The model, training procedure, task setup, and baselines are not the same as Jcode. The relevant design lesson is that compression should preserve stable task semantics and actionable milestone state, not merely shorten text.

Source: <https://arxiv.org/abs/2512.22087>

#### SWE-MeM, arXiv:2606.28434

SWE-MeM is a newer adaptive-memory result. It lets the agent decide when, what, and how to compress based on trajectory state, task progress, and remaining context budget. Its abstract reports **43.4%** and **60.2%** resolve rates on SWE-Bench Verified with 4B and 30B models, respectively, outperforming its memory-management baselines in performance and efficiency.

The relevant lesson is not that Jcode should train a memory policy. It is that the trigger should combine:

- context pressure;
- task progress;
- trajectory state;
- the semantic value of the information being discarded.

Source: <https://arxiv.org/abs/2606.28434>

#### CoMem, arXiv:2605.30842

CoMem decouples memory management from the main agent workflow and runs summarization asynchronously. Its abstract reports a **1.4x latency improvement** over vanilla long-context solutions on SWE-Bench Verified while preserving most performance. It is marked as work in progress, so the number should be treated as preliminary.

The architecture is relevant to Jcode because asynchronous memory work already exists locally. The practical opportunity is to ensure that memory preparation, embedding, or handoff-draft generation never blocks the productive coding turn unless a hard safety boundary requires it.

Source: <https://arxiv.org/abs/2605.30842>

#### Adaptive multi-agent scaffolding, arXiv:2606.25514

The icat-agent paper replaces shared context with synchronous event-based message passing and adapts its workflow based on issue quality. Its abstract reports improvements of **3.6-8.4 percentage points on SWE-Bench Verified** and **6.3-18.5 points on SWE-Bench Pro**, plus **$1.18 lower average cost per instance** than a multi-agent Claude Code baseline. It reports **67.4% on SWE-Bench Pro** for icat-agent plus GPT-5.4-xhigh.

This is not evidence that more agents are always better. It supports a narrower claim: explicit event contracts and role-specific context can beat one shared context when the task has genuine role separation. For Jcode/Locus, a durable artifact between stages is safer than passing a whole transcript between agents.

Source: <https://arxiv.org/abs/2606.25514>

### 5.2 Systems evidence relevant to active processes and cost

#### AgentSysBench, arXiv:2608.15127

AgentSysBench is a current systems-level benchmark for agentic workloads. Across ten applications and production traces, its abstract reports:

- non-LLM components dominate latency in **5 of 10** applications;
- sandbox working sets peak at **28 GB per session**;
- task latencies diverge by up to **32x** across resource types;
- sessions can hold state idle for minutes to hours;
- tool schemas and observations impose a control-plane tax;
- caching removed **35.2% of redundant search calls** and saved **19.3% of aggregate search latency** in one exploration.

This matters for `jcode-wzo`: token cost is only one part of long-run cost. Repeated repository scans, process startup, worktree setup, test execution, idle resources, and lost background work can dominate. A fresh-session policy must measure end-to-end run cost, not just model tokens.

Source: <https://arxiv.org/abs/2608.15127>

#### BENCH2ROBUST, arXiv:2608.11977

This benchmark injects transient, persistent, and silent tool failures and tests whether agents retry, switch paths, or abstain. The paper reports up to **16.8 percentage points** of robustness improvement from structured runtime recovery context without retraining, and **40.8-45.5%** under injected failures when combined with training interventions.

The direct takeaway is that a long-running loop should not equate “retry” with “repeat the same action.” It needs typed failure handling:

```text
retry same path | refresh context | switch strategy | ask human | abstain
```

Source: <https://arxiv.org/abs/2608.11977>

### 5.3 Durable-execution systems

LangGraph documents checkpointers for thread-scoped graph state, recovery, interruption, human-in-the-loop, and time travel, plus stores for cross-thread application data. Temporal uses an event history as the source of truth and replays deterministic workflow code, recording external effects as activities so they are not repeated during replay.

These systems validate the general split:

- state snapshots or summaries are useful for reasoning continuity;
- event/effect history is needed for safe recovery;
- deterministic control code should own retries and replay;
- external side effects require idempotency or receipts.

They do not imply that Jcode should adopt a general workflow engine. Locus already owns much of the effect ledger and recovery boundary that a coding factory actually needs.

Sources:

- <https://docs.langchain.com/oss/python/langgraph/durable-execution>
- <https://docs.langchain.com/oss/python/langgraph/persistence>
- <https://docs.temporal.io/workflows>

### 5.4 Additional long-context evidence

The independent research also surfaced three useful guardrails against overinterpreting a large context window:

- **RULER** evaluated 17 long-context models across retrieval, multi-hop tracing, aggregation, and question answering. Almost all degraded as context length and task complexity increased, and only about half maintained satisfactory performance at 32K despite claiming at least 32K context. This supports treating context capacity as an attention budget, not a reliable memory store. Source: <https://arxiv.org/html/2404.06654>.
- **Lost in the Middle** found position-dependent degradation: relevant information was often used best at the beginning or end of long contexts and less reliably in the middle. This supports structured summaries that put stable task state and next actions in predictable positions. Source: <https://aclanthology.org/2024.tacl-1.9/>.
- Current agent products increasingly expose compaction as the normal lifecycle. Claude Code documents automatic compaction and recommends clearing between unrelated tasks; Anthropic's API describes compaction as appropriate for long tool-heavy tasks; OpenAI documents server-side compaction as a quality, cost, and latency mechanism; Cursor exposes a `preCompact` hook; Aider exposes bounded chat-history summarization. These sources establish that compaction is the common continuity primitive. They do **not** establish that it is sufficient for semantic phase changes.

This strengthens the compaction-first conclusion but does not eliminate the case for handoff. Compaction solves “make the current context smaller.” Handoff solves “stop inheriting this trajectory and start the next phase from a deliberate contract.”

### 5.5 Recent Jcode session-audit evidence

The local audit at [`docs/audits/RECENT_SESSION_INSIGHTS_2026-08-21.md`](../audits/RECENT_SESSION_INSIGHTS_2026-08-21.md) is useful because it observes the real Jcode workflow rather than only external benchmarks. Its 30-file snapshot contained:

- 2,010 messages, 975 top-level tool calls, and 20 recorded tool errors;
- only 8 sessions longer than three messages, with the two largest accounting for 77.5% of messages and 75.1% of tool calls;
- 227 batch calls containing 723 child calls;
- one long workflow with 49 background-task management calls;
- 23 schedule calls, 6 of which produced errors;
- no recorded `session_transition` or swarm calls in the historical sample;
- 28 todo calls, but only one session visibly ended with every todo terminal;
- duplicate initial-prompt groups consistent with restart or retry behavior without a structured handoff.

The snapshot is explicitly directional and skewed, not a benchmark. The important product evidence is the recurring **lifecycle ambiguity**: implementation can finish while todos, validation evidence, worktree/landing state, or cleanup ownership remain unclear. This supports a model-visible lifecycle gate rather than a rigid workflow engine:

```text
acceptance criteria -> implement/investigate -> validate -> review risk
  -> land or explicitly hand off -> reconcile todos, Git, ownership, and final state
```

It also suggests two cost controls for the research harness:

1. deterministically prefilter empty, setup-only, and abandoned sessions before paying for qualitative summaries;
2. extract counts, errors, compaction markers, retries, and todo state deterministically before asking a model to interpret selected sessions.

For the long-running benchmark, this means measuring lifecycle closure and evidence completeness, not merely token count or memory-injection frequency. It also supports making the handoff envelope deterministic from todos, files, Git state, and tracking identifiers before asking the model to refine the prose.

## 6. Proposed policy for Jcode and Locus

### 6.1 Default decision table

| Situation | Default action | Why |
| --- | --- | --- |
| Same milestone, no topic shift, context below hard pressure | Continue | Avoid reset overhead |
| Same milestone, context pressure or projected growth | Compact | Preserve local continuity cheaply |
| Completed Bead/todo group and next group is related | Compact or continue, then handoff only if the next group changes responsibility | A completed group alone is not always enough |
| Completed milestone with a different role or topic | Fresh-session handoff | Remove anchoring and make the next contract explicit |
| Two or more compactions without objective progress | Handoff or ask for help | The current context or strategy is likely poisoned |
| Repeated same failure | Switch strategy or block | Do not spend budget on blind retries |
| Profile/model/instruction change | Fresh-session handoff | Operational context changed |
| Active background process or server | Checkpoint process ownership first; handoff only after resumability is recorded | Preserve effects and cleanup obligations |
| Hard context limit or oversized payload | Make a minimal safe checkpoint, then handoff | Avoid losing the current unit |
| Human asks for a new unrelated task | New session or clear context | Prevent stale-goal contamination |

### 6.2 Who should decide whether to hand off?

The agent should be allowed to **propose** a handoff, but it should not be the only authority that executes one. Semantic and operational information live in different places:

| Decision input | Best source |
| --- | --- |
| “The next milestone is materially different” | Agent plus Bead/todo state |
| “I am repeating a stale assumption” | Agent plus correction/failure counters |
| “The current compaction is still running” | Jcode controller |
| “A background process has an uncheckpointed owner” | Process/run manifest |
| “This handoff would exceed budget or chain depth” | Deterministic controller |
| “The child session restored the startup state” | Session/server receipt |

The optimal interaction is therefore:

1. **Controller automatically compacts** when context pressure requires continuity.
2. **Controller constructs a deterministic handoff envelope** from Beads/todos, Git/worktree state, validation receipts, process ownership, and durable tracking.
3. **Agent guidance teaches semantic handoff triggers** and exposes `session_transition` as a model-callable operation.
4. **The agent proposes handoff** when it reaches a semantic boundary, repeated correction loop, review boundary, or changed operating context.
5. **The controller validates preconditions**: safe turn boundary, no active compaction, durable todo/evidence state, process manifest present, budget available, and chain depth below the limit.
6. **The controller performs the transition** and waits for child startup acknowledgment.
7. **The child runs acceptance-oriented catch-up**, not a vague “continue” prompt.

This is better than either extreme:

- a fully automatic token-only handoff can reset at the wrong time;
- a purely agent-controlled handoff can happen too late, too early, or while operational state is unsafe.

In other words: **automatic compaction, agent-proposed semantic handoff, controller-enforced safety gate**.

### 6.3 The bounded hybrid loop

A Jcode-facing policy could be represented as:

```text
START RUN
  load Bead, Locus run manifest, and current worktree
  select one bounded milestone

WHILE milestone is not accepted:
  execute one normal Jcode turn
  record tool effects and process ownership

  IF hard safety failure:
    checkpoint -> block or recover

  ELSE IF same milestone and context pressure:
    compact

  ELSE IF milestone boundary, topic shift, role change,
          repeated compaction, or repeated correction:
    persist durable state
    generate bounded handoff
    create child session
    resume child

  ELSE IF deterministic acceptance fails:
    classify failure
    retry once with same context, or switch strategy

  ELSE:
    continue

  IF iteration, cost, time, or handoff-depth budget is exhausted:
    record blocked state and stop

RUN ACCEPTANCE
  deterministic checks
  independent evidence review
  Locus verifies effects and integration
  complete or block the Bead
```

Important guardrails:

- never use a completion phrase as the only completion proof;
- cap both iterations and handoff depth;
- treat a handoff as successful only after the child restores its startup state;
- persist a run manifest before resetting;
- preserve an append-only evidence trail;
- stop after repeated failures instead of making the loop infinite;
- do not reset while an untracked background process is the only source of truth.

### 6.4 Long-running process ownership

For a process such as a daemon, build, test server, workflow runner, or `jcode run`, the handoff artifact must include more than “the command is still running.” Store a redacted process manifest:

```yaml
run_id: <stable id>
owner: <bead or stage id>
working_dir: <absolute path>
command: <normalized command>
pid: <pid if live>
process_group: <pgid if known>
socket: <socket path if applicable>
log_path: <log path>
state_path: <durable state path>
started_at: <timestamp>
heartbeat_at: <timestamp>
expected_exit: <success|failure|long-lived>
restart_command: <safe restart form>
cleanup_command: <safe cleanup form>
lease_expires_at: <timestamp>
```

The handoff should say whether the next session is expected to:

- attach to the existing process;
- wait for it and inspect its receipt;
- restart it after verifying the old process is dead;
- abandon it and clean up the process group;
- ask the user because ownership is ambiguous.

This is where the distinction between “session reset” and “durable workflow recovery” is most important. A new context does not make an orphaned process safe.

## 7. Benchmark design

The benchmark should be a controlled replay, not an informal overnight run. The main result should be a paired comparison with the same task, model, repository state, acceptance tests, and budget across arms.

### 7.1 Research questions

- **RQ1:** Does fresh-session handoff reduce stale-goal errors and anchoring relative to compaction at the same checkpoint?
- **RQ2:** Does structured handoff improve downstream acceptance compared with a task ledger plus fixed prompt?
- **RQ3:** Is the extra startup and repository re-discovery cost justified by higher success or lower correction cost?
- **RQ4:** Does semantic triggering beat token-only triggering?
- **RQ5:** Does the hybrid policy reduce repeated failures without causing excessive resets?
- **RQ6:** Which policy recovers best after provider failure, process interruption, corrupted summaries, or socket disconnects?
- **RQ7:** Does Locus-owned process/effect state prevent failures that a context-only loop cannot detect?

### 7.2 Experimental arms

Use at least these arms:

| Arm | Context policy | Durable state | Purpose |
| --- | --- | --- | --- |
| A | Append-only same session | Git and task file | Baseline for transcript accumulation |
| B | Same-session automatic compaction | Git, task file, compaction summary | Isolate compaction benefit |
| C | Fresh-session handoff at fixed milestone boundaries | Git, task file, structured handoff | Isolate fresh context |
| D | Fresh-session file-backed Ralph | Git, PRD/Bead projection, progress log | Test fixed-prompt fresh iteration |
| E | Jcode hybrid | Bead, todos, compaction, structured handoff | Main candidate |
| F | Locus-controlled hybrid | Run manifest, leases, receipts, worktree, Jcode worker | Test end-to-end factory reliability |

For a first pilot, run A, B, C, and E. Add D and F after the harness and receipts are stable.

### 7.3 Checkpoint-equivalence protocol

There are two valid experiments, and they must not be confused.

#### Natural-policy track

Let every arm decide naturally when to compact or hand off. This measures real product behavior but confounds trigger quality, summary quality, and reset quality.

#### Controlled-boundary track

Pause all arms at a common checkpoint. Produce one canonical durable state bundle containing:

- repository commit and dirty diff;
- Bead and todo state;
- acceptance criteria;
- files changed;
- verified commands and receipts;
- unresolved risks;
- current process manifest;
- exact next milestone;
- bounded recent evidence.

Then give each arm the same factual checkpoint content, adapting only the wrapper required by the arm. This isolates whether a fresh context helps when the information content is held approximately constant.

The controlled track is the most important missing evidence for `jcode-wzo`.

### 7.4 Task suite

Build a small, representative corpus rather than beginning with only SWE-Bench.

| Category | Example | Failure signal |
| --- | --- | --- |
| Same-milestone coding | Implement one API change with tests | Compaction and iteration efficiency |
| Multi-milestone feature | Schema, implementation, docs, tests | Boundary quality |
| Topic shift | Fix a bug, then start unrelated CLI work | Stale-goal contamination |
| Ambiguous task | Incomplete issue with multiple plausible designs | Handoff and human escalation |
| Repeated failure | Failing test with misleading first diagnosis | Strategy switching |
| Long tool trace | Search, inspect, run tests, edit repeatedly | Context noise and redundancy |
| Active background process | Server/build/workflow keeps running across reset | Process ownership and recovery |
| Interruption | Kill agent or disconnect socket mid-tool | Durable recovery |
| Corrupted state | Truncated summary or stale progress file | Recovery safety |
| Integration conflict | Worktree change conflicts at merge | Locus receipts and ownership |

Use both synthetic tasks with known ground truth and real repository tasks. Synthetic tasks make fault injection and repeated seeds easier. Real tasks prevent overfitting to toy acceptance checks.

### 7.5 Suggested pilot size

A reasonable progression:

1. **Harness smoke:** 4 tasks, 2 arms, 2 seeds. Verify receipts, replay, interruption, and cleanup.
2. **Pilot:** 12 tasks, 4 arms, 3 seeds = 144 runs. Estimate effect size and identify broken metrics.
3. **Confirmatory campaign:** 24-30 tasks, 4-6 arms, 5 seeds, only if the pilot shows a meaningful signal and cost is approved.

Use paired task assignments and fixed random seeds where the provider supports them. Do not pool results across model versions without recording the version as a factor.

### 7.6 Metrics

#### Primary outcome metrics

- accepted completion rate;
- acceptance criteria pass rate;
- regression rate after later milestones;
- cost per accepted completion;
- recovery success after an injected interruption.

#### Quality and correctness metrics

- stale-goal error count;
- incorrect completion claims;
- repeated exploration of already-known files or tests;
- repeated failed strategy count;
- number of corrective turns after an alleged completion;
- independent review findings;
- process cleanup correctness;
- evidence completeness.

#### Efficiency metrics

- input tokens and output tokens;
- cache-read and cache-write tokens where available;
- model/API spend;
- number of provider requests;
- number and size of compactions;
- number and size of handoffs;
- repository re-discovery tool calls;
- test/build wall time;
- process startup and idle time;
- peak memory and sandbox/worktree footprint;
- user intervention count.

#### Context metrics

- context utilization at compaction and handoff;
- summary size and retained-fact recall;
- number of acceptance facts omitted from the checkpoint;
- number of stale facts carried into the next session;
- time to first useful action after reset;
- first-action correctness.

### 7.7 Instrumentation schema

Emit one JSONL record per run, stage, turn, reset, tool effect, and acceptance check. At minimum:

```json
{
  "run_id": "...",
  "task_id": "...",
  "arm": "hybrid",
  "model": "...",
  "model_version": "...",
  "seed": 1,
  "stage_id": "...",
  "session_id": "...",
  "parent_session_id": "...",
  "context_policy": "compact_then_handoff",
  "context_usage_before": 0.68,
  "reset_reason": "milestone_boundary",
  "input_tokens": 0,
  "output_tokens": 0,
  "cache_read_tokens": 0,
  "reported_cost_usd": 0.0,
  "tool_calls": 0,
  "repeated_exploration_calls": 0,
  "acceptance_passed": false,
  "stale_goal_error": false,
  "process_manifest_valid": true,
  "intervention_count": 0,
  "wall_time_ms": 0,
  "failure_class": null
}
```

Never store provider credentials, raw secrets, private repository contents, or unredacted user prompts in public benchmark traces.

### 7.8 Statistical analysis

Do not report only mean tokens or one successful overnight run. Use:

- paired bootstrap confidence intervals for arm differences;
- mixed-effects logistic regression for acceptance and recovery, with task and model as random effects;
- cost-success frontier plots rather than a single cost average;
- median and p95 latency, not only mean latency;
- error counts per accepted task, not only per attempted task;
- a predeclared primary endpoint, preferably accepted completion at a fixed budget;
- an explicit treatment of blocked, timed-out, and corrupted-state runs.

Useful derived measures:

```text
cost_per_accepted_task = total_run_cost / accepted_tasks

reset_efficiency = accepted_tasks / (compactions + handoffs + retries)

recovery_rate = recovered_runs / interrupted_runs

stale_goal_rate = stale_goal_runs / runs_after_reset

redundant_exploration_rate = repeated_discovery_calls / all_discovery_calls
```

A strategy that uses 15% fewer tokens but lowers acceptance by 5 points is not cost-effective. A strategy that adds a handoff but prevents a two-hour failed run may be cheaper in total system cost even if model tokens increase.

## 8. Fault-injection campaign

Long-running work should be tested as a recovery system, not only as a prompt system.

Inject one fault at a time first:

1. provider timeout during a normal turn;
2. provider disconnect after a tool call but before the assistant message;
3. compaction failure or malformed summary;
4. session process kill after a file write;
5. client/socket disconnect while the server continues;
6. stale handoff startup message;
7. duplicate child-session request;
8. stale lease or expired run manifest;
9. background process exits unexpectedly;
10. background process survives but the original session dies;
11. Git worktree has an unexpected dirty change;
12. validation command returns a transient failure;
13. integration conflict after the agent reported success;
14. corrupted or truncated progress ledger.

For each fault, verify:

- no duplicate irreversible effect;
- no orphaned process group;
- no false completion;
- no lost task ownership;
- no silent state rollback;
- clear blocked or retryable classification;
- enough evidence for an operator to resume.

The strongest acceptance artifact is a machine-readable receipt, not an agent sentence saying “recovered.”

## 9. Social and external validation

Social validation is useful, but it should validate the problem framing and reproducibility rather than substitute for evidence.

### 9.1 Public artifact package

Publish a small, inspectable package:

- this research report;
- benchmark task definitions;
- arm configuration files;
- redacted JSONL schema and sample traces;
- metric computation script;
- fault-injection recipes;
- exact model/provider versions;
- cost and latency accounting rules;
- pre-registered hypotheses and primary endpoints;
- a limitations section;
- reproduction instructions using a local or free mock provider where possible.

### 9.2 External review targets

Invite feedback from distinct communities:

- Ralph maintainers and users, for loop semantics and operational failure modes;
- Jcode, Pi, and Oh My Pi users, for handoff and compaction UX;
- Locus users and workflow-engineering practitioners, for receipts, leases, and recovery;
- SWE-agent benchmark researchers, for task validity and evaluation leakage;
- Temporal/LangGraph practitioners, for durable-execution and replay claims.

Ask reviewers to answer bounded questions:

1. Is the arm definition faithful to the system being compared?
2. Is the checkpoint-equivalence protocol fair?
3. Are the metrics sufficient to capture correctness and total cost?
4. Which failure mode is missing?
5. Can they reproduce one fault-injection scenario?

### 9.3 What counts as meaningful social evidence

Strong:

- an independent person reproduces a result from the public harness;
- an external maintainer identifies a real failure mode and the benchmark catches it;
- two independent reviewers agree the comparison is fair;
- a separate repository or task suite shows the same directional result;
- a user adopts the run manifest or handoff schema and reports measurable improvement.

Weak:

- likes, stars, reposts, or “this feels right” comments;
- one successful overnight run;
- self-reported project savings without a baseline;
- comparing different models, prompts, and task sets at once;
- a completion phrase or green-looking dashboard without receipts.

A useful public post should say what is known, what is only a hypothesis, and exactly how others can falsify it.

Suggested short call for replication:

> We are comparing same-session compaction, fresh-session handoff, file-backed Ralph iteration, and a Bead/Locus-controlled hybrid on long-running coding tasks. The key control is an equivalent checkpoint bundle. We are measuring accepted completion, stale-goal errors, repeated exploration, recovery after interruption, total wall-clock cost, and process cleanup. Can you run the four-arm pilot on one repository or review the checkpoint protocol for confounds?

## 10. Recommended implementation sequence

This is a research and observability roadmap, not an immediate source-code request.

### Phase 0: make the current behavior measurable

- record effective compaction and handoff policy per run;
- record context usage at every compaction/handoff decision;
- record the semantic reason, not just the token threshold;
- record handoff depth, prompt size, todo carryover, and child startup success;
- record process manifests for long-running commands;
- ensure logs distinguish compaction, handoff, retry, strategy switch, and block.

### Phase 1: build a local replay harness

- use mock providers or recorded provider responses first;
- run the controlled-boundary track without paid calls;
- verify event ordering, receipt durability, and cleanup;
- add fault injection before optimizing prompts.

### Phase 2: run the pilot

- use a small task matrix and four arms;
- predeclare endpoints;
- inspect individual traces, not only aggregate scores;
- diagnose any acceptance failure before rerunning the campaign.

### Phase 3: choose policy defaults

Only after the pilot:

- adjust thresholds if semantic triggers outperform token-only triggers;
- keep compaction as the default within a coherent milestone unless evidence says otherwise;
- add handoff recommendations at semantic boundaries;
- add a bounded retry/strategy-switch policy;
- keep human escalation for ambiguous or repeated failures.

### Phase 4: connect Locus receipts

- make the run manifest the source of process ownership;
- make Jcode handoff consume and update the manifest;
- make Locus verify child startup, validation, and cleanup;
- make Git and provider effects idempotent or receipt-backed;
- publish a portable redacted run report.

## 11. Current conclusion

The strongest design is not “Ralph instead of handoff” or “handoff instead of compaction.” It is a hierarchy:

1. **Use the transcript** while the current turn and milestone are coherent.
2. **Use compaction** to preserve continuity within that milestone.
3. **Use structured handoff** to cross a semantic boundary or escape context poisoning.
4. **Use Beads and repository artifacts** as durable task truth.
5. **Use Locus receipts and run manifests** as durable effect and process truth.
6. **Use Ralph-like iteration** as a bounded retry policy with acceptance checks, strategy switching, and stop conditions.

The likely Jcode/Locus default is therefore:

> **Compact first within a coherent milestone. Hand off at semantic boundaries, repeated compaction, repeated correction, or operational context changes. Let Beads describe the work, let Locus own effects, and never let an infinite loop or completion phrase be the authority for correctness.**

The highest-value next experiment is the controlled replay described above. It should compare equivalent checkpoint content across compaction-only, handoff-only, and hybrid arms, then add process interruption and cleanup faults. That experiment will answer the question more reliably than another round of prompt tuning or another overnight Ralph run.

## Sources and local evidence

### Local Jcode and Locus artifacts

- [`docs/proposals/HANDOFF_AGENT_COMPARISON.md`](../proposals/HANDOFF_AGENT_COMPARISON.md)
- [`docs/proposals/FRESH_SESSION_HANDOFF.md`](../proposals/FRESH_SESSION_HANDOFF.md)
- [`docs/MEMORY_ARCHITECTURE.md`](../MEMORY_ARCHITECTURE.md)
- [`docs/audits/RECENT_SESSION_INSIGHTS_2026-08-21.md`](../audits/RECENT_SESSION_INSIGHTS_2026-08-21.md)
- `crates/jcode-config-types/src/lib.rs` (`CompactionConfig`, `HandoffConfig`)
- `crates/jcode-app-core/src/agent.rs` (handoff guidance and advisory trigger)
- `crates/jcode-app-core/src/server/client_actions.rs` (child-session creation and generated handoff)
- [`../locus/docs/research/deterministic-agent-runner-feature-recommendations-2026-08.md`](../../../locus/docs/research/deterministic-agent-runner-feature-recommendations-2026-08.md)
- [`../locus/docs/research/context-efficient-staged-agent-workflows-2026-08.md`](../../../locus/docs/research/context-efficient-staged-agent-workflows-2026-08.md)
- [`../locus/docs/research/coding-agent-workflow-builder-comparison-2026-08-14.md`](../../../locus/docs/research/coding-agent-workflow-builder-comparison-2026-08-14.md)

### External primary sources

- Anthropic, **Effective harnesses for long-running agents**: <https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents>
- Anthropic Claude Code Ralph Wiggum plugin: <https://github.com/anthropics/claude-code/tree/main/plugins/ralph-wiggum>
- Official Ralph Stop hook: <https://raw.githubusercontent.com/anthropics/claude-code/main/plugins/ralph-wiggum/hooks/stop-hook.sh>
- snarktank/ralph: <https://github.com/snarktank/ralph>
- Anthropic autonomous coding quickstart: <https://github.com/anthropics/claude-quickstarts/tree/main/autonomous-coding>
- Claude Code session documentation: <https://code.claude.com/docs/en/sessions>
- Liu et al., **Context as a Tool**, arXiv:2512.22087: <https://arxiv.org/abs/2512.22087>
- Gao et al., **SWE-MeM**, arXiv:2606.28434: <https://arxiv.org/abs/2606.28434>
- Zhang et al., **CoMem**, arXiv:2605.30842: <https://arxiv.org/abs/2605.30842>
- Chen et al., **Adaptive Multi-Agent Scaffolding**, arXiv:2606.25514: <https://arxiv.org/abs/2606.25514>
- Chang et al., **AgentSysBench**, arXiv:2608.15127: <https://arxiv.org/abs/2608.15127>
- Chen et al., **BENCH2ROBUST**, arXiv:2608.11977: <https://arxiv.org/abs/2608.11977>
- LangGraph persistence and durable execution: <https://docs.langchain.com/oss/python/langgraph/persistence>, <https://docs.langchain.com/oss/python/langgraph/durable-execution>
- Temporal workflow replay and durable execution: <https://docs.temporal.io/workflows>

## Evidence status and limitations

- Claims from abstracts and project READMEs are reported as claims by those sources, not independently reproduced here.
- The Ralph plugin's reported real-world results are self-reported project claims and are not treated as controlled evidence.
- The CAT, SWE-MeM, CoMem, and icat numbers are not apples-to-apples with Jcode. They use different models, training, prompts, environments, and acceptance setups.
- Web search was partially blocked by search-provider anti-bot responses. The research therefore used direct official pages, GitHub raw files, arXiv records, and repository-local evidence.
- No paid live-model benchmark was run. The proposed benchmark deliberately separates local replay validation from later external-cost validation.
