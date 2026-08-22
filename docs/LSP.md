# Language Server Protocol (LSP)

Jcode's LSP support is **off by default**. Installed language servers can use
substantial CPU and memory, especially `rust-analyzer` and TypeScript servers.
Jcode never starts a built-in adapter merely because its executable is present.

Enable a server explicitly in either the user file
`$HOME/.config/jcode/lsp.toml` or a project file `.jcode/lsp.toml`:

```toml
[servers.rust-analyzer]
enabled = true
command = "rust-analyzer"
file_types = [".rs"]
language_id = "rust"
root_markers = ["Cargo.toml"]
startup_timeout_ms = 10000
request_timeout_ms = 2000
idle_timeout_ms = 600000
```

Project settings override user settings. Settings objects merge recursively.
An explicit `enabled = false` disables an inherited server. A server is eligible
only when all of these are true:

1. Its configuration is explicitly enabled.
2. The file type matches.
3. A root marker is found, unless no markers were configured.
4. The configured executable exists and is executable.

The built-in catalog is declarative and currently covers Rust, TypeScript and
JavaScript, Python, Go, C/C++, Java, C#, Kotlin, PHP, Ruby, and YAML. Built-ins
are templates only. Jcode does not install, update, or auto-enable adapters.

## Safe actions

The `lsp` tool exposes bounded `status`, `sessions`, `start`, `feedback`,
`diagnostics`, `capabilities`, `hover`, `definition`, `references`, `symbols`,
`formatting`, `rename`, `code_actions`, and `disconnect` actions. Optional
operations are rejected locally when the server does not advertise the required
capability. Formatting, rename, and code actions remain explicit tool actions.

When an enabled server is active, a successful `edit` synchronizes the document
using monotonically increasing versions and may append compact diagnostics. The
edit remains successful when LSP is disabled, missing, slow, malformed, or
otherwise unavailable. No server is started by an edit unless a matching server
was explicitly enabled.

Diagnostics are normalized to bounded UTF-8 messages, retain severity, source,
code, and ranges, and reject stale versions. Slow feedback is marked deferred.
Server processes are launched directly, never through a shell, and are owned by
the server-side session manager. Idle sessions can be reaped and disconnect
kills the owned adapter process.

## Troubleshooting

- **No matching server:** confirm `enabled = true`, the file suffix, and root marker.
- **Missing binary:** install the adapter separately or set an explicit absolute
  `command`; Jcode will report it as unavailable rather than launching a fallback.
- **Slow startup or requests:** increase the configured timeout only for the
  explicitly enabled server. Slow feedback is bounded and ordinary edits remain
  usable.
- **Malformed protocol:** the session reports a protocol error and cleans up
  pending requests. Retry after fixing the adapter.
- **High resource use:** remove `enabled = true` or set it to false. The default
  configuration starts no server.

## Local validation

The standalone crate can be tested without a real language server:

```bash
cargo test -p jcode-lsp --all-targets
cargo test -p jcode-app-core lsp
```

The tests use bounded in-process transport fixtures. They do not contact a
provider, install an adapter, or start a production daemon.
