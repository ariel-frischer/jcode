# Memory runtime routing note

> **Status:** Verified local behavior
> **Verified:** 2026-09-20 against installed build `8e67bb7af`
> **Scope:** Ariel's custom Jcode deployment after the Jev memory merge

## Effective behavior

Persistent memory now has two independent model paths:

1. **Recall uses Jev Decisions.** Automatic recall sends the eligible local project and global memories to Jev for typed relevance decisions. It does not use embeddings, the generative memory sidecar, or the former consensus reranker, and it fails closed rather than falling back to those paths.
2. **Writing uses the configured sidecar.** Optional transcript extraction remains enabled and uses `agents.memory_model = "gpt-5.6-luna"` with `agents.memory_reasoning_effort = "max"`. One sidecar completion is made per extraction trigger. Extraction can run every 12 processed turns and at session end.

The retained settings below no longer control automatic recall:

```toml
memory_rerank_cadence = 3
memory_rerank_votes = 2
memory_rerank_min_agree = 2
memory_embedding_backend = "local"
```

In particular, `memory_rerank_votes = 2` does **not** start two Luna judges. The two-vote consensus implementation remains in the source tree for compatibility or historical tooling, but it has no production recall caller, and the vote configuration has no runtime reader.

## Current provider resolution

`agents.memory_jev_provider` is not explicitly set, so it defaults to `auto`. Auto prefers Jcode subscription access, then OpenRouter, TypeSafe, and AI/ML API. At verification time, Jcode subscription credentials were unavailable and OpenRouter credentials were available, so memory recall resolved to OpenRouter using `typesafe/jev-1.13`.

This route can change if credential availability or `agents.memory_jev_provider` changes. Never infer the active provider solely from the retained Luna or rerank settings.

## Verification receipt

A whole-result check passed 12 of 12 assertions across the live configuration, production call sites, provider resolution, and post-install runtime outcomes. After the installed-build timestamp, the runtime produced pending and injected recall results, and all 21 observed extraction starts completed. This verifies routing and execution, not comparative relevance quality versus the former two-Luna consensus policy.
