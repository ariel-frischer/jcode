# Non-interactive Run Safety

Use the invocation-level `--max-turns` option to bound completed agent turns in a
single `jcode run` process:

```bash
jcode run --max-turns 3 "Complete the task, then verify it"
```

The value must be a positive decimal whole number. Jcode validates it before
provider initialization. If `JCODE_RUN_AUTO_POKE_MAX_TURNS` is also configured,
the lower bound stops first. When both limits are equal, the invocation-level
`--max-turns` bound wins before another automated follow-up is scheduled.

## Validate this feature

The quickest validation does not require a provider request:

```bash
jcode run --max-turns 0 "This must not reach the provider"
```

The command must fail immediately with:

```text
--max-turns must be a positive decimal whole number
```

For an end-to-end check with your configured provider, run:

```bash
jcode run --max-turns 1 "Reply with exactly: ONE_TURN_OK"
```

The response should contain `ONE_TURN_OK` and then stop after the first completed
agent turn.

## Output contract

When the invocation bound stops a run:

- plain output writes `Run stopped: maximum turns reached (max_turns_reached)`
  to stderr after the response
- JSON output adds `stop_reason: "max_turns_reached"`,
  `outcome: "bounded_stop"`, and
  `safety_bound: {"bound":"max_turns","source":"invocation"}`
- the final NDJSON `done` object adds the same structured fields

These fields are omitted when the invocation bound did not stop the run.
