# PR 4 bounded tool-round reconciliation matrix

## Decision summary

Current `dev` is authoritative. Fork PR 4 correctly identifies that `--max-turns` alone does not stop an infinite provider/tool loop inside one turn, but its standalone CLI implementation duplicates and narrows the richer current run-safety architecture. Port the missing per-turn safety invariant into `RunSafetyController`; preserve current precedence, validation, explicit global tool-step/token/deadline controls, harness/protocol reporting, and structured output.

## Behavior matrix

| Candidate behavior | Decision | Current-dev-compatible design | Direct check |
|---|---|---|---|
| `max_turns` implies a tool-loop bound | **Port** | Effective max_turns enables a fixed 32 completed provider/tool rounds per turn | Resolver/controller tests for invocation, environment, persisted, and unset sources |
| Stop before another provider request | **Port** | Check both capture and streaming loop heads before request 33 | Fake-provider request-count tests |
| Count one provider response containing tool calls | **Port** | Increment after the response is fully processed; do not count text completion, errors, cancellation, or partial processing | Focused accounting tests |
| Reset the count for each outer turn | **Port** | Reset in `before_turn` | Two-turn reset test |
| Typed tool-round stop | **Port with current naming** | Add `max_tool_rounds_exceeded`, bound 32, inherited max_turns source | Stop-reason serialization tests |
| Plain diagnostic on stderr | **Port** | Change bounded plain reporting from stdout to stderr | CLI channel test |
| JSON/NDJSON structured stop | **Preserve/extend** | Reuse the current shared report and optional reason fields | JSON parse and NDJSON event tests |
| Existing explicit `max_tool_steps` | **Preserve** | Remains invocation-global Registry execution count and may stop earlier | Priority test with a lower explicit tool-step bound |
| Existing token/deadline/max-turn controls | **Preserve** | Keep current thresholds, first-stop-wins state, and deterministic priority | Existing focused test suite |
| Current validation and source precedence | **Preserve** | Invocation > environment > persisted > unset remains canonical | Existing resolver and invalid-input tests |
| Harness/protocol typed outcomes | **Preserve/extend** | Map the new shared reason through existing result boundaries only | Harness/protocol compatibility tests |
| PR-only parallel CLI loop and output schema | **Reject** | No duplicate counter, resolver, or result type | Scope/diff review |
| User-configurable `max_tool_rounds` flag | **Reject** | Fixed internal 32-round invariant activated by max_turns | Parser regression confirms no new flag |
| PR2 code or unrelated cleanup | **Reject** | No changes outside matrix-owned run-safety paths | Final diff review |

## Counting and stop sequence

1. `before_turn` resets the per-turn completed-round count.
2. Before every provider request, existing deadline/token checks run, then the round-limit check runs.
3. A provider response is processed normally.
4. If it contained tool calls and completed processing, increment the completed-round count once.
5. After round 32, tools from that response may finish, but the next provider request is suppressed and the typed bounded-stop result is emitted.
6. An earlier explicit deadline, token budget, or max-tool-steps stop remains the primary first stop.

## Risk review

**Risk level: High before validation, Medium after focused and fresh-base checks.** The change sits inside asynchronous provider loops and machine-readable result contracts. An off-by-one error can issue an unwanted provider request; counting at the wrong seam can stop before promised tool work finishes or double-count multiple calls in one response; output-channel mistakes can corrupt JSON consumers. Blast radius is limited to unattended runs with effective max_turns plus the plain bounded-stop channel change. Mitigations are shared-controller ownership, identical capture/streaming tests, explicit request-count assertions, reset/accounting tests, existing-priority regression coverage, exact-source builds, and fresh-base guardrails. Rollback is the focused merge commit. Required review action is to confirm every product edit maps to a Port row and that existing safety controls are not weakened.

## Validation record

- Autospec-compatible acceptance artifacts: **COMPLETE**.
- TDD red/green evidence: **PASS**. The first focused compile failed on the missing policy field, reason, metadata limit, and controller APIs; the implemented controller and both provider-loop tests then passed.
- Exact-source focused tests and build: **PASS**. `cargo test -p jcode-app-core run_safety`, `cargo test -p jcode run_safety`, and `scripts/dev_cargo.sh build --profile selfdev -p jcode --bin jcode` passed with `JCODE_IN_DEV_CARGO=1` and isolated `CARGO_TARGET_DIR=/home/ari/.jcode/scratch/jcode-5hc-pr4-red-target`.
- Formatting, Clippy, and repository guardrails: **PASS after one repair pass**. The full guardrail run passed every gate except oversized-file and panic-prone budgets; the repair moved helper code to the existing run-safety owner, kept oversized files flat or smaller, removed the new panic-prone call, and the required focused tests, affected-package Clippy with `-D warnings`, both failed ratchets, formatting, and diff checks passed.
- Isolated runtime checks: **PASS**. The exact-worktree selfdev binary rejected `--max-turns 0` before provider initialization on a private socket and exposed no user-facing `max-tool-round` flag.
- Fresh-base merge, install, reload, and closure: pending.
