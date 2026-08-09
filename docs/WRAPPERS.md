# jcode wrapper / scripting guide

This document describes the non-interactive CLI surface intended for wrappers, scripts, and other tools that invoke `jcode`.

## Recommended flags

Use these flags by default in wrappers:

```bash
jcode --quiet --no-update --no-selfdev ...
```

- `--quiet` suppresses non-error CLI/status chatter
- `--no-update` avoids update-check noise/work
- `--no-selfdev` avoids repository auto-detection changing runtime behavior

## Discover available models

List model names that can be passed to `-m/--model`:

```bash
jcode --quiet model list
jcode --quiet model list --json
jcode --quiet --provider openai model list --json
```

## Discover providers and current selection

List provider IDs you can pass to `-p/--provider`:

```bash
jcode --quiet provider list
jcode --quiet provider list --json
```

Inspect the currently requested and resolved provider/model selection:

```bash
jcode --quiet provider current
jcode --quiet --provider openai --model gpt-5.4 provider current --json
```

Verbose human summary:

```bash
jcode --quiet model list --verbose
```

## Run one prompt and return JSON

```bash
jcode --quiet run --json "Reply with exactly OK"
```

## Run with a named session profile

Profiles are defined under `[profiles.<name>]` in `~/.jcode/config.toml` and
selected per invocation. They apply provider/model/reasoning, tool, skill, and
additional-instruction defaults without editing the config or affecting another
session:

```bash
jcode --quiet --profile review run "Review this change"
```

`--profile` is global, so `jcode --quiet run --profile review "Review this
change"` is equivalent. Explicit flags and environment overrides retain their
normal precedence. Unknown profiles and invalid fields fail before a provider
request with a corrective diagnostic. Existing `run --json`, `run --ndjson`, and
`run --schema` modes keep their established headless behavior; an omitted profile
continues to use the no-profile path.

Wrappers can inspect profile state without starting a provider or mutating a session:

```bash
jcode --quiet profile list --json
jcode --quiet profile show review --json
jcode --quiet profile current --json
jcode --quiet profile resolve review --json
```

`profile show` exposes configured non-secret fields and instruction presence/length;
`current` and `resolve` include effective tool/skill policy and field sources. For a
human TUI session, `jcode --profile review` starts with the selected profile; `/profile`
opens the picker, `/profile <name>` queues a future-turn switch, and `/profile none`
clears it. Restored sessions retain their saved snapshot and report missing/changed
profiles rather than silently adopting a different one. Child agents and swarm workers
inherit the effective restrictions unless explicitly overridden.

Structured `--schema` runs use the SDK bridge and cannot hand a per-session
profile to an already-running shared daemon. A profiled structured run therefore
fails before the request if that daemon is already active; stop it with
`jcode server stop` or select a private/new server socket before retrying.

## Run one prompt with schema-validated JSON

Pass a UTF-8 JSON Schema file to make the Rust SDK parse and validate the final
response. The SDK makes at most two corrective retries when the model returns
invalid JSON or a schema mismatch:

```bash
jcode --quiet run --schema result-schema.json "Return the requested result"
```

The successful stdout object has this stable shape:

```json
{
  "data": {"status": "ok"},
  "session_id": "session_...",
  "provider": "OpenAI",
  "model": "gpt-5.4",
  "attempts": 1,
  "usage": {
    "input_tokens": 123,
    "output_tokens": 7,
    "cache_read_input_tokens": 0
  }
}
```

`data` is the value that passed the supplied schema. `attempts` includes the
initial response and any corrective retries. Invalid or unreadable schema files,
and exhausted structured-output retries, return a non-zero exit status with an
actionable error on `stderr`; no partial structured result is printed to stdout.
`--schema` is mutually exclusive with `--json` and `--ndjson`.

## Stream one prompt as NDJSON

```bash
jcode --quiet run --ndjson "Reply with exactly OK"
```

Typical event types:

- `start`
- `connection_phase`
- `connection_type`
- `text_delta`
- `text_replace`
- `tool_start`
- `tool_input`
- `tool_exec`
- `tool_done`
- `tokens`
- `done`
- `error`

The final `done` event includes the assembled text and usage summary.

Example shape:

```json
{
  "session_id": "session_...",
  "provider": "OpenAI",
  "model": "gpt-5.4",
  "text": "OK",
  "usage": {
    "input_tokens": 123,
    "output_tokens": 7,
    "cache_read_input_tokens": 0,
    "cache_creation_input_tokens": null
  }
}
```

## Inspect authentication state

```bash
jcode --quiet auth status
jcode --quiet auth status --json
```

JSON output includes:

- `any_available`
- `providers[]`
  - `id`
  - `display_name`
  - `status`
  - `method`
  - `auth_kind`
  - `recommended`

## Inspect build/version details

```bash
jcode --quiet version
jcode --quiet version --json
```

JSON output includes:

- `version`
- `git_hash`
- `git_tag`
- `build_time`
- `git_date`
- `release_build`

## Notes

- JSON commands are designed so the intended machine-readable result is printed to `stdout`
- With `--quiet`, wrapper-oriented commands should keep `stderr` empty unless there is a real warning/error
- `jcode model list` and `jcode run --json` do not require the TUI
- `jcode model list` does not require an already-running shared server
