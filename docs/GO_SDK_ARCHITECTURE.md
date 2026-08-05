# Go SDK architecture and compatibility decision

**Status:** Decision for `jcode-zqc.1` (downstream implementation contract)
**Date:** 2026-08-05
**Owner:** Jcode maintainers (`github.com/1jehuang`)

## Decision summary

The official Go SDK will be a separate module at **`github.com/1jehuang/jcode-go`**. It is an external protocol client, not a Rust FFI binding and not a CLI-output parser. The existing harness API is the wire-contract owner. The Rust SDK and TypeScript SDK are behavioral references and compatibility oracles, while `crates/jcode-harness-api` and its schema tests define the serialized v1 boundary.

The first release supports connecting to an existing local Jcode harness and launching an isolated private Jcode instance through the installed Jcode executable. The public API is context-aware, typed for the curated v1 surface, and retains raw frame/request escape hatches for additive protocol evolution.

## Ownership and module layout

- Module: `github.com/1jehuang/jcode-go`.
- Repository ownership: the Jcode project maintainers under the `1jehuang` GitHub organization. Publishing is a separate release-review decision and is not implied by this document.
- The Go module owns framing, local transport, request correlation, typed protocol values, event dispatch, errors, cancellation, and optional private-instance supervision.
- The Rust runtime and `crates/jcode-harness-api` own server behavior and wire serialization. Go must not duplicate or modify server protocol definitions.
- The ACP/CLI boundary remains separate. Go v1 speaks the harness API, not ACP JSON-RPC and not terminal text. ACP interoperability can be added later as a separate transport/product decision.
- Downstream beads must reference this document rather than inventing module names, public vocabulary, or version rules.

Recommended implementation split:

```text
jcode-go/
  protocol/   // wire structs, constants, unknown preservation
  transport/  // Unix local socket and test transport
  client/     // Client, Session, requests, event streams
  launch/     // isolated executable supervision
```

The exact package split may be simplified, but the public names and ownership above are fixed.

## Platform and transport matrix

| Platform | v1 status | Transport |
| --- | --- | --- |
| Linux | supported | Unix domain stream socket |
| macOS | supported | Unix domain stream socket |
| Windows | not supported in v1 | No named-pipe or TCP fallback is authorized by this decision |

The SDK uses the same socket resolution as the harness API: `JCODE_API_SOCKET` if set, otherwise `$JCODE_RUNTIME_DIR`, `$XDG_RUNTIME_DIR`, macOS `$TMPDIR`, or the platform temporary directory fallback, with `jcode-api.sock` as the filename. A caller may always pass an explicit socket path. The internal daemon socket (`jcode.sock`) is not a public Go endpoint. The API socket and daemon socket are siblings, but Go connects only to the API socket.

The transport is NDJSON: one complete JSON object per line, with `v` on every frame. The implementation must expose an injectable byte-stream transport for protocol tests, but v1 production transport is local Unix socket only. No network listener, TLS, TCP, WebSocket, or ACP transport is part of v1.

## Lifecycle and credential policy

`Connect(ctx, options)` attaches to a running harness and shares its sessions. `Launch(ctx, options)` starts a private instance, waits for its API socket, connects, and owns cleanup. Closing a launched client stops the child process and removes only the SDK-created temporary instance state. Closing a connected client only closes the socket.

Launch is included in v1 because the existing Rust and TypeScript SDKs already define and test this boundary, and the parent epic requires an isolated-instance workflow. It is deliberately a thin supervisor, not a second runtime manager. The executable path is configurable; the default resolves the installed `jcode` executable. Launch must reject an unsafe or user-home-aliasing instance directory and must surface startup timeout/failure as typed errors.

- `InheritCredentials` defaults to true for launch compatibility with the existing SDKs, so a private instance can use the user's configured provider login.
- `InheritCredentials=false` starts with an empty private credential store.
- The SDK never accepts, prints, serializes, or copies secret values through protocol logs. API-key operations are explicit and are not a substitute for OAuth login.
- Credential inheritance is a local filesystem operation performed by the launcher, not a wire-protocol capability. OAuth tokens and provider stores must follow the runtime's existing ownership and permission rules.
- The caller supplies `WorkingDir`; absent that option, launch uses the current process working directory. Session working directories remain explicit protocol fields.

## Public Go API contract

Names below are normative. Idiomatic Go method receivers and package names may vary only if the exported behavior remains equivalent.

```go
client, err := jcode.Connect(ctx, jcode.ConnectOptions{})
private, err := jcode.Launch(ctx, jcode.LaunchOptions{})
s, err := client.CreateSession(ctx, jcode.CreateSessionOptions{WorkingDir: dir})
events := client.Events(ctx, jcode.EventFilter{SessionID: s.ID})
err = s.Send(ctx, "hello", jcode.SendOptions{})
err = client.Close()
```

- `Client`: clone-safe handle for concurrent request/reply operations and event subscriptions. `Close` is idempotent.
- `Session`: a lightweight ID-bound view, obtained from `CreateSession` or `AttachSession`. It exposes session operations without hiding the parent client.
- Requests: typed option structs such as `CreateSessionOptions`, `SendOptions`, `PermissionResponse`, and `RewindOptions`; no `map[string]any` is required for curated operations.
- Events: typed `Event` interface with concrete values matching the wire tags, including `TextDelta`, `ReasoningDelta`, `ToolStart`, `ToolInputDelta`, `ToolExec`, `ToolDone`, `TokenUsage`, `TurnDone`, `BackgroundProgress`, `MessageAccepted`, `PermissionRequest`, status/model/runtime/file events, and lifecycle events.
- Errors: one exported `*Error` with stable `Code`, operation context, and `Unwrap` support where useful. Codes include transport/connection failures (`connect_failed`, `handshake_failed`, `timeout`, `disconnected`, `unexpected_reply`, `startup_failed`, `startup_timeout`, `invalid_option`, `unsupported_transport`) and server error codes (`unsupported_version`, `unknown_request`, `unknown_session`, `invalid_request`, `internal`). Callers must branch on codes, never message text.
- Cancellation: every potentially blocking method accepts `context.Context`. Context cancellation and deadlines terminate waiting requests and event receives without changing the wire protocol. `Cancel(ctx, sessionID)` is the server-side generation cancellation request.
- Raw escape hatches: `Client.Request(ctx, RawRequest) (RawFrame, error)`, `Client.Notify(ctx, RawRequest) error`, and an `UnknownEvent` value preserving the event tag and fields. Raw APIs are for forward compatibility and diagnostics, not for bypassing safety checks in curated methods.

The client correlates replies by `reply_to` and request `id`; unsolicited events are delivered to subscriptions. Requests may be concurrent. Event subscriptions are bounded or explicitly backpressured and must never silently drop events. A slow consumer must be able to cancel its subscription and a closed connection must terminate all pending operations.

## Version negotiation and compatibility

- v1 means `API_VERSION_MAJOR=1`; the current minor is `API_VERSION_MINOR=0`.
- The first frame is `hello` with `min_version=1`, `max_version=1`, and a client identifier. The server must answer `hello_ok` or a typed error. A Go client must reject a server that does not negotiate major 1.
- Major-version changes are breaking and require a new negotiated major and a new compatibility review. The Go v1 client never silently downgrades across majors.
- Minor changes are additive. Servers may add fields and event kinds without breaking a v1 client. Go decoders ignore unknown object fields.
- Unknown event kinds are not fatal. They are delivered as `UnknownEvent` with raw fields and remain visible through the event stream and raw API. Unknown request replies are typed as server errors.
- Capabilities from `hello_ok` are an opaque string set. The client exposes `Capabilities()` and curated helpers such as `HasCapability`; absence means the feature is not assumed. A capability is required before using behavior that is optional or not guaranteed by the v1 baseline.
- `hello_ok.version` must be major 1. The server identity and capabilities are metadata, not authorization.
- Compatibility tests must compare Go's request/event tags and serialized field names against the Rust enum source and TypeScript protocol lists. Hand-maintained Go types are acceptable in v1, but every protocol addition must update the Rust/TypeScript parity checks and the Go compatibility fixture in the same change. Code generation is optional, not a new source of truth.

## Curated v1 wire surface

The following exact request tags are in scope, taken from `crates/jcode-harness-api/src/requests.rs` and `sdk/typescript/src/protocol.ts`:

`hello`, `list_sessions`, `archive_session`, `restore_session`, `set_retention_policy`, `create_session`, `attach_session`, `detach_session`, `send_message`, `cancel`, `soft_interrupt`, `get_history`, `peek_session`, `clear`, `rewind`, `permission_response`, `list_models`, `get_runtime_info`, `set_api_key`, `clear_api_key`, `read_file`, `find_files`, `search_text`, `file_status`, `set_model`, `set_reasoning_effort`, `compact`, `rename_session`, `rewind_undo`, `cancel_soft_interrupts`, `ping`.

The exact event tags are:

`hello_ok`, `ok`, `error`, `sessions`, `attached`, `history`, `pong`, `text_delta`, `reasoning_delta`, `reasoning_done`, `tool_start`, `tool_input_delta`, `tool_exec`, `tool_done`, `token_usage`, `turn_done`, `background_progress`, `message_accepted`, `permission_request`, `session_status`, `model_info`, `models`, `runtime_info`, `credential_updated`, `file_content`, `files`, `text_matches`, `file_status`, `compacted`, `session_renamed`.

The baseline payload shapes are the existing TypeScript discriminated unions and Rust `ApiRequest`/`ApiEvent` definitions. Important field rules include snake_case wire names, optional fields remaining optional, image attachments as `[media_type, base64_data]`, `permission_response.decision` as `allow | allow_always | deny`, and `reply_to` being present only on direct replies.

The `Unknown` Rust request/event catch-all is a server-side deserialization safeguard. The Go client must preserve the event-side unknown behavior even where a language union cannot express the Rust catch-all directly.

## ACP and CLI boundary

ACP is JSON-RPC over stdin/stdout with methods such as `initialize`, `session/new`, `session/load`, `session/resume`, `session/prompt`, `session/cancel`, and `session/close`. It maps daemon events into ACP `session/update` notifications and optional `_jcode/server_event` extensions. It is not the Go SDK contract. Go v1 does not parse ACP or invoke the CLI for normal operations. Launching the `jcode` executable is allowed only as a supervised private runtime process; all client operations still use the harness API socket.

## Non-goals and follow-ups

- No Rust FFI, embedded runtime, or reimplementation of Jcode.
- No CLI text parsing, ACP transport, Windows named pipes, TCP exposure, or remote-host support in v1.
- No promise of internal Rust-only session fields or TUI test-harness behavior that is absent from the harness API.
- No speculative wire changes. If a required Go use case cannot be expressed by the listed v1 surface, create a focused protocol follow-up instead of expanding this contract silently.
- No publish commitment. Release review must decide semantic versioning, support windows, and registry publication.

## Validation obligations for downstream beads

1. Unit-test framing, handshake, correlation, unknown fields/events, context cancellation, and error-code mapping with an in-memory transport.
2. Run Rust harness schema snapshots and TypeScript protocol tests; add Go parity fixtures for every request/event tag and field.
3. Test concurrent requests, event backpressure, close behavior, reconnect/resume, and launch cleanup.
4. Run `go test ./...` and `go test -race ./...` on Linux; run the supported macOS build/test job before release.
5. Validate a built `jcode` executable in an isolated launch smoke test without using the shared daemon or user sessions.
