# OpenAI Responses WebSocket v2

Jcode's native OpenAI providers (`openai` and `openai-api`) prefer persistent
Responses WebSockets in `auto` transport mode. Every new socket, including a
prewarm socket, sends:

```text
OpenAI-Beta: responses_websockets=2026-02-06
```

This selects the WebSocket v2 protocol. It is not a `/v2/responses` endpoint.
API-key requests use `/v1/responses` (or the configured Responses API base).
ChatGPT/Codex OAuth requests use the subscription Responses backend. Custom
Chat Completions providers are unaffected.

## What prewarming does

When an idle client subscribes, Jcode snapshots its tools and static instructions
and starts preparation while the user types. This snapshot does not pin tools or
consume late MCP discovery. Idle preparation is polled once after acknowledging
the subscription. If local preparation would yield, it is abandoned rather than
holding the agent lock in front of user input. The agent also tries prewarming before local turn
context preparation. Both hooks prepare the static prefix in the background
using `response.create` with `generate: false`, `input: []`, and `store: false`.
The server returns a completed response ID without model
output. If the warmup finishes before generation is needed, Jcode continues on
that socket with `previous_response_id` and the actual conversation input.

- Warmup never executes tools or emits assistant output into the conversation.
- The foreground request does not wait for an unfinished warmup. It cancels it
  and follows the ordinary connection path instead.
- Every request setting must still match, including model, instructions, tools,
  reasoning effort, service tier, and cache policy. Credentials and endpoint
  must also match the warmup handshake.
- A successful existing conversation socket takes precedence over warmup.
- Warmup has a 5-second timeout. Unused state expires after 30 seconds. Model,
  credential, and transport resets discard speculative state. Forks do not
  inherit a parent's warmup socket.
- Expiring credentials skip warmup. Speculative work never rotates OAuth refresh
  tokens, so cancellation cannot discard newly issued credentials.
- Warmup errors do not fail the user's request or put the model into a transport
  cooldown. Ordinary WebSocket failure recovery and HTTPS fallback still apply.

The benefit depends on having preparation time to overlap with the network
work. A warmup miss is not treated as a failure. There is no guaranteed speedup
from the version header alone, and sending fewer bytes does not make earlier
context free of token charges.

## Controls and diagnostics

```toml
[provider]
openai_transport = "auto" # auto | websocket | https
```

Prewarming is enabled by default for native OpenAI WebSockets. Set
`JCODE_OPENAI_PREWARM=0` (also `false` or `off`) in the **server process** environment
to disable speculative warmup without disabling persistent WebSockets. Setting
the transport to `https` disables both WebSockets and their warmup.

The provider's diagnostic summary includes `websocket_protocol=v2`. Lifecycle
logs include `ws_prewarm_ready`, `ws_prewarm_hit`, `ws_prewarm_miss`, and
`ws_prewarm_unavailable`. A hit uses the normal `websocket/persistent-reuse`
connection label. Logs do not include credential identities or warmup inputs.

## Stalled output recovery

Native OpenAI Responses streams, including HTTPS fallback, have an enabled-by-default
meaningful-progress watchdog:

```toml
[provider]
openai_stall_recovery = true
openai_stall_timeout_secs = 300
```

The timeout is seconds without substantive model output, not total turn duration.
Text, reasoning, and streamed tool arguments refresh it. Transport pings, SSE
comments, empty deltas, and lifecycle-only traffic do not. A timeout uses the
existing provider retry/fallback path, with three total attempts on the fresh
request path. Partial output is rolled back before replay and unusable persistent
state is discarded. This does not add a poke loop or replay already executed local
tools. Cancellation remains available while waiting for output.

Set `openai_stall_recovery = false` to disable this new watchdog while retaining
the existing transport timeouts and retry behavior. The base range is 1–3600
seconds. Persisted numeric values outside that range are clamped at runtime.
Reasoning effort uses the existing multiplier: low/medium 1×, high 2×, xhigh 3×,
max 4×. Existing `[provider] stream_idle_timeout_secs` and WebSocket first-event
and completion limits remain independent and can expire earlier. Raising only
the new setting does not extend those existing limits.

Server environment variables override persisted settings:

- `JCODE_OPENAI_STALL_RECOVERY`: normal boolean spellings such as `true`, `false`,
  `on`, and `off`.
- `JCODE_OPENAI_STALL_TIMEOUT_SECS`: an integer from 1 through 3600.

Empty, invalid, or out-of-range environment values leave the persisted/default
value unchanged. `/config` reports the configured and effective base timeout.
No client-only setting changes a remote server's provider policy. Restart or
deliberately reload that server to activate a new build or server environment.

Long silent reasoning is not proof of a lost request. Recovery can restart useful
generation and consume additional quota. Increase the budgets or disable the new
watchdog for workloads that intentionally reason silently for longer.

The TUI separately labels current thinking-phase time and cumulative `turn` time.
The latter includes tool rounds and automatic follow-ups, so a 15-minute turn does
not imply a single 15-minute provider request. Retry status remains separate from
the total-turn clock.

### Other harnesses

Checked 2026-09-09: [Codex's configuration reference](https://developers.openai.com/codex/config-reference)
documents `stream_idle_timeout_ms` (300000 ms by default), `stream_max_retries`
(5), and separate HTTP request retries (4). [OpenCode's LLM adapter](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/llm.ts)
passes an abort signal to provider execution, aborts it when its stream scope
closes, and explicitly controls the AI SDK retry count (`input.retries ?? 0`).
These support using transport deadlines, deliberate retry ownership, and
cancellation rather than injecting generic continuation prompts. They do not
prove either harness can distinguish every lost request from silent reasoning.

## Verification

Run the runtime's offline regression suite:

```bash
cargo test -p jcode-provider-openai-runtime --lib -- --test-threads=1
```

The opt-in live test uses configured credentials and a few short model requests.
It checks a cold v2 connection, warmup consumption, and subsequent continuation
using the newly compiled provider, independently of the shared daemon:

```bash
cargo test -p jcode-provider-openai-runtime --lib \
  live_openai_v2_prewarm_and_continuation -- --ignored --nocapture --test-threads=1
```

Its single-sample time-to-first-text observations are not a benchmark. A real
latency comparison should measure cold and warmed requests across many turns,
report warmup hit rate, and include preparation cost when it cannot overlap
other work. See the [application-level validation report](OPENAI_WEBSOCKET_VALIDATION.md)
for a repeated enabled-versus-disabled experiment and its limitations.

Native `response.steer` and multiplexed `stream_id` support are separate features
and are not implemented by this change.

Sources: [OpenAI WebSocket guide](https://developers.openai.com/api/docs/guides/websocket-mode)
and [OpenAI Codex client](https://github.com/openai/codex/blob/main/codex-rs/core/src/client.rs).
