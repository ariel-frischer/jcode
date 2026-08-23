# Non-interactive Run Safety

Use the invocation-level `--max-turns` option to bound a single `jcode run`
process. It limits both completed agent turns and the tool loop inside each turn:

```bash
jcode run --max-turns 3 "Complete the task, then verify it"
```

The value must be a positive decimal whole number. Jcode validates it before
provider initialization. If `JCODE_RUN_AUTO_POKE_MAX_TURNS` is also configured,
the lower bound stops first. When both limits are equal, the invocation-level
`--max-turns` bound wins before another automated follow-up is scheduled.

Each turn in a bounded invocation may execute at most 32 provider/tool rounds.
After the 32nd tool round, Jcode stops before starting another provider request.
This inner bound applies to plain, JSON, and NDJSON runs and prevents one agent
turn from postponing the invocation safety check indefinitely. Runs without
`--max-turns` retain the existing unbounded tool-loop behavior.

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

When the completed-turn bound stops a run:

- plain output writes `Run stopped: maximum turns reached (max_turns_reached)`
  to stderr after the response
- JSON output adds `stop_reason: "max_turns_reached"`,
  `outcome: "bounded_stop"`, and
  `safety_bound: {"bound":"max_turns","source":"invocation"}`
- the final NDJSON `done` object adds the same structured fields

When the per-turn tool-round bound stops a run, the corresponding values are:

- plain stderr: `Run stopped: maximum tool rounds reached (max_tool_rounds_reached)`
- JSON and final NDJSON `done` object:
  `stop_reason: "max_tool_rounds_reached"`, `outcome: "bounded_stop"`, and
  `safety_bound: {"bound":"max_tool_rounds_per_turn","source":"invocation"}`

A bounded stop is a successful, deliberate outcome and exits with status 0.
Structured modes write valid JSON only to stdout. Stop fields are omitted when
neither invocation bound stopped the run.
