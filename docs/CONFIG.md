# Jcode Configuration

Jcode reads persisted user configuration from `~/.jcode/config.toml`. Feature-specific
runtime overrides may also come from environment variables or explicit invocation
arguments. Invalid higher-precedence values fail closed rather than falling through
to a lower-precedence value.

## Session librarian

The session librarian is a manual, one-shot workflow. Merely configuring it,
registering its tool, or starting a session performs no librarian processing,
provider request, or artifact write. Cost and work begin only after an explicit
`/session-librarian` invocation or a direct `session_librarian` tool call.

### Persisted configuration

All persisted values are optional strings under `[session_librarian]`. Keeping the
raw text until invocation lets the resolver distinguish an omitted value from an
explicit empty, malformed, non-positive, or unsupported value.

```toml
[session_librarian]
provider = "openai-oauth"
model = "gpt-5.6-luna"
reasoning_effort = "xhigh"
max_input_tokens = "12000"
max_output_tokens = "2500"
max_requests = "1"
max_cost_usd = "0.50"
deadline_seconds = "120"
```

These example values are non-secret route and budget settings. Authentication is
resolved through the selected provider's existing credential flow. Do not place
OAuth tokens, API keys, or other live credentials in this table or in invocation
arguments.

### Settings, environment variables, and defaults

| Persisted field | Environment variable | Built-in default | Valid value |
|---|---|---:|---|
| `provider` | `JCODE_SESSION_LIBRARIAN_PROVIDER` | `openai-oauth` | Non-empty provider route name supported by the provider registry. |
| `model` | `JCODE_SESSION_LIBRARIAN_MODEL` | `gpt-5.6-luna` | Non-empty model name supported by the selected route. |
| `reasoning_effort` | `JCODE_SESSION_LIBRARIAN_REASONING_EFFORT` | `xhigh` | Non-empty effort accepted by the selected provider and model. |
| `max_input_tokens` | `JCODE_SESSION_LIBRARIAN_MAX_INPUT_TOKENS` | `12000` | Decimal whole number from `1` through `4294967295`. |
| `max_output_tokens` | `JCODE_SESSION_LIBRARIAN_MAX_OUTPUT_TOKENS` | `2500` | Decimal whole number from `1` through `4294967295`. |
| `max_requests` | `JCODE_SESSION_LIBRARIAN_MAX_REQUESTS` | `1` | Exactly `1`. The librarian never makes a second provider request for one invocation. |
| `max_cost_usd` | `JCODE_SESSION_LIBRARIAN_MAX_COST_USD` | `0.50` | Positive unsigned decimal USD with at most six fractional digits. |
| `deadline_seconds` | `JCODE_SESSION_LIBRARIAN_DEADLINE_SECONDS` | `120` | Decimal whole number from `1` through `18446744073709551615`. |

Whitespace around numeric values is ignored. Route fields are trimmed and must
remain non-empty. Empty or whitespace-only environment values intentionally
shadow persisted values and fail validation.

`max_cost_usd` is parsed exactly into integer micro-USD, where USD 1 equals
1,000,000 micro-USD. It does not use binary floating point. A leading sign,
multiple decimal points, more than six fractional digits, zero, malformed text,
or a value that overflows `u64` micro-USD is invalid. For example, `"0.50"` is
stored and compared as 500,000 micro-USD.

The following local admission caps are fixed safety limits, not additional
configuration fields:

| Local cap | Limit |
|---|---:|
| Serialized file or tool receipt | 1 KiB |
| One admitted item | 768 tokens |
| One normalized file | 1200 tokens |
| One tool category | 2000 tokens |

The fixed token caps are applied before, and are also bounded by, the effective
`max_input_tokens` global cap.

### Precedence and invocation overrides

Every field resolves independently in this exact order:

1. Explicit arguments on the current `session_librarian` tool invocation.
2. The corresponding `JCODE_SESSION_LIBRARIAN_*` environment variable, already
   applied to the loaded configuration.
3. The corresponding persisted `[session_librarian]` value.
4. The built-in default shown above.

Direct tool callers may override `provider`, `model`, `reasoning_effort`,
`max_input_tokens`, `max_output_tokens`, `max_requests`, `max_cost_usd`, and
`deadline_seconds`. They may also provide one optional `session_id`; omission
selects the current canonical session. The slash skill forwards its optional
session identifier to this same tool path.

The librarian route is independent of the active session route. Resolving or
using a librarian provider, model, or reasoning effort does not change the active
session's provider, model, profile, or reasoning effort. No active-session route
is used as an implicit fallback.

### Preflight and failure behavior

Configuration and provider preflight complete before session content is sent to
a provider. The invocation fails explicitly and publishes no partial artifacts
when any of these conditions applies:

- A route field is empty or the provider, model, or effort is unsupported.
- A budget is empty, malformed, zero, negative, signed, out of range, or, for
  `max_requests`, not exactly `1`.
- Authentication for the independently selected route is unavailable.
- Verified pricing metadata is unavailable.
- The route's calculated worst-case cost exceeds `max_cost_usd`.
- Eligible admitted content is empty or cannot fit within the input and category
  budgets.
- Provider creation, request execution, usage accounting, response parsing, or
  structured summary validation fails.
- Actual input, output, cost, request count, or elapsed time exceeds an effective
  hard budget.
- Fingerprinting, locking, staging, private-permission setup, atomic publication,
  synchronization, or validation of an existing artifact pair fails.
- An explicitly requested persisted `session_id` does not exist.

Failures return an actionable non-secret stage, code, and message, with bounded
usage when it is available. Diagnostics identify only non-secret route and budget
facts. They do not print raw credentials. Failed, timed-out, malformed, or
over-budget attempts do not replace a valid immutable summary pair.

Equivalent successful invocations reuse the existing fingerprinted artifact
pair and skip provider generation. Changed admitted content or summary-affecting
configuration produces a different immutable fingerprint directory under
`~/.jcode/feedback/sessions/<session-id>/`.
