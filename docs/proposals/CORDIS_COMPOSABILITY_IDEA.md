# Idea: Cordis-style composability for Jcode

**Status:** Idea only. Not a proposal, not planned, not filed as an issue.
**Date:** 2026-08-19
**Related:** [`CORDIS_RUST_REWRITE.md`](CORDIS_RUST_REWRITE.md), [`../DEEPSEEK_HARNESS_COMPARISON.md`](../DEEPSEEK_HARNESS_COMPARISON.md)

## The idea

DeepSeek Harness is built on [Cordis](https://github.com/cordiverse/cordis), a plugin framework with a [formal paper](https://github.com/cordiverse/paper/blob/main/paper.pdf) behind it. Its model is two ideas:

- **Revertible effects.** Every registration goes through one primitive that returns its own inverse, so unloading a component undoes everything it did. Registration and cleanup live at the same site.
- **Reactive coeffects.** A component declares the services it needs. The runtime activates it when they exist and deactivates it when they vanish, instead of relying on manual startup ordering.

Together those give "everything is a plugin" real teeth: tools, model adapters, session logging, agent loops, persistence, and UI can all be components that load and unload cleanly at runtime.

The paper is explicit that the paradigm is language-agnostic, and names Rust as a good fit: traits let a provider extend the context type, and proc macros can generate typed dependency accessors at compile time instead of needing TypeScript's `Proxy` interception. It also names self-evolving agent harnesses as a motivating case, which is essentially a description of Jcode.

## Where Jcode has the same problems

Two spots in the current tree show the gap, if we ever wanted to close it.

**Teardown by naming convention.** The tool registry has no concept of who owns a registration, so MCP teardown recovers ownership from a string prefix:

```rust
// crates/jcode-app-core/src/tool/mcp.rs
registry.unregister_prefix("mcp__").await;
```

It works, but only for tools, because tools are the only effect the convention can name. Anything else a server caused is not covered by the same sweep.

**Reload is process replacement.** Jcode `exec`s a fresh binary (`replace_process` in `crates/jcode-app-core/src/server/reload.rs`), discarding process-local state. The paper calls this the coarse-grained workaround: using the OS process as your unit of cleanup because you lack a finer one. For a self-dev binary swap that is the right call. It is heavier than needed for reconnecting an MCP server or swapping a provider.

## Pros

- **Genuinely dynamic extensibility.** Components load, unload, and reload at runtime without a restart, with cleanup guaranteed by the abstraction rather than by author diligence.
- **Cleanup becomes checkable.** Ownership is tracked, so "did unloading this actually remove everything" is a testable property instead of a convention.
- **Startup ordering becomes a graph.** Dependencies are declared rather than encoded in call order, and cycles get reported at load time rather than surfacing at reload.
- **Reactive dependencies.** A tool needing an unconfigured provider stays inactive instead of failing at call time; swapping a provider reactivates only its real dependents.
- **Better interception seams.** Waterfall-style middleware fits tool authorization, permission policy, and prompt transformation more naturally than fixed hook call sites.
- **Possibly useful beyond Jcode.** No Rust runtime unifies revertible effects with reactive coeffects today, and every coding agent needs roughly these seams. A standalone crate could serve more than one consumer.

## Cons

- **Huge refactor.** Unifying tools, hooks, providers, MCP, and sessions without breaking session behavior, permissions, background tasks, or reload safety is a very large effort touching the most load-bearing parts of the codebase. This is the dominant cost.
- **Performance overhead.** Every registration, dependency access, and event dispatch goes through the runtime. Dynamic dispatch, indirection through a context, and per-component allocation are a real cost against the current direct-call model. Jcode's whole differentiation is being lean, so a composability layer has to earn its overhead. Rust helps but does not make it free.
- **Granularity cost.** The paper admits (§6.5) that decomposing mutual dependencies can grow integration components quadratically, meaning more configuration, more naming, and a bigger dependency graph to hold in your head.
- **Inverses are unverified.** Cordis does not check that a supplied inverse actually undoes its effect; that stays on the author. Effects crossing the system boundary can only be compensated, not reverted.
- **Framework versus product.** Jcode's opinionated architecture is an advantage for people who want a working terminal workflow rather than a kit to assemble. Maximum replaceability trades that away.
- **No stable Rust ABI.** External plugins would need an out-of-process protocol or Wasm; arbitrary `.so` loading is not viable.

## Read

The pros are real and the paper is good work, but the cost side is dominated by one thing: this is a rewrite of Jcode's composition model, not an addition to it. The extensibility payoff mostly matters for an open third-party plugin ecosystem, which Jcode does not currently have and may not want.

The cheapest version of the idea, if it ever came up again, is not adopting the model at all but stealing one piece: make tool registrations ownership-tracked so MCP teardown stops relying on `unregister_prefix`. That is a contained change with a testable outcome and no runtime paradigm attached.

## References

- Shi, Zhang, Cui. *A Programming Paradigm for Spatiotemporal Composability.* [cordiverse/paper](https://github.com/cordiverse/paper/blob/main/paper.pdf)
- [Cordis](https://github.com/cordiverse/cordis), [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness), [Cordis primer](https://deepseek-harness.github.io/deepseek-harness/reference/cordis-primer)
