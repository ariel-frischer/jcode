<div align="center">

# jcode

[![Latest Release](https://badgen.net/github/release/1jehuang/jcode?icon=github)](https://github.com/1jehuang/jcode/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)
[![Platforms](https://img.shields.io/badge/platforms-Linux%20%7C%20macOS%20%7C%20Windows-blue?style=flat-square)](https://github.com/1jehuang/jcode/releases)
[![Last Commit](https://badgen.net/github/last-commit/1jehuang/jcode/master?icon=github)](https://github.com/1jehuang/jcode/commits/master)
[![GitHub Stars](https://badgen.net/github/stars/1jehuang/jcode?icon=github)](https://github.com/1jehuang/jcode/stargazers)
[![Discord](https://img.shields.io/badge/Discord-Join%20Community-5865F2?style=flat-square&logo=discord&logoColor=white)](https://discord.gg/nBe9vGyK9a)

The most RAM efficient harness <br>
The most intelligent harness

<a href="https://trendshift.io/repositories/25042?utm_source=repository-badge&amp;utm_medium=badge&amp;utm_campaign=badge-repository-25042" target="_blank" rel="noopener noreferrer"><img src="https://trendshift.io/api/badge/repositories/25042" alt="1jehuang/jcode | Trendshift" width="250" height="55"></a>

<a href="https://github.com/1jehuang/jcode/releases/download/readme-assets/jcode-yc-launch.mp4">
  <img src="https://github.com/1jehuang/jcode/releases/download/readme-assets/jcode-yc-launch.webp" alt="jcode YC launch video" width="800">
</a>

<br>

[Website](https://jcode.sh) · [Docs](https://jcode.sh/docs) · [SDK](https://jcode.sh/sdk) · [Benchmarks](https://jcode.sh/bench) · [Features](#features) · [Install](#installation) · [Quick Start](#quick-start) · [Further Reading](#further-reading) · [Contributing](CONTRIBUTING.md)

</div>

---

## Ariel's personal fork

This repository is my personal Jcode fork, based on the upstream project at
[1jehuang/jcode](https://github.com/1jehuang/jcode). I will try to keep it
reasonably synchronized with upstream while preserving features and workflow
improvements that are useful to me. It is therefore not intended to be a
drop-in replacement for, or a fully equivalent distribution of, upstream Jcode.

### Custom features and fixes

This section is intentionally limited to additions, behavior changes, and bug
fixes that I have made or maintained in this fork. It does **not** restate the
many features that are already part of regular upstream Jcode, such as the
general swarm, browser, diagram, and provider surfaces. It also avoids
restating the upstream debugger surface; the fork-specific DAP controls below
are included because they are part of this custom delta.

The clearest examples of my custom delta include:

- **Interactive TUI controls and file mentions:** a `Ctrl+P` command palette
  and model-picker flow, including remote-session support, plus related picker
  behavior such as keeping favorite models at the top, staging useful models
  before the full catalog arrives, showing clear model-switch notices, and
  keeping picker loading off the input path. The composer also has an `@`-based
  fuzzy picker rooted at the session workspace, configurable ignore paths,
  responsive discovery, and expansion of selected files into model context.
- **Persistent adaptive session bar:** active sessions keep identity, provider
  credit or usage, model, authentication, reasoning, and connection context in
  dedicated top chrome instead of relying on scroll position. The bar uses one
  or two rows normally, expands to at most three when useful, and suppresses
  itself before crowding chat on constrained terminals. Set
  `[display] top_bar = false` to restore the previous bar-free layout.
- **Clickable inline document previews:** click a file shown in an inline
  tool/diff preview or an `@filepath` mention to open or collapse it in the
  TUI. The fork fixes relative- and home-directory path resolution, detects
  mentions in inline markup, keeps ordinary file content readable across light
  and dark themes, and opens HTML mentions externally by default with a
  configurable override.
- **Named session profiles:** reusable named policies for a session that bundle
  the provider, model, reasoning effort, tools, skills, and additive
  instructions. Profiles can be selected from the CLI or TUI, inherited by
  child agents and swarm workers, inspected without exposing credentials, and
  restored from a credential-free snapshot that detects profile drift.
- **Privacy-safe lifecycle observability:** typed per-session events for
  compaction, handoff, retry, strategy-switch, and block decisions. Events are
  persisted in bounded JSONL sidecars with rotation, retention, malformed-record
  recovery, and cleanup isolation, and can be queried through the built-in
  lifecycle protocol. Configure the local sinks under
  `[lifecycle_observability]`; see the [lifecycle observability guide](docs/LIFECYCLE_OBSERVABILITY.md).
- **Fresh-session handoff and continuation:** a special workflow for moving
  work into a new session while carrying forward the relevant context. It can
  be configured for automatic or agent-directed handoff, preserves summaries
  and prompt state, supports generated summaries for no-argument `/handoff`,
  carries todos by default with an opt-out, and uses startup barriers so input
  is not sent to the wrong parent or child session. This fork also includes an
  optional **handoff-poke**:
  when a todo group completes near a configured context threshold, Jcode gives
  the agent a concise advisory reminder with the current context percentage and
  next pending item. It never forces a transition. When enabled, the session
  prompt explains why handoff helps and how to leave a bounded continuation
  prompt with the outcome, decisions, risks, next steps, relevant files, and
  Bead ID, and explicitly points the agent to `session_transition`. Configure
  it under `[handoff]` in `~/.jcode/config.toml`. For example, set
  `poke_enabled = true`; the default remains disabled.

  The rationale is context quality first: completed milestones become irrelevant
  context that can dilute attention and make later work less reliable. A fresh
  session keeps the next milestone focused while preserving durable todos and a
  deliberate continuation prompt. It can also reduce repeated prompt processing
  and token usage over long runs, but the primary goal is better reasoning quality,
  not an automatic cost optimization. The poke is intentionally advisory so the
  agent can defer handoff when continuity is more valuable.
- **Headless and session workflow fixes:** safer bounded `jcode run`
  execution and one-shot result delivery, restored prompt state across resume,
  explicit socket handling during startup, and improvements to child-session
  handoff behavior.
- **Bidirectional queued-message editing:** `Alt+Q` takes back the newest queued
  user message and moves toward older held drafts, while `Alt+Shift+Q` moves
  toward newer drafts. Text and ordered images are saved before each move, and
  `Enter` updates or deletes exactly the selected queued message. See
  [Queued-message editor](#queued-message-editor) for ownership, concurrency,
  recovery, and compatibility details.
- **Provider and session fixes:** OpenRouter reported-cost tracking and its
  downstream consumers, safer legacy provider-cost handling, redacted provider
  failure codes surfaced through the harness and Go SDK, fixes that prevent
  stale provider rerouting on restore, safer standard-tier OpenAI request
  defaults, and bounded, cancellable post-login validation for `jcode login`.
- **Resilient free websearch:** a configurable DuckDuckGo-to-Bing fallback
  chain with bounded retries and timeouts, temporary suppression of unhealthy
  engines, privacy-safe diagnostics, and concise fallback status in the TUI.
  Trusted SearXNG endpoints can be added explicitly, while the master switch
  and per-engine controls preserve the legacy path when disabled. See the
  [websearch guide](docs/WEBSEARCH.md) for configuration and behavior.
- **Configurable debugging:** DAP debugger operations, adapter setup and
  launch transport, policy controls, and CLI/configuration switches for
  disabling the debugger when it is not wanted.
- **Experimental passive workflow progress:** an opt-in, server-owned main-TUI box for owned subagents and explicitly registered artifact workflows, with separate activity/checkpoint ages and safe health evidence. See the [workflow guide](docs/public/workflow-observability.md).
- **Experimental LSP feedback:** language-neutral diagnostics, navigation, and
  edit synchronization based on the Oh My Pi LSP design. Language servers are
  explicitly opt-in and remain disabled by default to avoid TypeScript/Rust
  resource overhead. See the [LSP documentation](https://github.com/ariel-frischer/jcode/blob/dev/docs/LSP.md)
  for configuration and safety details.
- **Agent-oriented development tools:** the `agentgrep` context-saving search
  workflow, session-feedback workflows, memory-sidecar work, typed Go SDK event
  compatibility, side-pane image events, and the companion Go SDK maintained
  for this fork.
- **Operational and process fixes:** foreground process-group ownership and
  cancellation cleanup, prevention of orphaned `jcode serve` processes during
  reinstall, restoration of terminal modes on signal exit, safe draining of
  terminal color-query replies, and startup/reload behavior intended for my
  self-development workflow, including restoration of idle headless swarm
  workers after reload. Linux compositor launch hotkeys are also explicitly
  opt-in rather than enabled during setup by default.

This is a representative list, not a claim that every upstream commit is
absent here or that every item is enabled in every build. The commit history is
the source of truth for the current fork delta. I will continue synchronizing
with upstream while keeping these personal additions and fixes where they do
not conflict with intentional upstream changes.

### Queued-message editor

When the composer is empty, press `Alt+Q` to take back the newest eligible
queued user message. While the editor is active, `Alt+Q` first saves the current
text and ordered images, then moves to the next older held message.
`Alt+Shift+Q` performs the same save-before-move step toward the next newer held
message. Reaching either boundary leaves the selection, text, and images
unchanged and displays an oldest/newest boundary notice.

Press `Enter` to finish editing. A draft containing text or one or more images
updates only the selected queued message. A draft is deleted only when both its
text and image list are empty, so an images-only draft is committed rather than
deleted. Editing then exits, every other held message is restored once in its
original relative order, and ordinary prompt history is not changed merely by
navigating between drafts.

Local queued messages and authoritative server soft interrupts use the same
composer controls. Server-backed navigation is available only when the peer
advertises `queued_message_navigation_v1`; otherwise Jcode retains the legacy
one-shot `Alt+Q` behavior. An authoritative editing snapshot includes only
queued, non-injected, `User`-source messages owned by the requesting client.
Messages belonging to another client, unowned legacy records, system or
background interrupts, non-`User` sources, and already injected work are never
reserved, disclosed, reordered, or changed. Messages arriving after editing
starts stay outside the stable snapshot and keep their arrival order.

Held server messages cannot dispatch while editing is active. When editing
finishes or is released, Jcode restores held messages between their nearest
surviving predecessor and successor anchors. If one anchor disappeared, the
messages remain adjacent to the surviving anchor on their original side. If
both disappeared, pre-snapshot survivors remain before the held messages and
post-snapshot arrivals remain after them. An inexact but safe restoration shows
a **stale placement** notice. An unsafe commit shows a **conflict** notice and
preserves the complete text and ordered images for retry or recovery. Operation
replays and transient owner reconnects reuse stable session identities so they
do not reserve, update, delete, lose, or duplicate a message twice; expiry or
explicit release restores the immutable originals once.

The queued-message editor is intentionally keyboard-only. It does not add an
FZF picker or slash-command picker.

### Go SDK

This fork uses and maintains the companion Go SDK at
[ariel-frischer/jcode-go](https://github.com/ariel-frischer/jcode-go). Its
`dev` branch is the sole SDK development source, and version tags are the
release boundaries consumed by Go modules. Jcode retains the authoritative
Rust protocol-v1 wire contract in `crates/jcode-harness-api` and validates an
explicit SDK checkout without copying it:

```bash
scripts/validate_jcode_go_compat.sh --jcode-go-dir /absolute/path/to/jcode-go
```

The SDK provides a typed client for creating, attaching to, and streaming
events from Jcode sessions.

---

<div align="center">

## Installation

</div>

```bash
# macOS & Linux
curl -fsSL https://jcode.sh/install | bash
```

```powershell
# Windows 11 (PowerShell 5.1+)
irm https://jcode.sh/install.ps1 | iex
```

Need Homebrew, source builds, provider setup, or want an agent to set it up for you?
[Jump to detailed installation](#detailed-installation).

---


<div align="center">

## Performance & Resource Efficiency

</div>

jcode is built to be as performant and resource efficient as possible. Every metric is optimized to the bone, which is important for scaling multi-session workflows. Here we sample a few metrics to show the difference: RAM usage and boot up.

### RAM comparison

<div align="center">

<table>
  <tr>
    <td valign="top" align="center" width="50%">
      <strong>1 active session</strong>
      <table>
        <thead>
          <tr>
            <th>Tool</th>
            <th>PSS</th>
            <th>Comparison</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><strong>jcode (local embedding off)</strong></td>
            <td align="right"><strong>27.8 MB</strong></td>
            <td align="right">baseline</td>
          </tr>
          <tr>
            <td><strong>jcode</strong></td>
            <td align="right"><strong>167.1 MB</strong></td>
            <td align="right"><strong>6.0× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>pi</strong></td>
            <td align="right"><strong>144.4 MB</strong></td>
            <td align="right"><strong>5.2× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>Codex CLI</strong></td>
            <td align="right"><strong>140.0 MB</strong></td>
            <td align="right"><strong>5.0× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>OpenCode</strong></td>
            <td align="right"><strong>371.5 MB</strong></td>
            <td align="right"><strong>13.4× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>GitHub Copilot CLI</strong></td>
            <td align="right"><strong>333.3 MB</strong></td>
            <td align="right"><strong>12.0× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>Cursor Agent</strong></td>
            <td align="right"><strong>214.9 MB</strong></td>
            <td align="right"><strong>7.7× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>Claude Code</strong></td>
            <td align="right"><strong>386.6 MB</strong></td>
            <td align="right"><strong>13.9× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>Antigravity CLI</strong></td>
            <td align="right"><strong>243.7 MB</strong></td>
            <td align="right"><strong>8.8× more RAM</strong></td>
          </tr>
        </tbody>
      </table>
    </td>
    <td width="24"></td>
    <td valign="top" align="center" width="50%">
      <strong>10 active sessions</strong>
      <table>
        <thead>
          <tr>
            <th>Tool</th>
            <th>PSS</th>
            <th>Comparison</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><strong>jcode (local embedding off)</strong></td>
            <td align="right"><strong>117.0 MB</strong></td>
            <td align="right">baseline</td>
          </tr>
          <tr>
            <td><strong>jcode</strong></td>
            <td align="right"><strong>260.8 MB</strong></td>
            <td align="right"><strong>2.2× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>pi</strong></td>
            <td align="right"><strong>833.0 MB</strong></td>
            <td align="right"><strong>7.1× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>Codex CLI</strong></td>
            <td align="right"><strong>334.8 MB</strong></td>
            <td align="right"><strong>2.9× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>OpenCode</strong></td>
            <td align="right"><strong>3237.2 MB</strong></td>
            <td align="right"><strong>27.7× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>GitHub Copilot CLI</strong></td>
            <td align="right"><strong>1756.5 MB</strong></td>
            <td align="right"><strong>15.0× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>Cursor Agent</strong></td>
            <td align="right"><strong>1632.4 MB</strong></td>
            <td align="right"><strong>14.0× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>Claude Code</strong></td>
            <td align="right"><strong>2300.6 MB</strong></td>
            <td align="right"><strong>19.7× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>Antigravity CLI</strong></td>
            <td align="right"><strong>1021.2 MB</strong></td>
            <td align="right"><strong>8.7× more RAM</strong></td>
          </tr>
        </tbody>
      </table>
    </td>
  </tr>
</table>

</div>

### Time to first frame

<div align="center">

| Tool | Time to first frame | Range | Comparison |
|---|---:|---:|---:|
| **jcode** | **14.0 ms** | 10.1–19.3 ms | baseline |
| **Antigravity CLI** | **383.5 ms** | 363.1–415.4 ms | **27.4× slower** |
| **pi** | **590.7 ms** | 369.6–934.8 ms | **42.2× slower** |
| **Codex CLI** | **882.8 ms** | 742.3–1640.9 ms | **63.1× slower** |
| **OpenCode** | **1035.9 ms** | 922.5–1104.4 ms | **74.0× slower** |
| **GitHub Copilot CLI** | **1518.6 ms** | 1357.4–1826.8 ms | **108.5× slower** |
| **Cursor Agent** | **1949.7 ms** | 1711.0–2104.8 ms | **139.3× slower** |
| **Claude Code** | **3436.9 ms** | 2032.7–8927.2 ms | **245.5× slower** |

</div>

Measured on this Linux machine across 10 interactive PTY launches.

### Time to first input
(time until typed probe text appears on the rendered screen; Antigravity uses its internal input-ready log marker because the sign-in screen suppresses probe echo.)
<div align="center">

| Tool | Time to first input | Range | Comparison |
|---|---:|---:|---:|
| **jcode** | **48.7 ms** | 30.3–62.7 ms | baseline |
| **Antigravity CLI** | **383.7 ms** | 363.4–415.7 ms | **7.9× slower** |
| **pi** | **596.4 ms** | 373.9–955.2 ms | **12.2× slower** |
| **Codex CLI** | **905.8 ms** | 760.1–1675.7 ms | **18.6× slower** |
| **OpenCode** | **1047.9 ms** | 931.1–1116.9 ms | **21.5× slower** |
| **GitHub Copilot CLI** | **1583.4 ms** | 1422.8–1880.0 ms | **32.5× slower** |
| **Cursor Agent** | **1978.7 ms** | 1727.3–2130.0 ms | **40.6× slower** |
| **Claude Code** | **3512.8 ms** | 2137.4–9002.0 ms | **72.2× slower** |

</div>

Measured on this Linux machine across 10 interactive PTY launches. Antigravity CLI was unauthenticated for this run; its sign-in screen rendered normally and emitted an internal `CLI ready for user input` marker, but did not echo the typed probe.

### Additional clients / memory scaling

<div align="center">

| Tool | Extra PSS per added session | Comparison |
|---|---:|---:|
| **jcode (local embedding off)** | **~9.9 MB** | baseline |
| **jcode** | **~10.4 MB** | **1.1× more RAM** |
| **pi** | **~76.5 MB** | **7.7× more RAM** |
| **Codex CLI** | **~21.6 MB** | **2.2× more RAM** |
| **OpenCode** | **~318.4 MB** | **32.2× more RAM** |
| **GitHub Copilot CLI** | **~158.1 MB** | **16.0× more RAM** |
| **Cursor Agent** | **~157.5 MB** | **15.9× more RAM** |
| **Claude Code** | **~212.7 MB** | **21.5× more RAM** |
| **Antigravity CLI** | **~86.4 MB** | **8.7× more RAM** |

</div>
versions tested for this corrected memory rerun:

- `jcode v0.9.1888-dev (be386f2)`
- `pi 0.62.0`
- `codex-cli 0.120.0`
- `opencode 1.0.203`
- `GitHub Copilot CLI 1.0.24` for the 1-session rerun, `GitHub Copilot CLI 1.0.27` for the 10-session rerun
- `Cursor Agent 2026.04.08-a41fba1`
- `Claude Code 2.1.86 (Claude Code)`
- `Antigravity CLI 1.0.0`

<div align="center">

  <a href="https://github.com/1jehuang/jcode/releases/download/readme-assets/jcode-performance-demo.mp4">
    <img src="https://github.com/1jehuang/jcode/releases/download/readme-assets/jcode-performance-demo.webp" alt="jcode performance demonstration" width="900">
  </a>

  <p><em>jcode performance demonstration</em></p>

</div>


---

## Memory (Agent memory)

Jcode embeds each turn/response as a semantic vector. Every turn does queries a graph of memories to efficiently find related memory entries via a cosine similarity check. The embedding hits are fed into the conversation, or optionally uses a memory sideagent which verifies the memories are relevant, and potentially does more work for information retreival before injecting into the conversation. This results in a human like memory system which allows the agent to automatically recall relevant information to the conversation without actively calling memory tools or being a token burner. 
ot 
To have memories which are retrieved, they must also be extracted and stored. Every so often (semantic drift, K turns since last extraction, session end, etc), memories are extracted via a memory sideagent, and put into the memory graph. 

The harness also provides explicit memory tools to allow the agent to actively search or store the memory without relying on a passive background process. The harness also provides session search for traditional RAG on previous sessions. 

Memories are automatically consolidated every so often via the ambient mode. This reorganizes, checks for staleness and conflicts, etc

<div align="center">

  <a href="https://github.com/1jehuang/jcode/releases/download/readme-assets/jcode-memory-demo.mp4">
    <img src="https://github.com/1jehuang/jcode/releases/download/readme-assets/jcode-memory-demo.webp" alt="jcode memory demonstration" width="900">
  </a>

  <p><em>jcode memory demonstration</em></p>

</div>

<!-- Memory demo media is hosted in the readme-assets release. -->

---

## UI: Side panels, Diagrams, Info Widgets, rendering, scrolling, alignment

The side panel is a place for auxiliary information. Tell your jcode agent to load a file into the side panel and see it update in real time, or tell your agent to write directly to the side panel, or use it as a diff viewer. The side panel (and chat) is able to render mermaid diagrams inline. 
<img width="2877" height="1762" alt="image" src="https://github.com/user-attachments/assets/6c7bec81-ef3f-434d-8a7b-d55f8a54e5cf" />

To make this possible, I created a new mermaid rendering library to render diagrams 1800x faster. It has no browser or Typescript dependency. See https://github.com/1jehuang/mermaid-rs-renderer

To show you important information without taking space away from the screen that could be used for responses, I developed info widgets. Info widgets will only ever take up the negative space on the screen to show you information, and will get out of the way if there isn't any. 

Jcode can render at over a thousand fps. Your monitor will not have the refresh rate to show you, but this means you will not have silly flicker problems. 

The custom scrollback implementation of jcode allows it to do much more than a native scrollback. However, it is a terminal-level limitation that I cannot have smooth, partial line scrolling with a custom scrollback. To fix this, I made my own terminal. Handterm https://github.com/1jehuang/handterm implements a native scroll api, and also happens to be very efficient. This is a work in progress. Scrolling is still well implemented for normal terminals.

Jcode is left-aligned by default. You can switch to centered mode with the `Alt+C` hotkey, with the `/alignment` command, or in the config.

To disable emoji globally in TUI and CLI output, set `emoji = false` under `[display]` in `~/.jcode/config.toml`, or launch with `JCODE_NO_EMOJI=1`. Jcode replaces emoji with compact ASCII markers while preserving other Unicode text.

---

## Swarm

Spawn two or more agents in the same repo, and they will automatically be managed by the server to allow native collaboration. When agent A edits a file that agent B has read (code shifting under its feet), the server notifies agent B. Agent B can ignore it if it is not relevant, or it can check the diff to make sure that it doesn't conflict. Each agent has messaging abilities, capable of DMing just one agent, broadcasting to all other agents hosted by the server, or just agents working in that repo. This allows you to spawn multiple sessions in the same repo, and have all conflicts automatically resolved.

<div align="center">

  <a href="https://github.com/1jehuang/jcode/releases/download/readme-assets/swarm-demo.mp4">
    <img src="https://github.com/1jehuang/jcode/releases/download/readme-assets/jcode-swarm-demonstration.webp" alt="jcode swarm demonstration" width="900">
  </a>

  <p><em>jcode swarm demonstration</em></p>

</div>

Agents are also able to spawn their own swarms autonomously. They have a swarm tool which allows them to spawn in their own teamates to accomplish tasks in parallel. Doing so turns the main agent into a coordinator and the spawned agents into workers. Groups of agents, their messaging channels, their completion statuses, etc are all automatically managed. This can be done headlessly or headed.

---

## OAuth and Providers

jcode works with subscription-backed OAuth flows and many provider integrations, so you can use the models you already pay for and still fall back to direct API providers when needed.

### Supported built-in login flows

- **Claude** (`jcode login --provider claude`)
- **OpenAI / ChatGPT / Codex** (`jcode login --provider openai`)
- **Google Gemini** (`jcode login --provider gemini`)
- **GitHub Copilot** (`jcode login --provider copilot`)
- **Azure OpenAI** (`jcode login --provider azure`)
- **Alibaba Cloud Coding Plan** (`jcode login --provider alibaba-coding-plan`)
- **Fireworks** (`jcode login --provider fireworks`)
- **Novita AI** (`jcode login --provider novita`, API key)
- **MiniMax** (`jcode login --provider minimax`)
- **Meta Model API / Muse** (`jcode login --provider meta-muse`)
- **LM Studio** (`jcode login --provider lmstudio`)
- **Ollama** (`jcode login --provider ollama`)
- **Custom OpenAI-compatible endpoint** (`jcode login --provider openai-compatible`)

For custom OpenAI-compatible endpoints, jcode now prompts for the API base and supports local localhost servers without requiring an API key.

The native OpenAI providers use Responses WebSocket v2 with opportunistic
background prewarming and HTTPS fallback. See [OpenAI WebSocket transport](docs/OPENAI_WEBSOCKET.md)
for behavior, controls, and verification.

### Config-file setup for self-hosted endpoints and MCP

If you prefer to configure things by editing files instead of using the login UI, jcode supports both a custom OpenAI-compatible endpoint config and MCP config files.

#### OpenAI-compatible providers

Many hosted services speak the standard OpenAI `/v1/chat/completions` API. jcode talks to them through one shared OpenAI-compatible provider, so you can use almost any such endpoint without waiting for a dedicated integration.

There are two ways to set one up:

- **Built-in named profiles** — jcode ships ready-made profiles for several popular OpenAI-compatible services. Log in by id and jcode fills in the base URL and key environment variable for you:

  ```bash
  jcode login --provider <profile-id>
  # for example:
  jcode login --provider openrouter
  jcode login --provider orcarouter
  jcode login --provider deepseek
  jcode login --provider opencode      # OpenCode Zen
  jcode login --provider moonshotai
  jcode login --provider meta-muse     # Meta Model API / Muse Spark
  ```

  Built-in OpenAI-compatible profile ids include: `openrouter`, `orcarouter`, `deepseek`, `zai`, `kimi`, `moonshotai`, `meta-muse` (Meta Model API / Muse Spark), `opencode` (OpenCode Zen), `opencode-go`, `302ai`, `baseten`, `cortecs`, `huggingface`, `nebius`, `scaleway`, `stackit`, and `firmware`. Each profile only sets the endpoint and key variable; you still pick the model with `/model` (or `--model`). Run `jcode login` with no provider to see the interactive list.

- **Any other endpoint** — point jcode at an arbitrary OpenAI-compatible API (hosted or local) with `jcode login --provider openai-compatible` or the scriptable `jcode provider add` command described below.

Useful environment overrides for these endpoints:

- `JCODE_STREAM_IDLE_TIMEOUT_SECS` — raise the base streaming idle timeout (default 180s) for slow reasoning models that think silently before emitting tokens. High reasoning efforts scale this automatically (high 2x, xhigh 3x, max 4x). Also settable as `[provider] stream_idle_timeout_secs` in `config.toml`.
- Per-model `context_window` (alias `context_limit`) in a `[[providers.<name>.models]]` entry — set the context window when the endpoint has no usable `/v1/models` response, so jcode does not fall back to the generic 200k default.
- `extra_body` — inject non-standard top-level fields into every chat/completions request body for backends that require them. See [Extra request-body fields](#extra-request-body-fields-extra_body) below.

For details on self-hosting, local runtimes, and the exact config file shape, see below.

#### Self-hosted OpenAI-compatible endpoints, including vLLM

For agents and scripts, the preferred path is the one-shot provider profile command. It writes a named profile to `~/.jcode/config.toml`, stores secrets in jcode's private app config directory when requested, and prints exact run/validation commands:

```bash
# Secret-safe setup for a hosted OpenAI-compatible API.
printf '%s' "$MY_API_KEY" | jcode provider add my-api \
  --base-url https://llm.example.com/v1 \
  --model my-model-id \
  --api-key-stdin \
  --set-default \
  --json

# Smoke test the profile.
jcode --provider-profile my-api auth-test --prompt 'Reply exactly JCODE_PROVIDER_SETUP_OK'

# Use it directly.
jcode --provider-profile my-api run 'hello'
```

For local servers that do not require auth:

```bash
jcode provider add local-vllm \
  --base-url http://localhost:8000/v1 \
  --model Qwen/Qwen3-Coder-30B-A3B-Instruct \
  --no-api-key \
  --set-default
```

Built-in local profiles are available for the common desktop/local runtimes:

```bash
# Ollama: start the local server and install a model first.
ollama pull llama3.2
jcode login --provider ollama
jcode --provider ollama --model llama3.2 run 'hello'

# LM Studio: start the Local Server, load a chat model, then use the exact
# model identifier shown by LM Studio or by curl http://localhost:1234/v1/models.
jcode login --provider lmstudio
jcode --provider lmstudio --model '<model-id>' run 'hello'
```

Ollama and LM Studio both expose OpenAI-compatible `/v1/models` and `/v1/chat/completions` endpoints. jcode uses streaming chat completions, function/tool calling, and OpenAI-style image content for vision-capable local models. If a local server requires a token, enter it during `jcode login` or create a named profile with `--api-key-stdin`.

Useful flags:

- `--api-key-env NAME`: reference an existing environment variable instead of storing a key.
- `--api-key-stdin`: read and store a key without putting it in shell history.
- `--context-window TOKENS`: persist the model context window for model selection and routing.
- `--overwrite`: replace an existing profile of the same name.
- `--model-catalog`: use the endpoint's `/models` response in addition to configured models.

The generated profile can also be edited manually in `~/.jcode/config.toml`:

```toml
[provider]
default_provider = "my-api"
default_model = "my-model-id"

[providers.my-api]
type = "openai-compatible"
base_url = "https://llm.example.com/v1"
api_key_env = "JCODE_PROVIDER_MY_API_API_KEY"
env_file = "provider-my-api.env"
default_model = "my-model-id"
# Optional: prevent model names such as `gpt-5-*` from automatically enabling
# `reasoning_effort` on gateways that reject it.
disable_reasoning_heuristics = true

[[providers.my-api.models]]
id = "my-model-id"
context_window = 128000
# Explicitly enable `/effort` and select this model's initial effort. Set
# `reasoning = false` on an individual model to disable it instead.
reasoning = true
reasoning_effort = "high"
```

Anthropic Messages-compatible gateways use the same named-profile surface with
`type = "anthropic-compatible"`. The profile can select bearer, custom-header,
or no authentication and attach gateway-specific headers to every request:

```toml
[provider]
default_provider = "corp-claude"
default_model = "claude-sonnet-4-6"

[providers.corp-claude]
type = "anthropic-compatible"
base_url = "https://gateway.example.com/anthropic/v1"
auth = "bearer"
api_key_env = "CORP_CLAUDE_TOKEN"
default_model = "claude-sonnet-4-6"

[providers.corp-claude.headers]
x-tenant-id = "tenant-42"

[[providers.corp-claude.models]]
id = "claude-sonnet-4-6"
context_window = 200000
```

For direct environment-based configuration, `ANTHROPIC_BASE_URL` overrides the
non-OAuth Messages endpoint and `ANTHROPIC_AUTH_TOKEN` is sent as a bearer token.
Claude OAuth traffic always continues to use Anthropic's official endpoints.

##### Extra request-body fields (`extra_body`)

Some OpenAI-compatible backends require non-standard top-level request fields. For example, NVIDIA NIM DeepSeek-V4 reasoning models (`deepseek-ai/deepseek-v4-flash`, `deepseek-ai/deepseek-v4-pro`) only enable thinking when the request includes `chat_template_kwargs`; without it they reply without reasoning (or, for some deployments, hang). jcode lets you inject arbitrary top-level fields two ways.

1. Per named profile, via `extra_body` in `config.toml` (a TOML table merged verbatim into the JSON body):

   ```toml
   [providers.my-nim]
   type = "openai-compatible"
   base_url = "https://integrate.api.nvidia.com/v1"
   api_key_env = "NVIDIA_API_KEY"
   default_model = "deepseek-ai/deepseek-v4-flash"

   [providers.my-nim.extra_body.chat_template_kwargs]
   thinking = true
   reasoning_effort = "high"
   ```

2. For built-in profiles (e.g. `nvidia-nim`) or any endpoint, via the `JCODE_OPENAI_EXTRA_BODY` environment variable (a JSON object string). It can live in the provider's env file (`~/.config/jcode/nvidia-nim.env`) next to the API key:

   ```bash
   JCODE_OPENAI_EXTRA_BODY={"chat_template_kwargs":{"thinking":true,"reasoning_effort":"high"}}
   ```

Keys from `extra_body` are merged last and override any jcode-generated body field with the same name (`JCODE_OPENAI_EXTRA_BODY` wins over the config `extra_body` on key collisions). Invalid values are logged and ignored rather than failing the request.

The custom OpenAI-compatible provider reads overrides from environment variables or from an env file in jcode's app config directory. On Linux this is usually `~/.config/jcode/`, so the default file is usually:

```text
~/.config/jcode/openai-compatible.env
```

Example for a local or LAN vLLM server:

```bash
JCODE_OPENAI_COMPAT_API_BASE=http://192.168.1.50:8000/v1
JCODE_OPENAI_COMPAT_DEFAULT_MODEL=Qwen/Qwen3-Coder-30B-A3B-Instruct
# Optional if your server expects auth
OPENAI_COMPAT_API_KEY=your-token-here
```

Notes:

- `jcode login --provider openai-compatible` can create or update this for you.
- Plain `http://` is accepted for `localhost` and private LAN IPs. Public remote HTTP is still rejected.
- HTTPS endpoints work as usual.

#### MCP config files

MCP config is separate from `config.toml`.

Primary config files:

- `~/.jcode/mcp.json` for global MCP servers
- `.jcode/mcp.json` for project-local MCP servers

Claude Code compatibility:

- `~/.claude.json` (Claude Code's user config): top-level `mcpServers`, plus per-project servers under `projects.<abs_path>.mcpServers` for the current directory
- `.mcp.json` at the repo root (Claude Code's project config)
- `.claude/mcp.json` (legacy fallback)

Claude Code config is read live on every load rather than copied into jcode's
global config. Additions, edits, and deletions therefore take effect without
leaving a stale snapshot (and inline environment values are not duplicated).
For migration from Codex CLI, jcode still performs a one-time import from
`~/.codex/config.toml` into `~/.jcode/mcp.json` when the latter does not exist.
That imported file is then jcode-owned; later Codex changes are not synced
automatically. Imported environment values are copied too and may contain
secrets.

Both the canonical `mcpServers` key and jcode's historical `servers` key are accepted. jcode currently supports stdio (command-based) servers only; HTTP/SSE entries (`"type": "http"`/`"sse"`) are recognized and skipped with a log line.

Example MCP config:

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "/path/to/mcp-server",
      "args": ["--root", "/workspace"],
      "env": {},
      "shared": true
    },
    "websearch": {
      "command": "/path/to/slow-mcp-server",
      "timeout_secs": 120
    }
  }
}
```

Each request to an MCP server (`tools/call`, `tools/list`, `initialize`) times out after 30 seconds by default. Set `timeout_secs` on a server whose tools legitimately run longer.

For headless or SSH sessions, OAuth-style providers support `jcode login --provider <provider> --no-browser` (alias: `--headless`) so jcode prints the auth URL/QR and falls back to manual code or callback paste instead of trying to launch a local browser.

For more scriptable remote flows, `claude`, `openai`, `gemini`, and `antigravity` also support a two-step pattern:

```bash
# Step 1: print a resumable auth URL
jcode login --provider openai --print-auth-url --json

# Step 2: complete later with the callback URL or auth code
jcode login --provider openai --callback-url 'http://localhost:1455/auth/callback?...'
jcode login --provider gemini --auth-code '...'
```

Additional scriptable cases:

```bash
# Copilot device flow: print URL + user code, then complete later
jcode login --provider copilot --print-auth-url --json
jcode login --provider copilot --complete

# Gmail/Google OAuth after credentials are already configured
jcode login --provider google --print-auth-url --google-access-tier readonly
jcode login --provider google --callback-url 'http://127.0.0.1:8456?...'
```

Pending scriptable login state is stored under `~/.jcode/pending-login/`, automatically expires, and stale entries are cleaned up when new scriptable logins start or resume.

For the built-in OpenAI login flow, jcode opens a local callback on
`http://localhost:1455/auth/callback` by default.

<img width="2877" height="1762" alt="Screenshot from 2026-04-02 14-28-51" src="https://github.com/user-attachments/assets/530684c0-9d12-4363-aa0e-1b39a0d4e1be" />
The above image is the first page of provider logins

### Supported provider

- **Native / first-party style providers:** `claude`, `openai`, `copilot`, `gemini`, `azure`, `alibaba-coding-plan`
- **Aggregator / compatibility providers:** `openrouter`, `orcarouter`, `openai-compatible`
- **Additional provider integrations:** `opencode`, `opencode-go`, `zai` / `kimi`, `302ai`, `baseten`, `cortecs`, `deepseek`, `firmware`, `huggingface`, `moonshotai`, `nebius`, `scaleway`, `stackit`, `groq`, `mistral`, `perplexity`, `togetherai`, `deepinfra`, `fireworks`, `novita`, `minimax`, `xai`, `lmstudio`, `ollama`, `chutes`, `cerebras`, `cursor`, `antigravity`, `google`

Jcode also supports easy multi-account switching. Ran out of tokens on your first ChatGPT Pro subscription? /account and quickly switch to your second. 

---

## Customizability / Self-Dev

Jcode is inventing a new form of customizability. One that doesn't limit you to what a plugin or extension can do. Tell your jcode agent to enter self dev mode, and it will start modifying its own source code. Jcode is optimized to iterate on itself. There is significant infrastructure around self developement, which allows it to edit, build, and test its own source code, then reload its own binary and continue work in your (potentially many) sessions, fully automatically. 

It is reccomended that you use a frontier model for this. The jcode codebase is not a simple one, and weaker models can make subtle, breaking changes. GPT 5.5 or the latest available frontier model works well.

<!-- Add self-dev demo thumbnail/video and fuller writeup here. -->

---

## Misc.

The devil is in the details. There are many undocumented optimizations and niceties that jcode implements. Some examples: 

Anthropic's Claude cache goes cold after 5 minutes. If you initiate Claude after these 5 minutes, you have a cache miss, potentially costing you lots of tokens. The ui warns you when the cache went cold, and notfies you if there was an unexpected cache miss. 

jcode comes with instructions on how to set up Firefox Agent Bridge. Ask you agent to set it up, and then you will have browser automation in jcode as well. 

Agent grep is a grep tool I made for the jcode agent. It adds file strucuture information (ie the list of functions, their displacement, etc) to the grep return, so that the agent can infer more of what the file doesn without actually reading the file. It also implements a harness-level integration that adaptively truncates returns based on what the agent has already seen. This saves on context a lot. 

Inputs are by default interleaved with the working agent. It sends the input as soon as it safely can without breaking the KV cache. Submit with shift enter instead, and it will send a queue send, and wait for the agent to fully finish its turn before sending.

Resume sessions from different harnesses. Claude code broke on you? Resume the session from jcode and continue where you left off. Session resume is supported for codex, claude code, opencode, and pi. 

<img width="2877" height="1762" alt="Screenshot from 2026-04-11 16-28-52" src="https://github.com/user-attachments/assets/c2b383cf-2531-4217-85ae-6a863354dc97" />
image of /Resume for codex sessions


Skills are not all loaded on startup. The conversation is embedded as a semantic vector, and will automatically inject a skill if there is an embedding hit similar to memories. The agent has a skill tool for you to manually activate a skill at anytime. You may also activate via slash commands. 

---

## iOS Application / Native OpenClaw

A native iOS application version of jcode is coming soon. This will allow you to work with jcode on your personal machine's environment from your phone, via Tailscale. Openclaw like features will be bundled with this iOS application. 

---

## Other planned features

Agents dont like to commit in dirty git state with active changes. Git was clearly not built for multi-agent workflows, and git worktrees is not a good solution. Given this, I believe that is an opporunity for a new git like primitive to be born. 

Build speed improvements: An incremental debug cargo build with cache enabled takes about 1 minute on my machine. The goal is 5-20 seconds. Refactors and crates seams should be able to make this happen. 

<!-- Add iOS / native OpenClaw preview and fuller writeup here. -->

---

<div align="center">

## Quick Start

</div>

```bash
# Launch the TUI
jcode

# Run a single command non-interactively
jcode run "say hello"

# Run with a named session profile from ~/.jcode/config.toml
jcode --profile review run "review this change"

# Resume a previous session by memorable name
jcode --resume fox

# Run as a persistent background server, then attach more clients
jcode serve
jcode connect

# Send voice input from your configured STT command
jcode dictate
```

jcode supports interactive TUI use, non-interactive runs, persistent server/client workflows,
and hotkey-friendly dictation without requiring a bundled speech-to-text stack.

Named session profiles live under `[profiles.<name>]` in `~/.jcode/config.toml` and bundle
provider, model, reasoning effort, tools, skill policy, and additive instructions for a
session. This is the interactive follow-on to the completed headless contract in
`specs/003-add-session-profiles`; no-profile behavior and existing `jcode run` output
modes remain unchanged.

```bash
# Start the TUI with a profile (validated before provider/session creation)
jcode --profile review

# Inside the TUI: open the searchable picker, choose directly, or clear the profile
/profile
/profile review
/profile none

# Inspect profiles without initializing a provider or changing session state
jcode profile list
jcode profile show review
jcode profile current
jcode profile resolve review
```

`/profile <name>` takes effect at the next turn boundary; an in-flight turn, child,
or swarm worker keeps its immutable policy. Selecting `none` restores the normal
unprofiled resolution for future turns. `profile list` is stable and marks the current
selection; `show`, `current`, `resolve`, and `/info` report safe values and sources only
(instruction bodies and credentials are represented by presence/length metadata).

Profiles support `skills_mode = "all"`, `"allowlist"`, or `"none"`, plus `skills` and
subtractive `disabled_skills`; omitted `skills_mode` preserves legacy no-profile skill
behavior. Child agents and swarm workers inherit the effective profile, tool policy, and
skill policy unless an explicit child override is supplied. Session metadata stores the
selected name and a credential-free resolved snapshot. Restore uses that snapshot and
warns when the named profile is missing or changed; it never silently falls back.

The TUI `@` file picker is enabled by default and discovers suggestions in bounded background
batches so typing remains responsive. Sending an existing `@path` includes the full UTF-8 file
contents in the model-facing message while keeping the compact `@path` in the visible transcript.
Missing, binary, or oversized paths remain literal so the agent can handle them normally. Set
`enabled = false` to opt out of filesystem-backed suggestions and submit-time expansion. The
picker skips common generated and dependency directories by default. Add custom global patterns
under `[file_mentions]`, or add profile-specific patterns under `[profiles.<name>]`:

```toml
[file_mentions]
# enabled = false # Uncomment to opt out.
ignore = ["private/", "*.generated.*"]

[profiles.review]
file_mentions_ignore = ["fixtures/", "snapshots/2024/"]
```

Profile patterns are additive to the global list and apply when that profile is active.

The generated config template documents the field list and precedence (explicit
invocation > environment > selected profile > unprofiled persisted config > built-in
default). Unknown profiles and invalid values fail before a provider request with the
profile/key and available choices or correction guidance; selecting one never rewrites
the config or changes another session.

<div align="center">

  <a href="https://github.com/1jehuang/jcode/releases/download/readme-assets/workflow.mp4">
    <img src="https://github.com/1jehuang/jcode/releases/download/readme-assets/jcode-workflow-demonstration.webp" alt="jcode workflow demonstration" width="900">
  </a>

  <p><em>jcode workflow demonstration</em></p>

</div>

---

## Browser Automation

jcode includes a first-class built-in `browser` tool for browser control inside agent sessions.

Current built-in backend:
- Firefox via Firefox Agent Bridge

Current built-in tool actions include:
- `status`
- `setup`
- `open`
- `snapshot`
- `get_content`
- `interactables`
- `click`
- `type`
- `fill_form`
- `select`
- `wait`
- `screenshot`
- `eval`
- `scroll`
- `upload`
- `press`

Quick setup:

```bash
jcode browser status
jcode browser setup
```

Once setup is complete, the model can use the built-in `browser` tool directly. The UI also summarizes browser tool calls compactly, for example opening a URL, clicking a selector, or typing into a field without echoing sensitive typed text.

Notes:
- the provider/tool architecture is in place for additional backends
- Firefox is the wired built-in backend today
- Chrome bridge / remote debugging style providers can be added on top of the same browser tool later

---

## Further Reading

- [jcode.sh/docs](https://jcode.sh/docs) — install, providers, configuration, keybindings
- [jcode.sh/swarm](https://jcode.sh/swarm) — many coding agents in one repository
- [jcode.sh/sdk](https://jcode.sh/sdk) — TypeScript SDK: drive jcode sessions from your own program
- [jcode.sh/bench](https://jcode.sh/bench) — benchmark methodology and results
- [Ambient Mode / OpenClaw](docs/AMBIENT_MODE.md)
- [Browser Provider Protocol](docs/BROWSER_PROVIDER_PROTOCOL.md)
- [DAP Debugger Operations](docs/DAP.md)
- [Opt-in LSP Feedback and Diagnostics](docs/LSP.md)
- [Memory Architecture](docs/MEMORY_ARCHITECTURE.md)
- [Swarm Architecture](docs/SWARM_ARCHITECTURE.md)
- [Server Architecture](docs/SERVER_ARCHITECTURE.md)
- [Safety System](docs/SAFETY_SYSTEM.md)
- [Sponsored Discovery Sponsor Onboarding](docs/SPONSORED_DISCOVERY_SPONSOR_ONBOARDING.md)
- [Windows Notes](docs/WINDOWS.md)
- [Wrappers and Shell Integration](docs/WRAPPERS.md)
- [Refactoring Notes](docs/REFACTORING.md)

---

## Detailed Installation

### Setup

If you want another agent to set up jcode for you, give it this prompt:

```text
Set up jcode on this machine for me.

1. Detect the operating system, available package managers, and shell environment, then install jcode using the best matching command below instead of referring me somewhere else:

   - macOS with Homebrew available:
     brew tap 1jehuang/jcode
     brew install jcode

   - macOS or Linux via install script:
     curl -fsSL https://jcode.sh/install | bash

   - Windows PowerShell:
     irm https://jcode.sh/install.ps1 | iex

   - From source if the above paths are not appropriate:
     git clone https://github.com/1jehuang/jcode.git
     cd jcode
     cargo build --release
     scripts/install_release.sh

   - For local self-dev / refactor work on Linux x86_64, prefer:
     scripts/dev_cargo.sh build --release -p jcode --bin jcode
     scripts/dev_cargo.sh --print-setup
     scripts/install_release.sh

2. Verify that `jcode` is on my `PATH`.
3. Launch `jcode` once in a new terminal window/session to confirm it starts successfully.
4. Before attempting any interactive login flow, assess which providers are already available non-interactively and prefer those first. Check existing local credentials, config files, CLI sessions, and environment variables such as:
   - Claude: `~/.jcode/auth.json`, `~/.claude/.credentials.json`, `~/.local/share/opencode/auth.json`, `ANTHROPIC_API_KEY`
   - OpenAI: `~/.jcode/openai-auth.json`, `~/.codex/auth.json`, `OPENAI_API_KEY`
   - Gemini: `~/.jcode/gemini_oauth.json`, `~/.gemini/oauth_creds.json`
   - GitHub Copilot: existing auth under `~/.config/github-copilot/`
   - Azure OpenAI: `~/.config/jcode/azure-openai.env`, `AZURE_OPENAI_*`, or an existing `az login`
   - OpenRouter: `OPENROUTER_API_KEY`
   - Fireworks: `~/.config/jcode/fireworks.env`, `FIREWORKS_API_KEY`
   - Novita AI: `~/.config/jcode/novita.env`, `NOVITA_API_KEY`
   - MiniMax: `~/.config/jcode/minimax.env`, `MINIMAX_API_KEY`
   - NVIDIA NIM: `~/.config/jcode/nvidia-nim.env`, `NVIDIA_API_KEY`
   - Alibaba Cloud Coding Plan: existing jcode config/env if present
5. Prefer whichever provider is already configured and verify it with `jcode auth-test --all-configured` or a provider-specific auth test when appropriate.
6. Only if no usable provider is already configured, guide me through the minimal manual step needed:
   - Claude: `jcode login --provider claude`
   - GitHub Copilot: `jcode login --provider copilot`
   - OpenAI: `jcode login --provider openai`
   - Gemini: `jcode login --provider gemini`
   - Azure OpenAI: `jcode login --provider azure`
   - Fireworks: `jcode login --provider fireworks`
   - MiniMax: `jcode login --provider minimax`
   - NVIDIA NIM: `jcode login --provider nvidia-nim`
   - Alibaba Cloud Coding Plan: `jcode login --provider alibaba-coding-plan`
   - OpenRouter: help me set `OPENROUTER_API_KEY`
   - Anthropic direct API: help me set `ANTHROPIC_API_KEY`
7. After setup, run a simple smoke test with `jcode run "say hello"` and confirm it works.
8. If I want browser automation, also check `jcode browser status`. If browser automation is not ready, run `jcode browser setup`, verify the built-in `browser` tool works, and explain any remaining manual step.
9. Explain any manual step that still needs me, especially browser OAuth, device login, API key entry, or browser extension approval.
```

This is intended to be a copy-paste bootstrap prompt for jcode itself or any other coding agent.

### Quick Install

```bash
# macOS & Linux
curl -fsSL https://jcode.sh/install | bash
```

On Termux, install the glibc runtime and `patchelf` first so the installer can
patch the downloaded Linux binary to Termux's glibc dynamic linker and create a
launcher that avoids Termux's `LD_PRELOAD` shim:

```bash
pkg install glibc patchelf
curl -fsSL https://jcode.sh/install | bash
```

```powershell
# Windows 11 x64 or ARM64 (PowerShell 5.1+)
irm https://jcode.sh/install.ps1 | iex
```

The Windows installer selects the correct architecture and verifies the download
against the release's `SHA256SUMS`. Alacritty and the optional global launch
hotkey require explicit consent and are not installed by default. See
[Windows support, security, Defender, and SmartScreen notes](docs/WINDOWS.md).

If a release does not contain a matching Windows asset, the installer stops
instead of unexpectedly starting a long compilation. An explicit source build
is available with `-BuildFromSource` and requires Git, Rust, and Visual Studio
2022 Build Tools with the **Desktop development with C++** workload.

### macOS via Homebrew

```bash
brew tap 1jehuang/jcode
brew install jcode
```

### From Source (all platforms)

```bash
git clone https://github.com/1jehuang/jcode.git
cd jcode
cargo build --release
```

For local self-dev / refactor work on Linux x86_64, prefer:

```bash
scripts/dev_cargo.sh build --release -p jcode --bin jcode
scripts/dev_cargo.sh --print-setup
```

That wrapper automatically uses `sccache` when available, prefers a fast
working local linker setup (`clang + lld`) instead of assuming every machine's
`mold` configuration is valid, and can print the active linker/cache setup via
`--print-setup` so slow-path builds are easier to diagnose.

Then symlink to your PATH:

```bash
scripts/install_release.sh
```

From the repository root, the same global reinstall workflow is also available
through Make:

```bash
make install-fast  # Fast release rebuild, install, and server reload
make install       # Full LTO release rebuild, install, and server reload
make i             # Short alias for make install
```

#### Post-merge dev workflow

After merging a feature into your checkout, the normal workflow is simply:

```bash
make install-fast   # or: make install
```

The installer builds the current checkout, updates the `current` and launcher
symlinks, and then asks any running shared daemon to perform a **graceful,
conditional reload** onto the new binary. No manual daemon restart is normally
needed. If no daemon is running, the reload step is a no-op and the next
`jcode` launch starts the installed binary.

The reload keeps the same socket and persisted session records. Other jcode
clients therefore reconnect and continue using their existing sessions rather
than being discarded. A turn or tool that is actively running during the brief
handoff can still be interrupted, so wait for active work to finish first when
that operation must not be interrupted.

### Uninstall

Removes installed binaries and the launcher but keeps your config, auth, and
sessions so a clean reinstall picks up where you left off:

```bash
curl -fsSL https://raw.githubusercontent.com/1jehuang/jcode/master/scripts/uninstall.sh | bash -s -- --yes
```

For a full wipe of everything including config, auth, sessions, logs, and
memory (useful for recovering from a broken install):

```bash
curl -fsSL https://raw.githubusercontent.com/1jehuang/jcode/master/scripts/uninstall.sh | bash -s -- --purge --yes
```

Add `--dry-run` to preview what would be removed without deleting anything.

### Platform Support

| Platform | Status |
|---|---|
| **Linux** x86_64 / aarch64 | Fully supported |
| **macOS** Apple Silicon & Intel | Supported |
| **Windows** x86_64 | Supported (native + WSL2) |
| **Termux** aarch64 / x86_64 | Supported with `pkg install glibc patchelf` |

</div>
