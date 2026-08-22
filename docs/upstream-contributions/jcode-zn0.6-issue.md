# Draft upstream issue: Add a narrow `--max-turns` safety bound to `jcode run`

> **Draft only. Do not post without Ariel's explicit approval.**
>
> Proposed branch link: `<fork branch link after approval>`
>
> Related upstream issue: [#803](https://github.com/1jehuang/jcode/issues/803)

## Summary

Add an invocation-level `--max-turns N` option to `jcode run`. The option accepts a positive decimal whole number, stops the unattended run after that many completed top-level turns, and reports a stable `max_turns_exceeded` reason in plain, JSON, and NDJSON output.

This is intentionally narrower than a general run-budget system. It does not add persisted defaults, environment-variable configuration, tool-step limits, token budgets, deadlines, or agent-core policy.

## Problem

`jcode run` can continue beyond its initial provider turn when auto-poke follow-ups are enabled. Scripts and unattended callers currently have no invocation-level way to place a hard upper bound on completed turns. The existing `JCODE_RUN_AUTO_POKE_MAX_TURNS` setting is internal to auto-poke diagnostics and does not provide a public CLI contract or a stable structured stop reason.

A small `--max-turns` boundary gives operators a predictable safety limit while preserving current behavior when the flag is absent.

## Proposed minimal behavior

1. Accept `jcode run --max-turns N <message>` in plain, `--json`, and `--ndjson` modes.
2. Require `N` to be a positive decimal whole number. Reject empty, zero, negative, fractional, and non-numeric values before provider initialization.
3. Count only successfully completed top-level agent turns.
4. Stop before starting another turn once the configured count is reached.
5. Emit a stable stop contract:
   - plain stderr: `Run stopped: maximum turns exceeded (max_turns_exceeded)`
   - JSON result fields:
     - `"stop_reason": "max_turns_exceeded"`
     - `"outcome": "bounded_stop"`
     - `"safety_bound": {"bound": "max_turns", "source": "invocation"}`
   - NDJSON: the same fields on the final `done` event
6. Preserve legacy output and behavior when `--max-turns` is absent.
7. Keep the existing `JCODE_RUN_AUTO_POKE_MAX_TURNS` behavior separate and unchanged.

## Non-goals

- Persisted `config.toml` defaults or a new environment variable for the public limit
- Tool-call or tool-step limits
- Token, cost, or wall-clock budgets
- Agent-core cancellation or policy changes
- Changing auto-poke follow-up selection
- Treating provider failures as completed turns
- Posting this issue, publishing a branch, or opening a pull request without approval

## Upstream base and working implementation

Port base:

- `upstream/master`
- commit `a63dbc4546895ecb4d1be1a285d98e6e13fb1b74`

The narrow implementation reuses proven ideas from the fork's broader unattended-run safety work:

- `6f6a4bf58` initial bounded unattended run safety
- `bac81b00a` review fixes
- `df0529e96` safety refactor
- `367d83f86` review findings
- `3bd1bbe84` positive-integer parser test format
- `786907675` error guardrail
- merge commits `e0f9a6811` and `e9cb1cc47`

The broader fork implementation also includes tool-step, token, deadline, configuration, and agent-core changes. Those were deliberately excluded. Autospec/spec artifacts were also excluded at Ariel's direction. This contribution contains only the minimal CLI boundary, run-loop integration, output annotation, and focused tests needed on the upstream base.

## Files changed

- `src/cli/args.rs`
- `src/cli/args/tests.rs`
- `src/cli/dispatch.rs`
- `src/cli/commands.rs`
- `src/cli/commands/run_safety.rs`
- `src/cli/commands_tests.rs`

## Implementation notes

- Clap preserves the raw string so validation occurs in the run command before provider initialization.
- `RunTurnLimit` owns parsing, completed-turn accounting, and the stable stop reason.
- The plain, captured JSON, and streaming NDJSON loops call the same limit after each successful top-level turn.
- JSON output is converted to `serde_json::Value` and annotated only when the safety bound stops the run. This leaves the legacy report shape unchanged when the option is absent.
- NDJSON annotates only the terminal `done` event. Existing streaming and auto-poke events are unchanged.
- The existing auto-poke-only maximum was renamed locally to `auto_poke_max_turns` for clarity but retains its prior behavior.

## Compatibility and edge cases

Covered behavior includes:

- Missing `--max-turns` remains unbounded and adds no output fields.
- Surrounding whitespace is accepted around a valid positive integer.
- `0`, negative numbers, fractions, empty strings, and text are rejected.
- Invalid values fail before provider initialization or network work.
- Only successful turns increment the count.
- Plain, JSON, and NDJSON modes share the same stable reason code.
- Existing `--json` and `--ndjson` conflict behavior is unchanged.
- Existing auto-poke diagnostics and `JCODE_RUN_AUTO_POKE_MAX_TURNS` remain separate.
- Session cleanup still runs after successful bounded stops and provider errors.

## Related upstream issue and duplicate analysis

[#803](https://github.com/1jehuang/jcode/issues/803) directly requests a `--max-turns N` equivalent for `jcode run`, including a clear stop reason and machine-readable output. This draft is a deliberately narrow implementation of that request.

The existing `JCODE_RUN_AUTO_POKE_MAX_TURNS` code is not a duplicate public interface. It is an environment-driven auto-poke-specific diagnostic guard and does not expose a CLI option or stable JSON/NDJSON bounded-stop contract.

## Validation evidence

Test-first evidence on the actual upstream-based worktree:

1. Focused parser and safety-state tests were added before production code.
2. The first exact command targeted `--bin jcode`; the repository guard correctly reported zero matching tests. The corrected `--lib` target then compiled the intended CLI modules.
3. The initial implementation compile exposed five integration updates needed in existing parser and one-shot tests. Those call sites were repaired once.
4. Exact-worktree focused validation with `JCODE_IN_DEV_CARGO=1` passed:
   - `max_turns`: 4 passed, covering parsing, completed-turn accounting, exact plain stop text, and JSON/NDJSON annotation fields
   - `unset_limit`: 1 passed, covering legacy unbounded behavior
   - existing JSON and NDJSON parser tests: 1 passed each
   - existing plain/JSON/NDJSON session-cleanup test: 1 passed
   - existing resumed-session cleanup test: 1 passed
   - existing provider-error cleanup test: 1 passed
5. `JCODE_IN_DEV_CARGO=1 cargo fmt -p jcode -- --check` passed.
6. `JCODE_IN_DEV_CARGO=1 ./scripts/dev_cargo.sh clippy -p jcode --lib --no-deps` passed. The only emitted warning was pre-existing dead-code noise in `jcode-harness-api-server`.
7. `JCODE_IN_DEV_CARGO=1 ./scripts/dev_cargo.sh build --profile selfdev -p jcode --bin jcode` passed.
8. The built worktree binary exposes `--max-turns <N>` in `jcode run --help`.
9. The built worktree binary rejects `--max-turns 0` with the stable preflight error before provider initialization.

Broader suite diagnosis:

- The full `jcode` library suite ran 236 tests: 228 passed and 8 failed in existing upstream-base areas unrelated to this branch. Failures covered auth lifecycle sandbox state, debug-control-dependent restart snapshots, provider catalog/choice drift, a hosted-provider display-name expectation, and an existing auto-poke wording assertion.
- The existing auto-poke wording test also fails in isolation at unchanged `commands_tests.rs:333`; this branch does not modify that helper or assertion. All changed-path and one-shot integration filters pass.

No live provider call, upstream push, issue creation, pull request, or GitHub comment was performed.

## Proposed acceptance criteria

- [ ] `jcode run --max-turns N` accepts positive whole numbers and rejects invalid values before provider initialization.
- [ ] The run performs no more than `N` successfully completed top-level turns.
- [ ] Plain output reports `maximum turns exceeded (max_turns_exceeded)` when bounded.
- [ ] JSON and NDJSON terminal output include the stable bounded-stop fields when bounded.
- [ ] Unset behavior and output remain backward compatible.
- [ ] Existing auto-poke-specific limits remain unchanged.
- [ ] Focused tests, formatting, and a built `jcode` binary pass on the upstream base.
