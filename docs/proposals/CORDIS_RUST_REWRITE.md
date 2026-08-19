# Cordis-Inspired Rust Plugin Runtime for Jcode

## Status

Exploratory proposal. This document records a possible standalone Rust rewrite of the Cordis runtime and its eventual integration with Jcode.

## Goal

Rewrite the core ideas of [Cordis](https://github.com/cordiverse/cordis) in Rust first, without embedding TypeScript or a JavaScript runtime into Jcode. Once the Rust implementation is useful and stable enough, integrate it as the foundation for Jcode's plugin and component system.

The intent is **not** to port Cordis line by line. The target is a Rust-native implementation of the underlying model:

- spatiotemporal composability
- reversible effects
- reactive dependencies and service availability
- lifecycle-managed components
- typed event dispatch
- configuration-driven loading and teardown

Reference material:

- [Cordis repository](https://github.com/cordiverse/cordis)
- [Cordis primer](https://deepseek-harness.github.io/deepseek-harness/reference/cordis-primer)
- [Cordis paper](https://github.com/cordiverse/paper)

## Why this is relevant to Jcode

Jcode already contains several partial extension mechanisms:

- `BusEvent` and the internal event bus
- lifecycle hooks such as `HookEvent`
- the `Tool` trait and dynamic tool `Registry`
- provider registration
- MCP servers and dynamically registered MCP tools
- session-scoped tool policies
- reload and self-development infrastructure

These systems are useful, but registration, ownership, dependency ordering, teardown, and reload behavior are distributed across multiple subsystems. A Cordis-inspired runtime could provide one composition model for them.

## Core concepts to preserve

### Components and services

A plugin is a component that may provide named services and consume services supplied by other components. Consumers depend on stable service keys rather than concrete implementations.

Example service keys could include:

```text
llm
sessions
tools
agents
memory
mcp
observability
```

### Dependency-aware activation

A component declares its dependencies. The runtime activates it only when those dependencies are available, instead of relying on manually ordered startup code.

### Reversible effects

Every registration is owned by the component that created it and can be undone during teardown. Effects should cover at least:

- tools
- commands
- event listeners
- prompt/context providers
- model providers
- adapters
- background tasks
- configuration watchers

A component should not be considered successfully unloaded until its owned effects have been disposed.

### Typed event dispatch

The runtime should support explicit dispatch semantics rather than one generic event method:

| Mode | Behavior |
| --- | --- |
| `emit` | Notify listeners without awaiting results. |
| `parallel` | Await all listeners concurrently. |
| `serial` | Await listeners in registration order. |
| `waterfall` | Allow middleware to wrap, transform, or short-circuit downstream work. |

Waterfall events are especially relevant for tool authorization, prompt transformation, model request policy, and post-processing pipelines.

## Rust-native design direction

A first API could look conceptually like this:

```rust
pub trait Plugin: Send + Sync {
    fn metadata(&self) -> PluginMetadata;
    fn dependencies(&self) -> &[ServiceKey];
    fn activate(
        &self,
        ctx: &mut PluginContext,
    ) -> BoxFuture<'_, anyhow::Result<PluginHandle>>;
}

pub struct PluginHandle {
    disposers: Vec<Disposer>,
}
```

The context would expose scoped registration operations:

```rust
ctx.effect(|| registry.register_tool(tool))?;
ctx.on("post_tool", handler)?;
ctx.provide("memory", memory_service)?;
```

The exact API should be driven by Rust ownership, `async` cancellation, error propagation, and testability rather than by TypeScript compatibility.

## Proposed implementation stages

### Stage 1: Standalone Rust core

Implement and test the runtime independently of Jcode:

- service keys and service storage
- plugin metadata
- dependency resolution
- activation and teardown
- scoped reversible effects
- event registration and dispatch
- deterministic teardown ordering
- clear errors for missing, cyclic, or conflicting dependencies

This stage should use mock services and test plugins. It should not require Jcode internals.

### Stage 2: Rust-native event and lifecycle semantics

Add the behavior that makes the runtime meaningfully Cordis-like:

- `emit`, `parallel`, `serial`, and `waterfall`
- plugin state transitions
- dependency changes and reactive activation/deactivation
- cancellation of plugin-owned async work
- idempotent teardown
- configuration reconciliation
- reload tests proving registrations disappear completely

### Stage 3: Jcode adapter layer

Integrate the standalone runtime behind adapters for existing Jcode systems:

- tool registry
- bus and hooks
- provider registry
- MCP tool registration
- prompt/context contributions
- session services
- background task ownership

The adapter layer should preserve existing Jcode behavior while gradually moving ownership into plugin scopes.

### Stage 4: Jcode built-in components

Convert a small number of existing subsystems into components, for example:

1. MCP management
2. provider/model catalog
3. memory or context providers
4. tool policy and authorization
5. observability hooks

Avoid converting all of Jcode at once. Each migration should prove activation, reload, teardown, and failure recovery.

### Stage 5: External plugin boundary

Only after the in-process model is stable, define a language-neutral external protocol. This could support Rust, Go, Python, or TypeScript plugins without putting TypeScript into the Jcode process.

Possible transports include:

- JSON-RPC over stdio
- Unix sockets
- a dedicated Jcode plugin protocol
- MCP for tool-only integrations
- Wasm for sandboxed components later

The protocol should not be designed before the lifecycle and ownership semantics are understood.

## How large is the rewrite?

The answer depends on what is included:

| Scope | Approximate difficulty |
| --- | --- |
| Minimal service container and plugin lifecycle | Medium |
| Reversible effects and scoped teardown | Medium to high |
| Typed event modes and waterfall middleware | Medium |
| Reactive dependency management and config reconciliation | High |
| Hot reload with reliable cleanup | High |
| Jcode adapters across tools, hooks, providers, MCP, and sessions | High |
| Mature external plugin ecosystem and compatibility guarantees | Very high |

The standalone core is a manageable focused project. The full Jcode integration is a major architectural effort because it must unify several existing systems without breaking session behavior, permissions, background tasks, or reload safety.

## What this would enable

If integrated carefully, the runtime could provide a common plugin model for:

- tools and commands
- model and provider integrations
- prompt and context augmentation
- agent policies
- tool authorization
- memory and retrieval
- logging and observability
- session persistence
- background jobs
- MCP bridges
- workflows and automation
- future external or sandboxed plugins

However, the runtime does not automatically make every subsystem pluggable. Each subsystem still needs a deliberate service contract and event boundary.

## Important constraints

### Do not load arbitrary Rust dynamic libraries initially

Rust does not provide a stable native ABI. Loading arbitrary `.so` or `.dylib` plugins would create compiler-version, dependency, memory-safety, and upgrade problems.

Prefer, in order:

1. built-in Rust components
2. a versioned out-of-process protocol
3. Wasm/WASI for sandboxed plugins
4. native dynamic loading only with a carefully designed C ABI, if it is ever needed

### Keep the first runtime independent

The Rust Cordis rewrite should not depend on Jcode's existing registries during its initial design. Otherwise it will inherit current coupling and become difficult to test or reuse.

### Treat teardown as a correctness property

A plugin that can register a tool but cannot reliably remove it is not fully integrated. Tests should verify that unloading removes every listener, tool, task, provider, and service owned by the component.

## Recommendation

Proceed with a standalone **Cordis-inspired Rust runtime** first. Do not put TypeScript into Jcode.

Use Cordis as the conceptual reference, but design the implementation around Rust's async runtime, ownership, cancellation, and error model. The eventual architecture could be:

```text
Cordis-inspired Rust runtime
        |
        +-- built-in Rust components
        +-- Jcode subsystem adapters
        +-- external process plugins
        +-- MCP tool plugins
        +-- future Wasm components
```

The primary payoff is not just more extensions. It is making Jcode's internal composition safe, reloadable, inspectable, dependency-aware, and reversible.
