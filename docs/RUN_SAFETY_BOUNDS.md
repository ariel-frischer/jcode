# Unattended run safety bounds

`jcode run` accepts optional, per-invocation safety bounds. When every bound is
unset, execution and output retain the legacy behavior.

## Configuration and precedence

Each field resolves independently in this order:

1. Invocation flags: `--max-turns`, `--max-tool-steps`, `--token-budget`, and
   `--deadline`.
2. Environment: `JCODE_RUN_MAX_TURNS`, `JCODE_RUN_MAX_TOOL_STEPS`,
   `JCODE_RUN_TOKEN_BUDGET`, and `JCODE_RUN_DEADLINE`.
3. Persisted `[run_safety]` fields with the same names.
4. Unset (disabled).

Persisted configuration uses this TOML shape:

```toml
[run_safety]
max_turns = "10"
max_tool_steps = "100"
token_budget = "100000"
deadline = "2030-01-01T00:00:00Z"
```

Values remain raw until the resolver validates them. Positive bounds are
decimal whole numbers greater than zero. Deadlines are future absolute RFC3339
timestamps with an explicit offset, for example
`2030-01-01T00:00:00Z` or `2030-01-01T01:00:00+01:00`. Empty, whitespace-only,
malformed, zero, negative, overflowing, and past values fail before provider or
tool work starts; an invalid higher-precedence value never falls through to a
lower source.

## Accounting and enforcement

`max_turns` counts completed top-level turns, including auto-poke follow-ups. An
effective `max_turns` value also enables a fixed limit of 32 completed
provider/tool rounds inside each turn. A round is one fully processed provider
response containing tool calls. The count advances only after those tool calls
have been handled and resets before the next top-level turn. Jcode stops before
provider request 33. The implicit bound inherits the effective `max_turns`
source, is not separately configurable, and is disabled when `max_turns` is
unset.

`max_tool_steps` is separate: it counts jcode Registry executions immediately
before they start across the entire invocation. Multiple tool calls in one
provider response consume one provider/tool round but may consume several tool
steps. Provider-internal tools are not controllable Registry work. The token
budget is the current invocation's delta of native input, output, cache-read,
and cache-creation usage, with saturating arithmetic. Resumed-session history is
the baseline and is not charged to the new run.

The deadline is converted once to a monotonic instant and observed before
provider requests, between stream events, before Registry calls, and after
active tools. When multiple bounds are reached at one checkpoint, the primary
reason is selected in this order: deadline, token budget, tool steps, tool
rounds, turns.

## Results

Plain output prints a stable bounded-stop label to stderr. JSON and NDJSON keep
stdout machine-parseable and add fields only when a bound stops the run:

```json
{
  "stop_reason": "token_budget_exceeded",
  "outcome": "bounded_stop",
  "observed_usage": 100000,
  "safety_bound": {
    "bound": "token_budget",
    "source": "environment"
  }
}
```

The per-turn stop uses `max_tool_rounds_exceeded` and reports
`{"bound":"max_tool_rounds","source":"<max_turns source>","limit":32}`.
The other canonical codes are `max_turns_exceeded`, `max_tool_steps_exceeded`,
`token_budget_exceeded`, and `deadline_exceeded`. The `safety_bound` object names
the effective bound and its winning source (`invocation`, `environment`, or
`persisted`). Ordinary completion, provider errors, and cancellation remain
distinct. Explicit safety flags are rejected with `--schema`, because that mode
uses a separate SDK bridge; persisted and environment bounds are not silently
advertised as enforced there.
