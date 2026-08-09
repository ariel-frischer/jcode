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

`max_turns` counts completed top-level turns, including auto-poke follow-ups.
`max_tool_steps` counts jcode Registry executions immediately before they
start; provider-internal tools are not controllable Registry work. The token
budget is the current invocation's delta of native input, output, cache-read,
and cache-creation usage, with saturating arithmetic. Resumed-session history
is the baseline and is not charged to the new run.

The deadline is converted once to a monotonic instant and observed before
provider requests, between stream events, before Registry calls, and after
active tools. When multiple bounds are reached at one checkpoint, the primary
reason is selected in this order: deadline, token budget, tool steps, turns.

## Results

Plain output prints a stable bounded-stop label. JSON and NDJSON add fields only
when a bound stops the run:

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

Canonical codes are `max_turns_exceeded`, `max_tool_steps_exceeded`,
`token_budget_exceeded`, and `deadline_exceeded`. The `safety_bound` object names
the effective bound and its winning source (`invocation`, `environment`, or
`persisted`). Ordinary completion, provider errors, and cancellation remain
distinct. Explicit safety flags are rejected with `--schema`, because that mode
uses a separate SDK bridge; persisted and environment bounds are not silently
advertised as enforced there.
