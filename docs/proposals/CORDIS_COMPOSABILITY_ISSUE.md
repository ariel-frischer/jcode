# Adopt Cordis's composability principles, and build the runtime in Rust

**Status:** Draft issue text. Not posted.
**Date:** 2026-08-19
**Related:** [`CORDIS_RUST_REWRITE.md`](CORDIS_RUST_REWRITE.md) (implementation staging), [`../DEEPSEEK_HARNESS_COMPARISON.md`](../DEEPSEEK_HARNESS_COMPARISON.md) (market context)

---

## Summary

DeepSeek Harness (`dsh`) launched on 2026-08-13 and its central architectural claim is **"everything is a plugin"**: model adapters, tools, session logging, agent loops, persistence, sandboxing, approval policy, and UI are all composed into a single plugin tree. That tree is not bespoke. It is [Cordis](https://github.com/cordiverse/cordis), vendored into the harness, and Cordis has a [formal paper](https://github.com/cordiverse/paper/blob/main/paper.pdf) behind it describing a programming paradigm for *spatiotemporal composability*.

This issue proposes that Jcode adopt the paradigm, not the implementation. Specifically:

1. Cordis's two principles solve problems Jcode already has, and Jcode currently solves them by hand, inconsistently, in several places.
2. A Rust implementation of the Cordis model is a **generally useful piece of infrastructure for any coding agent**, not a Jcode-internal refactor. Agent harnesses are exactly the workload the paper names as motivating, and the runtime layer for them needs to be fast and memory-cheap. That is a Rust job.
3. The correct sequencing is a standalone Rust runtime first, Jcode as its first real consumer second.

This issue is about *whether the direction is right*. Implementation staging is already sketched in [`CORDIS_RUST_REWRITE.md`](CORDIS_RUST_REWRITE.md).

## What Cordis actually claims

The paper is not a plugin-API design doc. It identifies two orthogonal dimensions of dynamic composition and lifts two classical type-system concepts (effects and coeffects) into runtime mechanisms.

| Dimension | Definition | Static analogue | Runtime mechanism |
|---|---|---|---|
| **Temporal composability** | Removing a component completely and safely reverses every modification it made to the shared environment | Lexical scoping, RAII, bracket patterns | **Revertible effects**: every context transformation carries an inverse the runtime tracks |
| **Spatial composability** | Components declare, discover, and resolve dependencies on one another in a structured, verifiable way | Module import resolution | **Reactive coeffects**: a component declares required keys; every context change notifies it as activating, deactivating, or neutral |

Both are trivial when composition is static. Both get hard when components arrive and depart at runtime, because effects are no longer lexically bounded and dependencies can appear, disappear, or change identity mid-execution.

Two consequences matter for us:

**Cleanup stops being a matter of author diligence.** In Cordis every context mutation flows through one primitive, `ctx.effect`, which returns a disposer. Registration and its inverse live at the same site. The paper's phrasing is that correctness "that would otherwise rest on each author's diligence is instead discharged once, by the abstraction." The dsh docs state the practical result plainly: anything registered through `ctx` (listeners, tools, timers) is cleaned up on unload, with no manual uninstall path.

**Dependency cycles become a static, reportable condition.** Under reactive coeffects a cycle just leaves the involved components permanently inactive, and unlike deadlock it is predictable from the declarations alone, so the runtime can report it at load time rather than at reload time.

The paper's own case study is Koishi: 4000+ community plugins over four years on this model, including hot module replacement that re-applies edited plugins on save while preserving cache state and live connections elsewhere. It is honest about the limits of that evidence: a single ecosystem in a single host language, observational rather than a controlled comparison, an "existence-and-adoption result rather than a quantitative one."

## Why this is relevant to Jcode specifically

Jcode is not missing extensibility. It has an event bus, hooks, a `Tool` trait with a dynamic `Registry`, provider registration, MCP servers with dynamically registered tools, session-scoped tool policies, skills, swarm/subagents, and reload infrastructure. The problem is that ownership, ordering, and teardown are re-implemented per subsystem, by convention.

Three concrete symptoms in the current tree:

**1. Teardown by string prefix.** The tool registry has no notion of who owns a registration, so MCP teardown recovers ownership from a naming convention:

```rust
// crates/jcode-app-core/src/tool/mod.rs
pub async fn unregister_prefix(&self, prefix: &str) -> Vec<String>
```

```rust
// crates/jcode-app-core/src/tool/mcp.rs
registry.unregister_prefix(&crate::mcp::dispatch_name(&server_name, "")).await;
// ...
registry.unregister_prefix("mcp__").await;
```

This works, but it is precisely the failure mode the paper describes: the disposal path is separated from the creation path, so completeness is unverifiable. It only covers tools, because tools are the only effect the convention can name. Anything else an MCP server caused (listeners, background tasks, cached state) is not covered by the same sweep.

**2. Reload is process replacement.** Jcode's reload path `exec`s a replacement binary:

```rust
// crates/jcode-app-core/src/server/reload.rs
prepare_server_exec(&mut cmd, &socket);
let err = crate::platform::replace_process(&mut cmd);
```

The paper names this directly as *the coarse-grained workaround*: operating systems supply temporal composability at process granularity, so a misbehaving module is handled by restarting the process, discarding all process-local accumulated state. Jcode goes to real effort to soften that cost, including carrying a resolved `JCODE_RELOAD_AUTH_STATUS` snapshot across `exec` so the replacement does not re-probe credentials while the socket is down. That workaround is evidence for the argument: we are paying engineering cost to compensate for missing fine-grained composability.

To be clear, full-process reload is the right call for a self-dev binary swap and should stay. The claim is narrower: it should not be the only granularity available. Reconnecting an MCP server, swapping a provider, toggling a policy, or reloading a skill should not need the same hammer.

**3. Startup ordering is manual.** Registration order across tools, providers, MCP, skills, and session services is expressed as imperative call order rather than declared dependencies. That is workable today and gets progressively worse as seams multiply. Across 85 crates the ordering knowledge is not written down anywhere a runtime can check.

None of this is broken. All of it is the "distributed across multiple subsystems" state that a single composition model would unify.

## Why Rust, and why this is bigger than Jcode

This is the part I care about most, and it is the reason to treat it as a standalone project.

**The paradigm is explicitly language-agnostic.** The paper devotes a section (6.4) to language independence and concludes the model can be realized in any language meeting requirements on both dimensions. It calls out Rust by name twice: traits let a provider extend the context type from its own module via `impl`, giving well-typed dependency access; and procedural macros can emit, for each dependency, a typed declaration together with the accessor that mediates it, "dispensing with a general-purpose interception primitive." Cordis in TypeScript needs `Proxy` to interpose on dependency access. Rust can do it at compile time.

**Agent harnesses are the paper's own motivating example.** Section 1.2.2 is about self-evolving agent harnesses: systems that compose tool suites and execution environments, govern permissions and sandboxing, maintain session state and persistence, manage context and memory, orchestrate subagents, and expose interfaces to users and automation. That is a description of Jcode. The paper's argument is that without temporal composability each self-modification forces a full restart that discards process-local state, and "even worse, a faulty self-modification can disable the very process needed to recover." Anyone who has broken a self-dev build knows that failure mode.

**The runtime layer is the hot path, so it should not be JavaScript.** A composability runtime mediates every registration, every dependency access, and every event dispatch in the process. In TypeScript that mediation is `Proxy` traps and megamorphic property access on the hot path, plus the memory floor of a Node process per agent. Jcode's whole differentiation is a low-memory, multi-session, single-server model. A Rust implementation can make context access a typed field resolution rather than a proxy trap, make dispatch a monomorphized call, tie disposer ordering to `Drop` and structured concurrency, and cancel component-owned async work through the existing runtime rather than bolting cancellation on.

**No good Rust implementation of this model exists.** There are DI containers, actor frameworks, ECS libraries, and plugin loaders, but I am not aware of a Rust runtime that unifies revertible effects with reactive coeffects the way the paper specifies. Meanwhile every serious coding agent is converging on the same needs: replaceable model adapters, dynamically registered tools, MCP bridges, permission interception, session persistence seams, subagent orchestration. Several of them are or want to be Rust. A `cordis-rs` that is genuinely independent of Jcode could serve all of them, and Jcode being its first demanding consumer is a feature, not a conflict of interest.

That is the strategic framing: **not "make Jcode more like dsh," but "build the runtime layer the whole category is missing, in the language the runtime layer should be written in."**

## What the model would give Jcode concretely

Mapping the paper's mechanisms onto seams that already exist:

| Cordis mechanism | Jcode application |
|---|---|
| `ctx.effect` with paired inverse | MCP server teardown removes its tools, listeners, tasks, and cached state as one owned unit, replacing `unregister_prefix` conventions |
| Reactive coeffects | Tools that need a provider stay inactive until one is configured instead of failing at call time; swapping a provider reactivates only its real dependents |
| Declared dependencies | Startup ordering becomes a checkable graph; cycles are reported at load rather than discovered at reload |
| `waterfall` dispatch | Tool authorization, permission policy, prompt transformation, and request policy become ordered middleware with explicit short-circuit, instead of hook call sites |
| Component-scoped contexts | Session-scoped and agent-scoped registrations unwind exactly with their session or agent |
| Service isolation realms | Swarm workers and subagents get their own instances of a service (e.g. a shell with different limits) without global state |
| Service broker (paper §6.2) | Multiple providers behind one interface, enabling load balancing, rolling provider updates, and cross-process invocation without consumers reloading |
| Fine-grained reload | Reconnecting MCP, swapping a provider, or reloading a skill stops requiring a process `exec` |

The service broker point deserves emphasis: it turns rolling updates from an infrastructure operation into an application-level composition pattern. For a tool that swaps model providers under live sessions, that is directly useful.

## Honest counterarguments

I want these in the issue rather than discovered later.

**Jcode is a product, dsh is a framework.** Our comparison doc already makes this point: Jcode's opinionated, product-oriented architecture is an advantage for users who want a reliable workflow rather than a framework they must assemble. Maximum replaceability has real costs in complexity and churn. This proposal should improve internal composition safety, not turn Jcode into a kit.

**dsh's star count is not evidence of correctness.** 166k stars in six days is mostly DeepSeek's distribution, a hot category, and a shareable slogan. The argument for Cordis has to stand on the paper and on Jcode's own pain points, and it does. It would stand even if dsh had launched to silence.

**Component granularity has a real cost.** The paper acknowledges (§6.5) that decomposing mutual dependencies can grow integration components quadratically, and that more components mean more configuration, naming, and cognitive overhead. Jcode should decompose deliberately, not maximally.

**The inverse is an obligation, not a guarantee.** Cordis does not verify that a supplied inverse actually undoes its effect; that remains on the component author. And effects that cross the system boundary (§6.1) cannot be reverted at all, only compensated. A Rust implementation inherits both limits. What it buys is that cleanup is *expressible and ordered by default*, not that it is proven.

**Rust has no stable ABI.** This constrains external plugins, and the existing proposal already handles it correctly: built-in Rust components first, then a versioned out-of-process protocol, then Wasm/WASI, and native dynamic loading only with a deliberate C ABI, if ever. Note the paper's own framing (§6.4): native code has no module registry, so introduction and retraction become explicit `dlopen`/`dlclose`. That is a reason to keep the first runtime in-process and the plugin boundary out-of-process.

**Scope risk is the main danger.** Unifying tools, hooks, providers, MCP, and sessions without breaking session behavior, permissions, background tasks, or reload safety is a large effort. This is why the standalone-first sequencing matters: if the Rust core turns out to be awkward, we learn that in an isolated crate rather than halfway through a migration.

## Proposed direction

Consistent with [`CORDIS_RUST_REWRITE.md`](CORDIS_RUST_REWRITE.md):

1. **Standalone Rust core.** Service keys, component metadata, dependency resolution, activation/teardown, scoped revertible effects, deterministic teardown ordering, clear errors for missing/cyclic/conflicting dependencies. Mock services and test components only. No Jcode internals.
2. **Rust-native lifecycle semantics.** `emit`/`parallel`/`serial`/`waterfall`, reactive activation and deactivation, cancellation of component-owned async work, idempotent teardown, configuration reconciliation, and tests proving registrations disappear completely.
3. **Jcode adapter layer.** Adapters for the tool registry, bus/hooks, provider registry, MCP registration, and session services, preserving current behavior while moving ownership into component scopes.
4. **Convert a few subsystems.** MCP management first, since it already has the clearest ownership problem and the weakest teardown story. Then provider catalog, then tool policy. Prove activation, reload, teardown, and failure recovery on each before continuing.
5. **External plugin boundary last.** Only after in-process semantics are stable.

The first milestone that would settle the direction is narrow: **make MCP server teardown ownership-tracked rather than prefix-swept, and show a test that unloading a server removes every effect it created.** If that is cleaner than the status quo, the model is earning its keep. If it is not, we have spent one crate finding out.

## Questions for discussion

1. Is a standalone `cordis-rs` crate, usable by agents other than Jcode, worth building as its own project?
2. Is MCP the right first conversion target, or is the provider catalog a better proof?
3. Should the event dispatch modes land first as a standalone improvement to the existing bus, independent of the component model?
4. How much of the paper's formal model do we want to honor versus treating it as inspiration? Specifically, do we want isolation realms and interception, or just effects plus coeffects?

## References

- Shi, Zhang, Cui. *A Programming Paradigm for Spatiotemporal Composability.* Peking University, DeepSeek-AI. [cordiverse/paper](https://github.com/cordiverse/paper/blob/main/paper.pdf)
- [Cordis](https://github.com/cordiverse/cordis)
- [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) and its [Cordis primer](https://deepseek-harness.github.io/deepseek-harness/reference/cordis-primer), [plugin tutorial](https://deepseek-harness.github.io/deepseek-harness/en/develop/basic/), [services and dependencies](https://deepseek-harness.github.io/deepseek-harness/en/develop/framework/service), and [core subsystem reference](https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/core)
- Local: [`CORDIS_RUST_REWRITE.md`](CORDIS_RUST_REWRITE.md), [`../DEEPSEEK_HARNESS_COMPARISON.md`](../DEEPSEEK_HARNESS_COMPARISON.md), [`../MODULAR_ARCHITECTURE_RFC.md`](../MODULAR_ARCHITECTURE_RFC.md)
