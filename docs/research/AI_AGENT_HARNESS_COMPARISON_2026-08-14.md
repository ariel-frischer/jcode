# AI Agent Harness Comparison: Jcode, Oh My Pi, OpenCode, and Kimi Code

**Prepared:** 2026-08-14  
**Scope:** Feature and workflow comparison, with recommendations for Jcode  
**Jcode baseline:** Local custom checkout, `v0.75.3-dev` at `b6848dec8535ea361b67ff78d5ba9e6beeb07d16`  
**External sources checked:** 2026-08-14

## Executive summary

Jcode is already unusually strong as a long-running, multi-session agent runtime. Its differentiators are not just a terminal chat loop:

- A shared daemon with reconnecting clients and persistent sessions.
- Named session profiles that bundle provider, model, reasoning effort, tools, skills, and instructions.
- Cross-harness session resume, including Codex, Claude Code, OpenCode, and Pi sessions.
- Native swarm coordination with messaging, lifecycle state, optional worktrees, and a task-DAG direction.
- Persistent semantic memory with local retrieval and an optional sidecar judge.
- Many provider and OpenAI-compatible endpoint profiles, including local runtimes.
- `jcode run` safety bounds for turns, tool steps, tokens, and deadlines.
- First-class browser automation, MCP support, skills, lifecycle hooks, side panels, and self-development infrastructure.
- A strong local performance and multi-session resource-efficiency story, although the README benchmark versions are older than this comparison.

The strongest outside ideas to borrow are mostly from **Oh My Pi**, not because Jcode lacks agent orchestration, but because Oh My Pi has several mature coding-loop affordances that Jcode does not currently expose as first-class capabilities:

1. **LSP feedback wired into edits**, so the agent can immediately use compiler and language-server diagnostics.
2. **DAP debugger operations**, allowing an agent to inspect a live process instead of relying on print debugging.
3. **An advisor/watchdog reviewer model** that watches the primary transcript and injects typed concerns or blockers.
4. **A highly observable Agent Hub** with per-worker activity, costs, transcripts, steering, revive, and kill controls.
5. **Typed subagent results and stronger task isolation**, including isolated worktrees and schema-validated handoffs.
6. **Mid-stream intervention rules**, which can stop, correct, and retry a turn when the model goes off policy.

OpenCode is the clearest source of ideas for a **declarative permission policy and plugin/event model**. Kimi Code is the clearest source for **ACP and browser-based access**, **lifecycle hooks**, and **remote MCP with OAuth**.

### Recommendation in one sentence

Do not copy another harness wholesale. Keep Jcode's daemon, memory, profiles, safety bounds, and swarm model, then selectively add an LSP/DAP feedback loop, an advisor/watchdog mode, typed task artifacts, a declarative permission DSL, and remote MCP or ACP where measured user value justifies the complexity.

## What the Composio benchmark does and does not show

The requested Composio article ran the same `moonshotai/kimi-k3` model through eight harnesses against 25 valid tasks using the same hosted Composio MCP tools, provider, reasoning level, task prompts, and connected application data. The tasks exercised Gmail, Calendar, Sheets, Airtable, GitHub, Slack, Notion, Linear, and PagerDuty workflows.

Reported results:

| Harness | Passed | Pass rate | Median time | Tool calls | Cost per success |
|---|---:|---:|---:|---:|---:|
| Oh My Pi | 22/25 | 88% | 231.7s | 248 | $0.52 |
| Kimi Code | 21/25 | 84% | 281.9s | 301 | $0.64 |
| OpenCode | 18/25 | 72% | 270.5s | 299 | $0.72 |
| Jcode | Not tested | Not tested | Not tested | Not tested | Not tested |

*Pass rate uses the full 25-task comparison. Token and cost metrics use the shared 24-task usage slice because one task lacked complete usage data. Cost per success is total estimated cost divided by successful tasks within that applicable slice.*

Interpretation:

- The benchmark is useful evidence that the harness layer can materially change outcomes even when the model and tools are held constant.
- Oh My Pi's result is a strong signal in its favor, especially because it combined the highest observed pass rate with moderate cost and tool use.
- Kimi Code's result is also strong, but it used more tokens than every other harness in the shared cost slice according to the article.
- OpenCode's 72% result should not be treated as a product-wide quality score. It reflects this specific model, MCP setup, task mix, and version.
- This is primarily a business-application tool-use benchmark, not a controlled software-engineering benchmark. It does not test compiler feedback, debugger workflows, local memory, worktree coordination, or codebase-scale refactors.
- Jcode was not included, so the article cannot establish that any competitor is better than Jcode overall.

## At-a-glance feature matrix

Legend: **Yes** means directly documented in the reviewed sources. **Partial** means the capability exists in a narrower, indirect, planned, or ecosystem-dependent form. **Not found** means it was not found in the reviewed primary sources and is not evidence that the product lacks it.

| Capability | Jcode | Oh My Pi | OpenCode | Kimi Code | Assessment for Jcode |
|---|---|---|---|---|---|
| Persistent daemon and many sessions | **Yes** | Partial | Partial | Partial | Jcode is a clear architectural strength |
| Named session profiles | **Yes** | Partial | Partial | Partial | Keep and continue making profiles ergonomic |
| Cross-harness session resume | **Yes** | Not found | Not found | Not found | Strong Jcode differentiator |
| Native multi-agent swarm | **Yes** | **Yes** | Partial | Partial | Jcode has the richer coordination substrate |
| Task graph and dependency scheduling | **Yes, evolving** | Partial | Partial | Partial | Finish typed artifacts and verification gates |
| Per-worker observability and steering | Yes, swarm widgets and events | **Yes, Agent Hub** | Partial | Partial | Oh My Pi's UX is worth studying |
| Schema-validated subagent result | Partial, task artifacts evolving | **Yes** | Partial | Not found | High-value Jcode improvement |
| LSP diagnostics | Not found as a first-class loop | **Yes** | **Yes** | Not found | Highest-value coding-loop gap |
| DAP debugger control | Not found | **Yes** | Not found | Not found | Valuable, but should follow LSP |
| Browser automation | **Yes**, Firefox Agent Bridge | **Yes** | Via MCP/plugins | Via MCP | Jcode already has a direct path |
| Persistent Python/Bun execution kernels | Not found | **Yes** | Partial | Not found | Oh My Pi runs persistent Python/Bun kernels. OpenCode has shell and plugin execution. Kimi Code has shell mode, but no persistent kernel was found in the reviewed sources. |
| MCP over local stdio | **Yes** | Yes | **Yes** | **Yes** | Jcode already has the local foundation |
| Remote MCP and OAuth | Not found in current Jcode docs | Partial | **Yes** | **Yes** | Strong ecosystem gap with security implications |
| File-backed skills | **Yes** | **Yes** | **Yes** | **Yes** | Jcode already has dynamic project-local skill behavior |
| Executable plugin/custom-tool SDK | Partial, self-dev and hooks | **Yes** | **Yes** | **Yes, beta** | Jcode could add a bounded extension contract |
| Declarative allow/ask/deny policy | Partial, hooks and safety bounds | **Yes** | **Yes** | Partial | OpenCode's policy model is the best reference |
| Lifecycle hooks | **Yes** | Partial | **Yes via plugins** | **Yes, beta** | Jcode hooks are strong but less declarative |
| Built-in reviewer model | Not found | **Yes, advisor/watchdog** | Custom agent possible | Not found | Very promising for Jcode |
| ACP or IDE agent protocol | SDK and desktop surfaces, not ACP | Partial | Ecosystem integrations | **Yes** | Add only if integration demand is real |
| Browser/web UI | Desktop and client surfaces | Browser relay and TUI | Desktop beta | **Yes** | Kimi's local web mode is a useful reference |
| Public conversation sharing | Not a default feature | Collaboration relay | **Yes** | Not found | Avoid copying by default due data exposure |
| Local semantic memory | **Yes** | Not found in reviewed sources | Not found in reviewed sources | Not found in reviewed sources | Major Jcode differentiator |
| Explicit run budgets and deadlines | **Yes** | Partial | Partial | Partial | Keep expanding this safety contract |

## Detailed comparison

### 1. Runtime architecture and sessions

**Jcode** uses a single-server, multi-client architecture. The server owns sessions and state, TUI clients connect over a Unix socket, clients reconnect after disconnects or reloads, and session state persists on disk. The current CLI also supports named session profiles. A profile can bundle provider, model, reasoning effort, tools, skill policy, and additive instructions. Jcode can resume sessions from other harnesses, including Codex, Claude Code, OpenCode, and Pi.

**Oh My Pi** has persistent sessions, profiles, ACP support, and a strong session-level agent registry. Its distinctive addition is the Agent Hub, which exposes worker status, parent and child lineage, current tool and arguments, context usage, cost, token counts, transcripts, steering, revive, and kill controls.

**OpenCode** has a polished TUI session model with `/new`, `/sessions`, `/undo`, `/redo`, `/export`, and `/share`. Its session sharing is useful for collaboration but uploads full conversation history and metadata to a public URL unless disabled or unshared.

**Kimi Code** supports interactive sessions, browser UI sessions, ACP sessions, session switching, titles, undo, fork, import/export, and shell mode. It is more client-oriented than daemon-oriented in the reviewed documentation.

**Conclusion:** Jcode's runtime foundation is already stronger for long-lived local multi-session work. The opportunity is to improve the session and worker UX, not replace the daemon.

### 2. Multi-agent work

**Jcode** provides native swarm coordination through the server. Agents can message each other, broadcast, track lifecycle state, share a plan, use optional worktrees, and operate through headless or headed sessions. The current architecture is moving from an agent-first swarm model toward a task-DAG model. Deep and light modes are intended to separate comprehensive decomposition and verification from cheap flat fan-out.

**Oh My Pi** has first-class subagents with isolated worktrees, typed schema-validated results, and an Agent Hub for live observability and control. This is the most directly useful comparison for Jcode. It makes the worker's contract explicit and makes the parent session less dependent on parsing prose reports.

**OpenCode** exposes primary agents and subagents. Built-ins include Build, Plan, General, Explore, and Scout. Custom agents can select prompts, models, permissions, and tool access. This is a good simple model for role specialization, but the reviewed docs do not describe a Jcode-scale swarm coordinator or task graph.

**Kimi Code** exposes task-oriented workflows and subagent lifecycle hooks. Its reviewed material is less explicit about a general multi-agent coordination layer than Jcode or Oh My Pi.

**Recommendation:** Keep Jcode's server-owned swarm and DAG. Borrow Oh My Pi's typed result contract, isolated output paths, per-worker metrics, and explicit verification state.

### 3. Coding feedback: LSP and debugger support

This is the largest practical gap.

**Oh My Pi** wires LSP into writes and advertises 14 LSP operations. The README describes rename operations routed through `workspace/willRenameFiles`, so re-exports, barrel files, and aliased imports can be updated before a move. It also drives real debuggers through DAP, including LLDB, Delve, and debugpy workflows.

**OpenCode** integrates LSP diagnostics as feedback to the agent and has a broad set of built-in language-server configurations. It recommends enabling LSP where diagnostics improve the workflow, while noting the memory and versioning cost.

**Kimi Code** focuses on file operations, shell commands, web access, MCP, plugins, skills, and ACP. LSP and DAP were not found as first-class built-in loops in the reviewed sources.

**Jcode** has strong external tool and browser infrastructure, but LSP and DAP are not currently described as first-class agent operations in the reviewed local docs.

**Recommendation:** Add an LSP feedback layer before DAP. The first useful version does not need to expose every protocol method. It can:

1. Detect project language servers or accept explicit per-project configuration.
2. Run diagnostics after an edit or at a model-selected checkpoint.
3. Return compact, deduplicated diagnostics to the agent.
4. Expose definition, references, hover, symbol, rename, and code-action operations incrementally.
5. Enforce per-session tool and permission policy.

After measuring that loop, add DAP as an optional tool family for debugging tasks.

### 4. Extensibility: skills, plugins, commands, and hooks

**Jcode** supports project and global instructions, prompt overlays, preferred tools, file-backed skills, MCP configuration, lifecycle hooks, and self-development. Its hooks can observe turns and sessions or gate selected tool calls. Its current MCP implementation supports local stdio servers and dynamic list, connect, disconnect, and reload operations. The README documents stdio as the current transport, while HTTP and SSE entries are skipped.

**Oh My Pi** has a broad skill discovery and provider system, plugins, custom tools, an SDK, browser tools, and a sophisticated advisor configuration. Its skills support multiple providers and explicit inclusion, exclusion, and precedence behavior.

**OpenCode** has three especially reusable extension primitives:

- Project and global `SKILL.md` files loaded on demand.
- Project and global plugins written in JavaScript or TypeScript, with hooks for tool calls, messages, sessions, LSP, permissions, shell environment, and TUI events.
- Custom commands with arguments, file references, shell-output interpolation, agent selection, model selection, and optional subagent execution.

**Kimi Code** supports skills, executable plugins declared through `plugin.json`, MCP, and 13 lifecycle hook events. Its hooks are beta but cover pre-tool, post-tool, session, subagent, compaction, notification, and stop events.

**Recommendation:** Jcode does not need a broad JavaScript plugin marketplace immediately. A safer next step is a versioned, typed extension contract with:

- Explicit capabilities and approval tier.
- A stable event schema.
- A process boundary for external tools.
- Timeouts and output limits.
- Project, user, and session scopes.
- Secret-safe configuration and no implicit credential injection.

OpenCode commands are also a good low-cost idea. Jcode's skills and slash commands could support `$ARGUMENTS`, file references, selected profile/model, and an explicit `run_as_subagent` option.

### 5. Safety and permissions

**Jcode** already has several safety primitives that should not be weakened:

- `jcode run` supports max turns, max tool steps, token budgets, and deadlines.
- Invalid bounds fail before provider or tool work starts.
- The session profile system preserves policy snapshots and warns on profile drift.
- Lifecycle `pre_tool` hooks can synchronously block a tool call.
- The safety design treats communication with humans, pushes, code modifications, and system changes as actions requiring review.

**OpenCode** has the cleanest declarative policy model. Each action resolves to `allow`, `ask`, or `deny`, with wildcard and argument-pattern rules. It supports per-tool policies, external-directory boundaries, subagent permissions, skill permissions, and a `--auto` mode that still honors explicit deny rules.

**Oh My Pi** has a detailed three-tier approval system, tool-declared approval metadata, user overrides, argument-dependent policies, critical safety overrides, and a clear distinction between tool approval and real-world action authorization.

**Kimi Code** requests confirmation for sensitive operations, applies the same approval path to MCP tools, and exposes hooks that can block calls. Its hooks and plugin systems are marked beta in the reviewed docs. YOLO or AFK modes automatically approve operations, which requires careful trust boundaries.

**Recommendation:** Extend Jcode hooks with a declarative policy layer modeled after OpenCode, but preserve Jcode's stronger explicit run bounds and safety-first defaults. A useful policy shape is:

```toml
[permission]
"*" = "ask"
read = "allow"
"bash:git *" = "allow"
"bash:rm *" = "deny"
"edit:.env*" = "deny"
"swarm" = "allow"
```

The policy resolver should be tested independently from tool execution, and explicit deny should always win over auto-approval.

### 6. Memory and context management

Jcode is the clear leader among the reviewed systems in persistent local memory. Its current memory path uses local embeddings, hybrid dense plus BM25 retrieval, bounded context, background extraction, and an optional sidecar judge. Retrieval is asynchronous and normally injects verified memories on the next fresh user turn. The sidecar uses bounded candidate pools, cadence gating, and consensus filtering.

The reviewed Oh My Pi, OpenCode, and Kimi Code sources describe compaction, session persistence, skills, and context tooling, but I did not find an equivalent documented local semantic memory system. This is not proof that no such feature exists in any extension or unrevealed subsystem. It is enough to say that Jcode's memory architecture is a major product differentiator and should not be traded away for feature parity.

**Recommendation:** Use Jcode memory to improve the new advisor and task-DAG systems. Store typed task outcomes, recurring diagnostics, successful repair recipes, and user preferences as memory candidates, but keep memory injection bounded and judge-verified.

### 7. MCP and external tools

All four systems support MCP in some form.

- Jcode has a shared MCP pool, local stdio configuration, dynamic connect/disconnect/reload, and compatibility with common config shapes.
- Oh My Pi supports MCP tools as part of its broad tool and plugin surface.
- OpenCode supports local and remote MCP, enable/disable controls, remote defaults, OAuth, headers, and token storage.
- Kimi Code supports stdio and HTTP MCP, OAuth authorization, testing, config files, status display, and approval integration.

Jcode's most concrete gap is remote MCP and OAuth. This gap matters for modern business-tool workflows because the Composio benchmark used hosted MCP tools. It is also the highest-risk item because remote MCP expands the trust boundary, adds credential storage, and makes prompt injection and data exfiltration more likely.

**Recommendation:** Treat remote MCP as a staged project, not a simple transport addition:

1. Add remote HTTP transport behind an explicit opt-in configuration.
2. Add per-server allow, ask, and deny policy.
3. Keep remote servers disabled by default unless the user explicitly enables them.
4. Add OAuth only after token storage and redaction are specified.
5. Display server provenance, tools, scopes, and last connection status in the TUI.
6. Add prompt-injection and data-exfiltration fixtures before broad rollout.

### 8. Client and IDE reach

Jcode already has a daemon protocol, desktop surfaces, a Go SDK, a TypeScript SDK, side panels, and browser automation. It is not starting from zero.

Kimi Code's ACP integration is a useful reference for plugging a terminal agent into Zed and JetBrains. Its local browser UI is also a pragmatic way to provide session management without building a complete desktop application first.

OpenCode has a polished TUI, a desktop beta, an SDK and plugin ecosystem, and public conversation sharing. The sharing feature is convenient but uploads full conversation history and metadata, so it should not be copied as a default behavior.

**Recommendation:** Evaluate ACP compatibility as an adapter over the existing Jcode server rather than creating a separate agent runtime. A thin protocol bridge could make Jcode available in Zed and JetBrains while preserving Jcode's daemon, profiles, memory, and safety policy.

## Prioritized recommendations

### P0: Add first-class LSP diagnostics after edits

- **Why:** This is the clearest coding-quality gap versus Oh My Pi and OpenCode.
- **Expected value:** Fewer invalid edits, faster type-error repair, better rename and refactor reliability.
- **Effort:** Medium to high.
- **Design:** Start with diagnostics and a small set of read-only queries. Make the feedback compact and deduplicated. Add edits or rename only after the read-only loop is stable.
- **Validation:** Build a fixture matrix for Rust, TypeScript, Python, and Go. Compare repair success, extra tool calls, latency, and token use with LSP disabled.

### P0: Add an advisor/watchdog reviewer mode

- **Why:** This is the most distinctive Oh My Pi workflow idea that complements Jcode's existing swarm.
- **Expected value:** Catch wrong APIs, missing tests, unsafe actions, and scope drift before the primary agent finishes.
- **Effort:** Medium.
- **Design:** A separate model receives bounded transcript deltas and project context. It can emit `nit`, `concern`, or `blocker` notes. It should be read-only by default and never directly mutate the primary session.
- **Validation:** Seed tasks with known mistakes and measure detection rate, false positives, added latency, and added cost. Verify that advisor notes do not recursively trigger advisor review.

### P1: Make swarm task results typed and verification-aware

- **Why:** Jcode already has the task-DAG direction. Typed artifacts would turn a strong design into a more reliable coding workflow.
- **Expected value:** Less prose parsing, clearer completion semantics, fewer orphaned changes, better parent-agent decisions.
- **Effort:** Medium.
- **Design:** Require a compact artifact with outcome, changed paths, validation commands, evidence, open questions, and explicitly unchecked areas. Add optional schema-validated result payloads for machine-consumed tasks.
- **Validation:** Run parallel repair and research tasks with intentionally incomplete reports. Confirm that the coordinator can distinguish completed, blocked, failed, and unverified work without reading full transcripts.

### P1: Add an allow/ask/deny permission DSL

- **Why:** Jcode hooks are powerful, but policy is currently more procedural than declarative.
- **Expected value:** Safer unattended runs, easier project policy, clearer user control over MCP and swarm actions.
- **Effort:** Medium.
- **Design:** Reuse current hook and safety-bound contracts. Add deterministic pattern matching, explicit deny precedence, safe previews, and policy explanations in the TUI.
- **Validation:** Test exact-path rules, shell command patterns, MCP tool names, external directories, child agents, malformed rules, and auto-approval overrides.

### P1: Finish the task-DAG and artifact dataflow

- **Why:** Jcode's current swarm architecture is already moving in this direction, while Oh My Pi demonstrates the value of isolated, result-oriented workers.
- **Expected value:** Better dependency handling, retries, reassignments, verification gates, and reproducible multi-agent work.
- **Effort:** Medium to high.
- **Design:** Keep one engine with light and deep modes. Make workers fungible and make typed node artifacts the primary handoff path.
- **Validation:** Test dependency unblocking, worker crash recovery, retry and reassign behavior, gate rejection, stale artifacts, and cancellation.

### P2: Add remote MCP with explicit trust boundaries

- **Why:** OpenCode and Kimi Code are ahead for hosted MCP and OAuth, and this is relevant to the Composio benchmark setup.
- **Expected value:** Access to hosted tools and business systems without local wrappers.
- **Effort:** High.
- **Risk:** High. Remote tools can exfiltrate code, credentials, and user data.
- **Validation:** Use mock servers first. Test OAuth redaction, server disablement, scope display, prompt injection, tool provenance, timeout behavior, and data-boundary enforcement.

### P2: Add a thin ACP adapter

- **Why:** Kimi Code shows the value of IDE-native access, and Jcode already has a persistent server plus SDKs.
- **Expected value:** Use Jcode from Zed, JetBrains, and other ACP clients without duplicating runtime logic.
- **Effort:** Medium to high.
- **Design:** Map ACP sessions to Jcode server sessions. Preserve profile selection, memory policy, safety bounds, and resume identity.
- **Validation:** Test session creation, attach, resume, streaming, tool approval, cancellation, profile selection, and reconnect behavior in a real ACP-compatible client.

### P3: Add DAP after LSP proves its value

- **Why:** Oh My Pi's debugger support is compelling for hard bugs, but it is a larger surface and less universally useful than diagnostics.
- **Expected value:** Faster debugging of crashes, hangs, and stateful runtime failures.
- **Effort:** High.
- **Design:** Start with launch, attach, stack, scopes, variables, evaluate, continue, step, and breakpoint operations. Keep language-specific adapters optional.
- **Validation:** Use small Rust, Go, and Python fixtures with deterministic breakpoints and expected variable values. Measure success against a print-debugging baseline.

### P3: Add richer command templates

- **Why:** OpenCode's commands are a low-cost quality-of-life feature.
- **Expected value:** Repeatable workflows for review, test, release, migration, and diagnosis.
- **Effort:** Low to medium.
- **Design:** Extend project-local commands or skills with `$ARGUMENTS`, `$1`, file references, selected model/profile, and an explicit subagent mode.
- **Validation:** Test argument quoting, missing arguments, file references, shell output caps, profile inheritance, and permission enforcement.

## What not to copy

1. **Do not make public conversation sharing the default.** OpenCode's feature is useful, but it creates an obvious data-retention and proprietary-code risk.
2. **Do not make YOLO or AFK approval the normal mode.** Keep explicit safety bounds and policy visibility as Jcode defaults.
3. **Do not optimize for maximum agent count.** Jcode should optimize for useful parallelism, verification, and resource-aware scheduling rather than an impressive worker cap.
4. **Do not add a plugin marketplace before defining the trust model.** Start with typed, capability-declared, process-isolated extensions.
5. **Do not replace Jcode's memory system with simple transcript compaction.** Compaction is necessary, but it is not a substitute for persistent semantic memory.
6. **Do not infer overall superiority from the Composio benchmark.** It is a valuable harness-effect signal, not a complete coding-agent benchmark.

## Suggested validation benchmark for Jcode

The current external benchmark does not exercise the most important Jcode differentiators. A useful local harness benchmark should run the same model and task set through Jcode, Oh My Pi, OpenCode, and Kimi Code where practical, then separately test:

### Coding tasks

- Type-error repair with and without LSP diagnostics.
- Cross-file rename with aliases and re-exports.
- Debugger-assisted crash diagnosis.
- Multi-package refactor with tests and formatting.
- Resume after provider interruption.
- Long task with context compaction and persistent memory.

### Agentic workflow tasks

- Parallel independent changes with typed handoffs.
- Dependency-ordered task DAG with a failed worker.
- Advisor detection of an intentionally wrong API choice.
- MCP tool use with an adversarial tool result.
- Unattended run stopped by token, tool-step, and deadline bounds.
- Profile switch and restore with changed configuration.

### Metrics

- Behavioral pass rate.
- Correctness of final repository state.
- Test pass rate.
- Median and p95 latency.
- Input, output, reasoning, cache, and total tokens.
- Tool-call count and retry count.
- Human interventions and approval prompts.
- Memory footprint per session and incremental session cost.
- False-positive advisor warnings.
- Rate of unsafe or policy-violating tool attempts.

## Risk review

### Low risk

- **Scope:** Documentation of the comparison and recommendations.
- **Blast radius:** Repository documentation only.
- **Mitigation:** External claims are linked to primary sources and dated. Jcode claims point to local README or architecture docs.
- **Rollback:** Delete or revert this Markdown file.
- **Required review action:** Confirm that the prioritization matches current Jcode product goals.

### Medium risk

- **Scope:** LSP integration, advisor mode, typed task artifacts, command templates, and permission DSL.
- **Blast radius:** Agent context, latency, tool policy, worker coordination, and user interaction.
- **Mitigation:** Ship behind feature flags or opt-in configuration, preserve existing behavior by default, add focused fixtures, and measure cost and latency.
- **Rollback:** Disable the feature flag or revert the isolated subsystem without changing the daemon/session contract.
- **Required review action:** Review public configuration shape, failure semantics, and whether policy defaults are fail-open or fail-closed.

### High risk

- **Scope:** Remote MCP, OAuth credential handling, ACP exposure, DAP process control, and any automatic or public collaboration feature.
- **Blast radius:** Credentials, external services, source code, running processes, IDE clients, and potentially public data exposure.
- **Mitigation:** Explicit opt-in, server provenance, capability and scope display, secret redaction, audit logging, strict timeouts, mock-first validation, and adversarial prompt-injection tests.
- **Rollback:** Disable the transport or adapter globally and remove stored credentials or server registrations through a documented cleanup path.
- **Required review action:** Independent security and privacy review before enabling by default or connecting to live external systems.

## Source notes

The links below were reviewed on 2026-08-14. External repositories and documentation are moving targets. Re-check them before implementing a recommendation.

### Requested overview and benchmark

- [Composio: 8 Best AI Agent Harnesses in 2026](https://composio.dev/content/best-ai-agent-harnesses)

### Oh My Pi

- [Repository README](https://github.com/can1357/oh-my-pi)
- [Raw README](https://raw.githubusercontent.com/can1357/oh-my-pi/main/README.md)
- [Agent Hub](https://raw.githubusercontent.com/can1357/oh-my-pi/main/docs/agent-hub.md)
- [LSP configuration](https://raw.githubusercontent.com/can1357/oh-my-pi/main/docs/lsp-config.md)
- [Approval mode](https://raw.githubusercontent.com/can1357/oh-my-pi/main/docs/approval-mode.md)
- [Advisor and WATCHDOG](https://raw.githubusercontent.com/can1357/oh-my-pi/main/docs/advisor-watchdog.md)
- [Skills](https://raw.githubusercontent.com/can1357/oh-my-pi/main/docs/skills.md)

### OpenCode

- [Repository README](https://github.com/anomalyco/opencode)
- [Agents](https://opencode.ai/docs/agents/)
- [MCP servers](https://opencode.ai/docs/mcp-servers/)
- [Plugins](https://opencode.ai/docs/plugins/)
- [Skills](https://opencode.ai/docs/skills/)
- [Permissions](https://opencode.ai/docs/permissions/)
- [Commands](https://opencode.ai/docs/commands/)
- [Share](https://opencode.ai/docs/share/)
- [LSP servers](https://opencode.ai/docs/lsp/)
- [TUI](https://opencode.ai/docs/tui/)

### Kimi Code

The older `kimi-cli` repository says it is evolving into the successor `kimi-code` repository. The comparison therefore uses both the successor repository and the maintained documentation site.

- [Kimi Code repository](https://github.com/MoonshotAI/kimi-code)
- [Kimi CLI repository and transition note](https://github.com/MoonshotAI/kimi-cli)
- [Getting started](https://moonshotai.github.io/kimi-cli/en/guides/getting-started.html)
- [Agent Skills](https://moonshotai.github.io/kimi-cli/en/customization/skills.html)
- [Plugins](https://moonshotai.github.io/kimi-cli/en/customization/plugins.html)
- [MCP](https://moonshotai.github.io/kimi-cli/en/customization/mcp.html)
- [Hooks](https://moonshotai.github.io/kimi-cli/en/customization/hooks.html)
- [IDE and ACP integration](https://moonshotai.github.io/kimi-cli/en/guides/ides.html)
- [Slash commands](https://moonshotai.github.io/kimi-cli/en/reference/slash-commands.html)

### Local Jcode evidence

- [`README.md`](../../README.md)
- [`docs/README.md`](../README.md)
- [`docs/SERVER_ARCHITECTURE.md`](../SERVER_ARCHITECTURE.md)
- [`docs/MEMORY_ARCHITECTURE.md`](../MEMORY_ARCHITECTURE.md)
- [`docs/SWARM_ARCHITECTURE.md`](../SWARM_ARCHITECTURE.md)
- [`docs/SWARM_TASK_GRAPH.md`](../SWARM_TASK_GRAPH.md)
- [`docs/RUN_SAFETY_BOUNDS.md`](../RUN_SAFETY_BOUNDS.md)
- [`docs/HOOKS.md`](../HOOKS.md)
- [`docs/RESUME_BEHAVIOR.md`](../RESUME_BEHAVIOR.md)
- [`docs/SYSTEM_PROMPT_CONFIG.md`](../SYSTEM_PROMPT_CONFIG.md)

## Bottom line

Jcode is not obviously behind these harnesses overall. It is ahead in persistent local runtime architecture, semantic memory, session interoperability, provider/profile control, explicit unattended-run bounds, and server-native swarm coordination.

Oh My Pi is ahead in coding-loop depth and agent observability. OpenCode is ahead in declarative permissions, plugin events, reusable commands, and remote MCP ergonomics. Kimi Code is ahead in ACP, local web access, and a broad lifecycle-hook surface.

The best next move is a focused Jcode quality loop: **LSP diagnostics first, advisor/watchdog second, typed DAG artifacts third, permission DSL and remote MCP behind explicit security gates.**
