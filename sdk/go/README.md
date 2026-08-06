# Go SDK

The Go SDK is the Go client for the jcode harness API. It speaks protocol v1 over an `io.ReadWriteCloser`, completes the `hello` handshake, correlates request replies, and delivers asynchronous events through bounded subscriptions.

> **API status:** The Go package provides both a transport-level client (`NewClient`) and private-instance helpers (`Launch`, `LaunchInstance`, and `LaunchOptions`). It does not yet provide typed session convenience methods such as `createSession` or `run`; those flows use `protocol.NewRawRequest` and `Client.Request`. The examples below show the supported Go API.

## Install

The module is currently published from this repository:

```bash
go get github.com/1jehuang/jcode-go
```

For a source checkout:

```bash
cd sdk/go
go test ./...
go vet ./...
```

The module requires Go 1.23 or newer. The SDK has no third-party dependencies.

## Choose a connection mode

There are two deployment patterns:

1. **Connect to a shared instance.** Start `jcode api-bridge` once, then dial its owner-only Unix socket and pass the connection to `NewClient`. This is suitable for editor plugins, dashboards, and tools that intentionally operate on the user's live sessions.
2. **Own a private instance.** `jcode.Launch` starts a separately configured bridge/daemon, gives it a separate home and socket, and returns a client that owns shutdown. `LaunchInstance` is available when the caller needs to control dialing. The Go SDK still does not provide typed session helpers, so protocol requests remain explicit.

A shared connection sees the user's sessions and actions are visible in their terminal. A private connection must use a distinct state directory and socket. Never point a private process at the user's live jcode home.

## Shared connect: one-shot CLI-like flow

Start the bridge first:

```bash
jcode api-bridge
```

Then run [`examples/oneshot`](examples/oneshot):

```bash
go run ./examples/oneshot "List the top-level files"
```

The example dials `$JCODE_API_SOCKET` when set, otherwise `$XDG_RUNTIME_DIR/jcode-api.sock`, creates a session, sends one message, prints text deltas, and exits at `turn_done`.

The important lifecycle is:

```go
ctx := context.Background()
conn, err := net.Dial("unix", socketPath)
if err != nil { return err }
client, err := jcode.NewClient(ctx, conn, jcode.Options{
    ClientName: "my-tool/1.0",
})
if err != nil { return err } // NewClient closes conn after handshake failure
sub := client.Subscribe(sessionID)
defer sub.Close()
defer client.Close()
```

`NewClient` starts its reader goroutine and performs a protocol v1 handshake. `Client.Close` is idempotent and wakes pending requests and subscriptions.

## Requests and session lifecycle

The Go SDK deliberately keeps request helpers close to the wire. Build a request with `protocol.NewRawRequest`, send it with `Client.Request`, and decode the correlated `ServerFrame` or inspect its event kind:

```go
create, err := protocol.NewRawRequest("create_session", map[string]any{
    "working_dir": workingDir,
})
if err != nil { return err }
reply, err := client.Request(ctx, create)
if err != nil { return err }

var fields json.RawMessage
if raw, ok := protocol.FieldsJSON(reply.Event); ok {
    fields = raw
} else {
    return errors.New("session creation returned no fields")
}
var sessions struct {
    SessionID string `json:"session_id"`
}
if err := json.Unmarshal(fields, &sessions); err != nil {
    return err
}
```

For production code, keep the session ID in application state, subscribe before sending a message when event loss matters, and close the subscription when the session is no longer needed. Typical request tags are `create_session`, `attach`, `detach`, `send_message`, and `cancel`; their fields are the protocol schema, not Go SDK abstractions. Inspect the protocol definitions when adding a new request.

`Request` waits for the correlated reply or `ctx.Done()`. Cancellation removes the pending request locally, but it does not necessarily cancel work already accepted by the server. To stop a model turn, send the protocol `cancel` request for the session.

## Streaming event consumption

[`examples/streaming`](examples/streaming) demonstrates a long-lived service. It uses one subscription and one consumer goroutine per client. `Subscription.Next` blocks until an event, context cancellation, client shutdown, or a backpressure error:

```go
for {
    event, err := sub.Next(ctx)
    if err != nil {
        if errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
            return nil
        }
        return err
    }
    switch event.Kind {
    case "text_delta":
        var value struct { Text string `json:"text"` }
        if err := event.Decode(&value); err != nil { return err }
        io.WriteString(out, value.Text)
    case "permission_request":
        // Apply your product's policy. Never blindly allow in an untrusted app.
    case "turn_done":
        return nil
    default:
        // Unknown event kinds are intentionally forward-compatible.
        log.Printf("jcode event kind=%s", event.Kind)
    }
}
```

Events are delivered to every subscription. A subscription buffer is bounded (`Options.EventBuffer`, default 128). If a consumer falls behind, that subscription terminates with `ErrSubscriberOverflow` rather than blocking all request traffic or silently dropping events. Consume promptly, increase the buffer deliberately, or fan out after a single reader. Do not call `Next` concurrently on one subscription.

## Cancellation, shutdown, and reconnect/resume

Use contexts for local deadlines and cancellation:

```go
ctx, cancel := context.WithTimeout(parent, 30*time.Second)
defer cancel()
reply, err := client.Request(ctx, request)
```

A canceled context only abandons the local wait. For a server-side turn cancellation, send `cancel` with the session ID and continue consuming events until `turn_done` or shutdown.

The SDK does not automatically reconnect or replay events. Use `Reconnect` when you configure a `transport.Factory`; it retries connection setup according to `ReconnectPolicy`, and with `Resume: true` it sends the remembered session ID after the new handshake. In-flight requests are never retried. Persist session IDs in your application and resubscribe after reconnect:

```go
client, err := jcode.NewClient(ctx, conn, jcode.Options{
    SessionID: sessionID,
    Reconnect: jcode.ReconnectPolicy{
        Factory: func(ctx context.Context) (transport.Transport, error) {
            return transport.UnixSocket(socketPath)(ctx)
        },
        MaxAttempts: 5,
        Backoff: 500 * time.Millisecond,
        Resume: true,
    },
})
// After ErrDisconnected:
if err := client.Reconnect(ctx); err != nil { return err }
sub := client.Subscribe(sessionID)
```

A fresh subscription only receives events from attach onward. Protocol v1 cannot replay events emitted before the new subscription attaches, so use `get_history`/`peek_session` after reconnect if your application needs a consistent transcript. Never blindly repeat a mutating request after a timeout or disconnect: its server-side outcome may be unknown.

## Errors

Branch on sentinel errors where available, and preserve protocol error code/message fields for diagnostics:

- `ErrClosed`: client or transport has closed.
- `ErrSubscriberOverflow`: one subscription exceeded its bounded queue.
- `context.Canceled` and `context.DeadlineExceeded`: local caller cancellation/deadline.
- `protocol.ErrMalformedFrame`, `ErrInvalidFrame`, and `ErrFrameTooLarge`: invalid or unsafe wire data.
- A `protocol.Error` event: the harness rejected a request. Its `Code` is stable; its `Message` is diagnostic.

Treat `timeout`, disconnect, and transport failures as unknown-outcome for mutations. Retry idempotent reads only, and refresh session state after reconnect. Keep a default branch for future protocol error codes and event kinds.

## Private instance pattern

The Go SDK's `Launch` owns a private daemon, temporary home, credential policy, and cleanup. Use it when embedding jcode as an agent engine:

```go
inherit := false
client, err := jcode.Launch(ctx, jcode.LaunchOptions{
    WorkingDir:  workingDir,
    InheritLogins: &inherit,
    ClientOptions: jcode.Options{ClientName: "my-service/1.0"},
})
if err != nil { return err }
defer client.Close()
```

`Launch` defaults to a temporary owner-only home and removes it on shutdown. Set `JcodeHome` to persist sessions. `LaunchInstance` starts the isolated process without dialing it, and its `SocketPath()` can be passed to `net.Dial("unix", ...)` and `NewClient`. Set `InheritLogins` to a bool pointer whose value is false to avoid copying/linking the user's recognized login files.


The private example uses the lower-level process/sockets pattern intentionally, which is useful when a service must supervise the child itself. For most applications, prefer `jcode.Launch` so the SDK owns startup and cleanup. Verify bridge flags for the jcode version you ship before enabling custom supervision. If a private process must use credentials, provision a dedicated service identity instead of inheriting a developer's login files.

## Security guidance

- Unix sockets and runtime directories should be owner-only. Do not change permissions to make a shared socket world-readable.
- `connect` operates on the user's live sessions. Treat prompts, file contents, tool inputs, permission requests, and transcripts as sensitive.
- Do not log raw protocol frames, authorization headers, environment variables, credential paths, prompts, tool arguments, or model output by default. Redact secrets before structured logging.
- Credential inheritance is powerful and dangerous. A process using the user's bridge or login files can spend their quota and access their sessions. Disable inheritance for untrusted code and use a dedicated account for services.
- Validate permission requests in application policy. Do not auto-approve tools merely to make an example convenient.
- Bound frame sizes (`Options.MaxFrameSize`) and event buffers. Apply request deadlines and avoid unbounded transcript or output retention.
- Treat the private home as sensitive state. Use a real, dedicated directory, restrict permissions, and remove temporary homes only after the child process has stopped.

## Platforms and protocol compatibility

The SDK is pure Go and compiles on platforms supported by Go. `transport.DialUnix` is available on Unix-like systems; Windows builds use the platform transport implementation where available. The examples are Unix examples and are not expected to run on Windows unchanged.

The client negotiates **protocol major version 1** (`protocol.APIVersionMajor == 1`). Minor, additive event fields should be decoded permissively. Unknown event kinds are represented as `protocol.UnknownEvent`; preserve or ignore them rather than failing the whole connection. A major-version mismatch requires upgrading the bridge and SDK together. Go SDK releases are independent package releases, so pin a compatible jcode version in deployment and test the pair.

## Troubleshooting

| Symptom | Likely cause | Action |
| --- | --- | --- |
| `dial unix ...: no such file` | Bridge is not running or socket path is wrong | Run `jcode api-bridge`; check `JCODE_API_SOCKET`, `XDG_RUNTIME_DIR`, and permissions. |
| Hello handshake fails | Wrong socket, incompatible protocol, or non-jcode service | Confirm the endpoint is the harness API socket and upgrade both sides. |
| `ErrClosed` during a request | Bridge/daemon exited or transport was closed | Reconnect, then refresh sessions. Do not blindly repeat mutations. |
| `ErrSubscriberOverflow` | Consumer is slower than event production | Consume faster, increase `EventBuffer`, or fan out from one reader. |
| No text appears | Session is not attached or events are consumed after sending | Subscribe/attach before `send_message`; inspect all event kinds and `turn_done`. |
| Permission request blocks | Application has not answered the request | Apply an explicit allow/deny policy and send the corresponding response request. |
| Private process cannot start | Invalid binary, home, socket, or credentials | Capture child stderr without logging secrets; verify paths and use a dedicated home. |

## Migration from exec-based integrations

An exec integration commonly starts `jcode`, writes prompts to stdin, parses terminal text, and treats process exit as completion. Migrate in stages:

| Exec integration | Go SDK replacement |
| --- | --- |
| `exec.Command` plus shell/terminal parsing | Start `jcode api-bridge` or a private process, dial its API socket, call `NewClient`. |
| Prompt text on stdin | `protocol.NewRawRequest("send_message", fields)` plus `Client.Request`. |
| Scraping stdout for tokens | `Subscribe` and decode `text_delta`, `tool_*`, `permission_request`, and `turn_done`. |
| Killing the child for cancellation | Send protocol `cancel`, then close the client/process during shutdown. |
| Assuming process exit means success | Inspect correlated `protocol.Error`, `turn_done`, and transcript/history. |
| Re-running the whole command after a timeout | Reconnect, refresh history, and retry only operations known to be safe. |

Keep the old exec path as a fallback while validating parity. Do not run both paths against the same live session unless you intentionally want concurrent actors. Once the SDK path is stable, remove shell quoting and terminal scraping, add explicit deadlines and permission policy, and redact protocol diagnostics before logging.

## Examples and compile checks

The three examples are ordinary Go packages and are checked by:

```bash
cd sdk/go
gofmt -d .
go test ./...
go vet ./...
go build ./examples/oneshot ./examples/streaming ./examples/private
```

They require a live jcode bridge only at runtime. `go build` and `go test` do not contact a daemon.
