# DAP debugger operations

Jcode's debugger capability is a language-neutral DAP client. It keeps adapter
processes and socket connections under the long-lived server instead of taking
over the user's terminal. Existing `DebugJob` operations are separate and are
unchanged when the debugger tool is unavailable or disabled.

## Enable an adapter

Adapters are declarative. Jcode never installs a debugger and never launches a
debuggee implicitly.

The built-in catalog currently describes `gdb`, `lldb-dap`, `debugpy`, `dlv`,
and `js-debug-adapter`. Availability is checked only when a debugger action is
requested.

User adapters live in `$JCODE_HOME/dap.toml` (normally `~/.jcode/dap.toml`).
Project adapters live in `dap.toml` files from the project ancestor chain down
to the session working directory. Higher-precedence files override lower ones.
Adapter objects and their `launch_defaults` and `attach_defaults` maps are
merged by key. Adapter names are stable and selection ties are resolved by
root-marker specificity, then name order.

```toml
[adapters.debugpy]
command = "python3"
args = ["-m", "debugpy.adapter"]
languages = ["python"]
file_types = [".py"]
root_markers = ["pyproject.toml", "requirements.txt"]
transport = "stdio"

[adapters.debugpy.launch_defaults]
request = "launch"
justMyCode = false
stopOnEntry = true

[adapters.debugpy.attach_defaults]
request = "attach"
justMyCode = false

[permissions]
# These values layer with the same user/project precedence as adapters.
allow_process_control = true
allow_evaluate = true
allow_memory_write = false
request_timeout_ms = 30000
startup_timeout_ms = 10000
max_output_bytes = 131072

# A project can keep an adapter definition but disable it safely.
# [adapters.debugpy]
# enabled = false
```

Supported transports are `stdio`, `tcp`, and Unix `socket` on Unix systems.
TCP launch arguments can use `${port}`. Unix-socket launch arguments can use
`${socket}`. Adapter arguments are passed directly to the executable. They are
not evaluated by a shell.

## Tool operations

The agent-facing tool is named `debugger` and uses one bounded action schema:

- `sessions` or `status`
- `launch` with an explicit `program`
- `attach` with an explicit `adapter` and `pid` or `host`/`port`
- `set_breakpoint`, `remove_breakpoint`
- `continue`, `pause`, `step_over`, `step_in`, `step_out`
- `threads`, `stack_trace`, `scopes`, `variables`, `evaluate`
- `output`, `modules`, `read_memory`, `write_memory`
- `stop`, `disconnect`

`ToolContext.working_dir` is the default debugger working directory. Use an
explicit `session_id` when more than one debugger session is active. Results
include a structured snapshot and retain only a bounded output tail.

## Safety tiers

The DAP crate classifies operations before dispatch:

| Tier | Operations | Default |
| --- | --- | --- |
| Read-only | sessions, status, threads, stack, scopes, variables, output, modules, read memory | Allowed |
| Process control | launch, attach, breakpoints, continue, pause, stepping, stop, disconnect | Allowed only through an explicit debugger action |
| Expression evaluation | evaluate | Allowed through an explicit debugger action |
| Memory write | write memory | Denied by default |

The ordinary Jcode tool allow/disable policy and named session profiles still
control whether the `debugger` tool is present. The DAP policy separately
rejects memory writes and can disable process control or evaluation. Adapter
capability negotiation rejects optional operations locally when the adapter did
not advertise the relevant DAP capability.

## Launch versus attach

Launch starts a configured adapter and sends a DAP `launch` request containing
the explicit target. Attach connects to a configured adapter or starts an
adapter for an explicit process id according to its attach defaults. An attach
request without a pid or endpoint is rejected. Jcode does not infer a process
from a filename and does not run arbitrary launch commands from tool input.

## Bounds and cleanup

- Requests have a bounded timeout. Transport writes have their own hard bound.
- One async reader drains each active transport and correlates responses by DAP
  sequence number. There is no response polling loop.
- Malformed JSON frames are counted and skipped so later valid frames can still
  be processed. Invalid headers and oversized frames return a bounded protocol
  error and terminate that transport rather than retrying the invalid buffer.
- Adapter processes are started directly. On Unix they use a non-interactive
  process group and are killed during disconnect or startup failure.
- Retained adapter output is capped at 128 KiB per session. Metadata marks a
  truncated tail.
- Terminated or idle sessions can be reclaimed with the manager's explicit
  `reap_idle` operation before the server exits.

## Troubleshooting

### No adapter matches

Check the source file extension, root marker, and effective project `dap.toml`.
Use an explicit `adapter` action argument to bypass automatic selection. If the
command is missing, install it outside Jcode and retry. A missing adapter does
not change ordinary `bash`, process, or `DebugJob` behavior.

### Adapter exits or times out

Inspect the adapter's own installation and launch syntax, then run the adapter
manually outside Jcode with the same arguments. Confirm that the adapter speaks
DAP over the configured transport. Disconnect the failed session before trying
again. Jcode returns the adapter-exit or timeout context and releases pending
requests.

### Unsupported operation

The adapter may not advertise the required capability. Use `sessions` to
inspect the negotiated snapshot, then choose a supported inspection operation.
Do not work around a capability rejection by sending a custom shell command.

### Safe local validation

Use the fake adapter tests, deterministic Python/native protocol recordings, and
the standalone crate first:

```bash
scripts/dev_cargo.sh test -p jcode-dap
scripts/dev_cargo.sh test -p jcode-app-core debugger_tests
python3 scripts/dap_benchmark.py --rounds 20
```

These checks are offline and do not require an installed debugger or a shared
Jcode daemon. For a built-binary smoke check, use a private socket and the
selfdev binary described in `AGENTS.md`.

## Research provenance

The module split and bounded transport choices were informed by the MIT-licensed
Oh My Pi DAP implementation (`can1357/oh-my-pi`, inspected at commit
`ffd53ff`). Jcode contains an independent Rust implementation rather than a
source translation. The upstream design was especially useful for separating
adapter config, transport framing, session state, capability gates, and the
agent-facing debugger tool.
