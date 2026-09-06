# Server Architecture

See also:

- [`SERVER_SERVICE_SPLIT_PLAN.md`](./plans/SERVER_SERVICE_SPLIT_PLAN.md)
- [`SWARM_ARCHITECTURE.md`](./SWARM_ARCHITECTURE.md)
- [`MULTI_SESSION_CLIENT_ARCHITECTURE.md`](./MULTI_SESSION_CLIENT_ARCHITECTURE.md)

## Overview

jcode uses a **single-server, multi-client** architecture. One server process
manages all sessions and state; TUI clients connect over a Unix socket and
can reconnect transparently after disconnects or server reloads.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              SERVER (🔥 blazing)                              │
│                                                                             │
│  jcode serve                                                                │
│  ├── Unix socket:  /run/user/$UID/jcode.sock                                │
│  ├── Debug socket: /run/user/$UID/jcode-debug.sock                          │
│  ├── Registry:     ~/.jcode/servers.json                                    │
│  ├── Provider (Claude/OpenAI/OpenRouter)                                    │
│  ├── MCP pool (shared across sessions)                                      │
│  └── Sessions:                                                              │
│        ├── 🦊 fox   (active)  → "🔥 blazing 🦊 fox"                         │
│        ├── 🐻 bear  (active)  → "🔥 blazing 🐻 bear"                        │
│        └── 🦉 owl   (idle)    → "🔥 blazing 🦉 owl"                         │
└─────────────────────────────────────────────────────────────────────────────┘
         │              │              │
         ▼              ▼              ▼
    ┌─────────┐   ┌─────────┐   ┌─────────┐
    │ Client 1│   │ Client 2│   │ Client 3│
    │ 🦊 fox  │   │ 🐻 bear │   │ 🦉 owl  │
    └─────────┘   └─────────┘   └─────────┘
```

## Naming

```
SERVER = Adjective/Verb modifier          SESSIONS = Animal nouns
────────────────────────────              ────────────────────────
🔥 blazing   ❄️ frozen   ⚡ swift          🦊 fox    🐻 bear   🦉 owl
🌀 rising    🍂 falling  🌊 rushing        🌙 moon   ⭐ star   🔥 fire
✨ bright    🌑 dark     💫 spinning       🐺 wolf   🦁 lion   🐋 whale

Combined: "🔥 blazing 🦊 fox" = server + session
```

The server gets a random adjective/verb name on startup (e.g., "blazing").
Each session gets an animal noun (e.g., "fox"). Together they form a natural
phrase displayed in the UI: "🔥 blazing 🦊 fox".

The server name persists across reloads via the registry (`~/.jcode/servers.json`).
When the server execs into a new binary on `/reload`, the new process registers
with a fresh name. Stale entries are cleaned up automatically.

## Lifecycle

```
  START                          CONNECT                     RELOAD
  ─────                          ───────                     ──────
  jcode (first run)              jcode (subsequent)          /reload
       │                              │                          │
       ├─▶ No server? Spawn daemon    ├─▶ Server exists?         ├─▶ Server execs into
       ├─▶ Wait for socket            │   Connect directly       │   new binary (same PID)
       ├─▶ Connect as client          │                          ├─▶ All clients disconnect
       └─▶ Create session             └─▶ Create/resume session  └─▶ Clients auto-reconnect
```

### Server Startup

When you run `jcode`, it checks if a server is already running:

1. **Server exists**: connect directly as a client
2. **No server**: spawn `jcode serve` as a detached daemon (with `setsid`),
   wait for the socket, then connect

The server is fully detached from the spawning client via `setsid()`, so killing
any client never affects the server or other clients.

Long-lived deployments can give the daemon a stable client-visible identity with
`jcode serve --server-name <name>` or the `JCODE_SERVER_NAME` environment
variable. The optional `JCODE_SERVER_DISPLAY_NAME` environment variable is also
accepted for service managers that prefer a display-oriented name. CLI input wins
over environment input. Names are normalized to registry-safe lowercase labels,
so `mount-cloud/fabian` displays as `mount-cloud-fabian`.

### Server Shutdown

The server shuts down when:
- **Idle timeout**: no clients connected for 5 minutes (configurable)
- **Manual**: server process is killed
- **Reload**: server execs into a new binary (same socket path)

### Remote Client Working Directory

By default, a client sends its current working directory to the server when it
subscribes, and the server uses that as the session working directory. Socket
forwarding wrappers for remote daemons can keep the client and server paths
separate with `--remote-working-dir`:

```bash
jcode --socket /tmp/jcode.sock -C /local/checkout --remote-working-dir /remote/checkout
```

`-C` must exist on the client. `--remote-working-dir` must be an absolute path
that exists on the server.

### Client Reconnection

Clients have a built-in reconnect loop. When the connection drops (server
reload, network issue, etc.):

1. Client shows "Connection lost - reconnecting..."
2. Retries with exponential backoff (1s, 2s, 4s... up to 30s)
3. On reconnect, resumes the same session (session state persists on disk)
4. If server was reloaded, client may also re-exec itself if a newer
   client binary is available

### Hot Reload (`/reload`)

1. Client sends `Request::Reload` to server
2. Server sends `Reloading` event to the requesting client
3. Server calls `exec()` into the new binary with `serve` args
4. New server process starts on the same socket
5. All clients auto-reconnect
6. The initiating client also re-execs if its binary is outdated

### Installed-binary upgrades

The source-install workflow uses the same reload protocol after installing a
new binary:

1. `scripts/install_release.sh` updates the immutable version, `current`, and
   launcher channels.
2. It runs `jcode server reload`, which reloads only when the installed binary
   is newer than the running daemon.
3. The server execs the new binary on the same socket and clients reconnect.
4. It runs `jcode server cleanup-stale` through the same cleanup path used by
   rebuilds. Cleanup only considers registry entries whose detached `serve`
   process is provably running from the managed immutable build store. The
   active shared listener, alternate sockets, explicit `JCODE_SOCKET` services,
   and entries with incomplete ownership metadata are preserved.

This is safe to run while other sessions are connected. Their persisted session
state and session IDs are retained, and clients resume after reconnecting. The
reload is not a transaction boundary for work already in progress, however:
an active generation or foreground tool can be interrupted during the handoff.
Headless work has explicit reload recovery where supported, but callers should
still avoid upgrading in the middle of work that cannot be safely retried.

## Socket Paths

```
/run/user/$UID/
├── jcode.sock          # Main communication socket
└── jcode-debug.sock    # Debug/testing socket
```

## Self-Dev Mode

When running `jcode` inside the jcode repository:

1. Auto-detects the repo and enables self-dev mode
2. Connects to the normal shared jcode server
3. Marks that session as canary/self-dev via subscribe metadata
4. Enables selfdev prompt/tooling only for that session
5. `/reload` still hot-reloads the shared server and clients reconnect

## Key Behaviors

| Scenario | Behavior |
|----------|----------|
| First `jcode` run | Spawns server daemon, connects |
| Subsequent `jcode` | Connects to existing server |
| Kill a client | Server + other clients unaffected |
| `/reload` | Server execs new binary, clients reconnect |
| Install a newer binary | Installer conditionally reloads the shared daemon; clients reconnect and sessions persist |
| All clients close | Server idle-timeout after 5 min |
| Resume session | `jcode --resume fox` reconnects to existing session |
