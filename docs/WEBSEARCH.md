# Web search

Jcode's `websearch` tool keeps the legacy DuckDuckGo, Bing, and SearXNG
interfaces. Resilient orchestration is an opt-in addition. Existing
configurations and requests continue to use the legacy path unless resilience
is explicitly enabled.

## Legacy mode

The public tool name remains `websearch`. Its request fields are:

```json
{
  "query": "rust async",
  "num_results": 8,
  "engine": "duckduckgo",
  "bing_market": "en-US"
}
```

`query` is required. `num_results`, `engine`, and `bing_market` are optional.
The accepted engine names are `duckduckgo` (alias `ddg`), `bing`, and `searxng`
(alias `searx`). Legacy configuration remains valid:

```toml
[websearch]
engine = "duckduckgo"
fallback_engines = ["bing"]
bing_market = "en-US"
# bing_api_key_env = "JCODE_BING_API_KEY"
# searxng_url = "https://searx.example.org"
```

Legacy Bing uses the configured API key for the preferred Bing search when one
is available and uses keyless HTML for a fallback Bing search. The existing
result text, `ToolOutput` fields, and events are unchanged in legacy mode.

## Resilient mode

Enable the master switch explicitly:

```toml
[websearch.resilience]
enabled = true
duckduckgo_enabled = true
bing_enabled = true
searxng_enabled = true
fallback_enabled = true
attempt_timeout_ms = 10000
retries_enabled = true
max_retries = 1
health_suppression_enabled = true
health_failure_threshold = 2
health_cooldown_ms = 30000
diagnostics_enabled = true
```

The master switch defaults to `false`. The subcontrols default to useful,
bounded values when resilient mode is enabled:

| Setting | Default | Inclusive bounds |
|---|---:|---:|
| `attempt_timeout_ms` | `10000` | `100..60000` |
| `max_retries` | `1` | `0..2` |
| `health_failure_threshold` | `2` | `1..10` |
| `health_cooldown_ms` | `30000` | `1000..300000` |
| `fallback_enabled` | `true` | boolean |
| `retries_enabled` | `true` | boolean |
| `health_suppression_enabled` | `true` | boolean |
| `diagnostics_enabled` | `true` | boolean |

DuckDuckGo uses its existing HTML adapter. Resilient Bing always uses the free,
keyless HTML adapter, even when a legacy Bing API credential is configured.
SearXNG is used only when an explicitly trusted endpoint is configured. No
paid API, browser automation, CAPTCHA bypass, or untrusted public default is
introduced.

## Fallback order and precedence

For operational settings, the effective value is resolved independently in
this order:

1. request/session `resilience` override,
2. environment variable,
3. persisted `[websearch.resilience]` value,
4. built-in default.

The fallback order has one explicit compatibility exception:

1. request `resilience.fallback_order`,
2. `JCODE_WEBSEARCH_FALLBACK_ENGINES`,
3. existing persisted `websearch.fallback_engines`,
4. built-in `["bing"]`.

The preferred engine, whether selected by the request or by `websearch.engine`,
is prepended to that order. Duplicates are removed by stable first occurrence.
When `fallback_enabled = false`, only the preferred engine is considered.
Disabled, unavailable, and health-suppressed engines are recorded as skipped
and are never contacted.

The supported environment candidates are:

| Environment variable | Setting |
|---|---|
| `JCODE_WEBSEARCH_RESILIENCE_ENABLED` | master switch |
| `JCODE_WEBSEARCH_DUCKDUCKGO_ENABLED` | DuckDuckGo eligibility |
| `JCODE_WEBSEARCH_BING_ENABLED` | Bing eligibility |
| `JCODE_WEBSEARCH_SEARXNG_ENABLED` | SearXNG eligibility |
| `JCODE_WEBSEARCH_FALLBACK_ENGINES` | fallback order |
| `JCODE_WEBSEARCH_FALLBACK_ENABLED` | fallback control |
| `JCODE_WEBSEARCH_ATTEMPT_TIMEOUT_MS` | per-attempt timeout |
| `JCODE_WEBSEARCH_RETRIES_ENABLED` | retry control |
| `JCODE_WEBSEARCH_MAX_RETRIES` | retry allowance |
| `JCODE_WEBSEARCH_HEALTH_SUPPRESSION_ENABLED` | health control |
| `JCODE_WEBSEARCH_HEALTH_FAILURE_THRESHOLD` | health threshold |
| `JCODE_WEBSEARCH_HEALTH_COOLDOWN_MS` | health cooldown |
| `JCODE_WEBSEARCH_DIAGNOSTICS_ENABLED` | diagnostics control |
| `JCODE_WEBSEARCH_TRUSTED_SEARXNG_URL` | trusted SearXNG endpoint |

`JCODE_WEBSEARCH_ENABLED`, `JCODE_WEBSEARCH_TIMEOUT_MS`,
`JCODE_WEBSEARCH_HEALTH_ENABLED`, and `JCODE_WEBSEARCH_HEALTH_THRESHOLD` are
accepted compatibility aliases for the corresponding canonical environment
settings.

Invalid request overrides fail before network work. Invalid persisted policy
is an actionable configuration failure before network work. Invalid or empty
environment candidates produce a value-free warning and fall through to the
next source. Unknown engine names and unsupported fallback entries are not
silently accepted.

## Outcomes, retries, and health

A usable result has a non-empty title and a valid `http` or `https` URL. A
snippet is optional. Empty, partial, and malformed result sets are unusable.
Per-engine outcomes are:

- `success`: a usable result set was returned.
- `empty`: no usable result was parsed.
- `challenge`: an anti-automation or CAPTCHA page was detected.
- `timeout`: the finite attempt timeout elapsed.
- `transient`: transport, rate-limit, or server-unavailable failure.
- `permanent`: authentication, malformed, unsupported, or other non-retryable failure.
- `disabled`: the engine's eligibility control is off.
- `unavailable`: the engine cannot be used, such as SearXNG without a trusted endpoint.
- `health_suppressed`: the engine is in its short process-local cooldown.

Only transient and timeout outcomes retry, and retries use the fixed 200 ms
delay. With `E` eligible engines and `R = max_retries`, physical attempts are
bounded by `E * (1 + R)`, with `E <= 3`. Total work is bounded by
`E * (1 + R) * attempt_timeout + E * R * retry_delay`, plus scheduler
 tolerance. A challenge, empty result, or other non-retryable outcome proceeds
to the next eligible engine.

Health state is process-local and isolated by engine. A terminal challenge,
transient, or timeout sequence increments that engine once after its retries are
exhausted. A successful result resets the matching engine. Suppression begins at
the configured threshold, expires when `now >= suppressed_until`, and is cleared
before the next normal attempt. Health state is not persisted.

## SearXNG trust boundary

SearXNG endpoints must be explicitly supplied through persisted configuration
or the trusted endpoint environment variable. HTTPS is required. HTTP is
allowed only for loopback addresses (`localhost`, `127.0.0.1`, or `::1`) so
local fixtures can be used. URLs with userinfo, missing hosts, invalid syntax,
or untrusted HTTP hosts are rejected before network work. Jcode never infers a
public instance and never includes the endpoint in diagnostics.

For example:

```toml
[websearch]
searxng_url = "https://search.example.org"

[websearch.resilience]
enabled = true
trusted_searxng_url = "https://search.example.org"
```

The existing `websearch.searxng_url` field remains supported. Keep endpoint
values out of requests and prompts.

## Diagnostics and TUI visibility

When `diagnostics_enabled = true`, resilient searches attach bounded metadata
with schema version `jcode.websearch.diagnostics.v1`. It contains effective
non-secret controls, canonical considered engine names, skip/attempt/retry
counts, bounded elapsed time, the selected engine, per-engine outcomes, and the
aggregate search terminal outcome. It never contains the query, credentials,
secret environment values, response bodies, or endpoint URLs.

A clean first-attempt success has metadata but no extra diagnostic text. A
fallback, retry, suppression, or aggregate failure adds at most one compact
summary line. The existing TUI prefers the resulting `ToolOutput` title for
`websearch`, showing only the selected engine for clean success or a bounded
attempt summary for meaningful activity. The title and summary are capped at
96 display characters. With diagnostics disabled, optional metadata and
aggregate detail are omitted while normal results and actionable failures
remain available.
