# Persistent credit top bar validation matrix

## Baseline

- Base revision: `356e867d2` on `dev`.
- Feature branch: `022-persistent-credit-top-bar` in the dedicated worktree.
- Existing behavior: session/server/client/model/auth context is composed into the scrollable transcript header. Context and model facts may also appear opportunistically in unused right-side cells. Provider quota or cost data is available through `InfoWidgetData::usage_info` and the existing usage widget.
- Unrelated baseline issue: `cargo test -p jcode-tui --lib` did not terminate after more than 20 minutes. The test process was sleeping on futex/epoll and was terminated so the workflow could continue. Do not treat that broad stateful suite as focused acceptance evidence. Use exact top-bar/config tests first and record any later broad-suite result separately.

## Acceptance matrix

| Requirement | Direct check |
|---|---|
| Persistent at every transcript scroll position | Ratatui `TestBackend` render at bottom and paused scroll offsets. Assert the same top rows contain identity and credit fields. |
| Session, provider/auth, model, and reasoning are current | Pure context derivation tests plus rendered text checks before and after changing each source value. |
| OpenAI OAuth and other provider credit formats reuse current data | Fixtures for OpenAI/Anthropic subscription windows, cost-based providers, Copilot token-only data, and unsupported providers. Assert no render-path fetch. |
| Known, pending, stale, unavailable, and not-applicable states are honest | Exact formatter tests. Unknown or unsupported data must never render as zero. Raw provider errors and credentials must not render. |
| Normal terminals use 1-2 rows and roomy terminals use at most 3 | Size matrix at 40x12, 60x16, 80x24, 120x32, and 160x48. Assert row count and field priority. |
| Chat remains readable on constrained terminals | Assert the bar compacts or suppresses before the transcript loses its minimum row or the complete status/input chrome. |
| No overlap with panes, scrollbars, overlays, status, or input | Compare recorded rectangles and rendered buffers with each surface enabled independently and in supported combinations. |
| Typing and paste preserve transcript anchor, text, and logical cursor | Render before/after single-line typing, multiline paste, usage refresh, and height-changing resize while paused above the tail. |
| Disabled preference reserves zero rows | Config decode/round-trip tests plus enabled/disabled layout comparison at identical dimensions. |
| Unicode and long labels remain bounded | Pure display-cell width tests using emoji, combining text, wide glyphs, long session names, and long model/provider labels. |
| Stable unchanged rendering | Repeat selector/render derivation 100 times at fixed context and geometry. Assert identical row count and visible field set. |
| Performance stays interactive | Pure derivation/render loop timing check with no I/O. Runtime smoke verifies normal typing remains responsive. |
| Packaging and runtime truth | `selfdev build target=tui`, focused guardrails, then debug-socket/TestBackend frame checks against the newly built binary. |

## Scope and safety boundaries

- Do not modify provider endpoints, provider billing collection, the shared daemon, protocol contracts, SDK contracts, persisted session data, or user credentials.
- Do not read credential files or issue network requests from the render path.
- Preserve existing usage and session sources when the bar is disabled or suppressed.
- Stage only files owned by this feature. Keep the root integration checkout on `dev`.

## Phase 7 validation evidence

### T027: frame-metrics attribution

- Added bounded `FramePerfStats` fields for top-bar context derivation,
  deterministic selection, rendering, row count, visible field kinds, and
  suppression reason. The metadata contains only static semantic field names,
  never user labels, credit values, provider errors, or credentials.
- Instrumented the existing frame path around `top_bar_context`,
  `select_top_bar_layout`, and top-bar clearing/rendering. No new filesystem,
  authentication, network, or provider usage work was added to rendering.
- Focused regression:
  `cargo test -p jcode-tui --lib top_bar_metrics_record_bounded_timing_and_safe_layout_metadata`
  Result: **PASS** (1 passed).
- Existing selector regression
  `adaptive_selector_is_stable_for_one_hundred_unchanged_refreshes` remains
  covered and passed in the focused top-bar test run below.

### T028: focused checks, formatting, build, and guardrails

Commands were run serially:

1. `cargo test -p jcode-config-types --lib top_bar`
   Result: **PASS** (4 passed).
2. `cargo test -p jcode-tui --lib top_bar`
   Result: **PASS** (27 passed), covering config-facing layout behavior,
   usage states, safe labels, Unicode width, 100-refresh determinism, panes,
   overlays, scrollback, multiline input, and disabled-layout behavior.
3. `cargo fmt --all` followed by `cargo fmt --all -- --check`
   Result: **PASS**. Formatting also normalized feature-owned top-bar files
   touched by earlier phases.
4. `selfdev build target=tui`
   Result: **PASS**. Built `target/selfdev/jcode` from the feature worktree.
5. `scripts/check_guardrails.sh`
   Result: **FAIL**, with formatting and dependency-boundary gates passing.
   The remaining failures were recorded rather than hidden: two unrelated
   existing e2e `Request::Subscribe` initializers omit `crash_on_disconnect`,
   an unrelated pre-existing `jcode-base` clippy error is promoted by `-D
   warnings`, warning and quality-ratchet baselines are already exceeded, and
   the accepted feature growth is reported by the oversized-file/test ratchets
   (including the touched TUI files). No ratchet baselines were updated.

The guardrail failure is a delivery caveat for this worktree, not evidence of
a top-bar functional test failure. It remains visible for maintainer review.

### T029: isolated runtime and debug-socket smoke

- Verified the resolved executable before the smoke:
  `target/selfdev/jcode`, SHA-256
  `d4dc79ddcce1851bc8a80c9de23931a39d9b78bd541b1a5205b6e9b67966a138`.
- Started a private daemon with `JCODE_RUNTIME_DIR`, `XDG_RUNTIME_DIR`,
  `JCODE_HOME`, and a private Unix socket. The shared daemon and caller
  session were not used or modified. A final retry removed provider credential
  variables from the private process environment before exercising the client.
- The built interactive client registered over the private debug socket. The
  smoke exercised a real message submission path, multiline input injection,
  repeated assistant-message injection, scroll-to-top, PTY resize/SIGWINCH at
  `120x32`, `40x12`, `60x16`, `160x48`, and `24x8`, visual-debug enable, render
  statistics, and the persisted `[display] top_bar = false` path.
- Captured active-session visual-debug frame evidence from the built binary at
  `120x32`: top-bar bounds were `{x:0,y:0,width:120,height:2}`, visible fields
  were `Session`, `Credit`, `ProviderModel`, `ServerClient`, and
  `VersionConnection`, and the messages/status/input rectangles began at rows
  2, 30, and 31 respectively. The frame reported no anomalies.
- The same active runtime's slow-frame record exposed the new safe metrics:
  derivation `0.114805ms`, selection `0.061145ms`, render `0.024266ms`, two
  rows, and five static visible field kinds. A credential-scrubbed retry also
  reported two rows with `Session`, `Credit`, `ProviderModel`, and
  `ServerClient`; no provider value or error text entered the metrics.
- Multiline input remained `line one\nline two` after scrollback commands in
  the debug client state. The config-off retry reported zero top-bar rows and
  suppression reason `Disabled` in its slow-frame metrics. Focused TestBackend
  checks provide the complete pane/overlay/no-overlap and size-matrix evidence.
- A credential-scrubbed PTY run injected a three-line paste and 20 additional
  characters, then read the existing `client:draw-stats 64` report: 17 draw
  samples, render p95 `4.870902ms`, total p95 `4.993062ms`, and total max
  `9.987055999999999ms`, all below the 100ms responsiveness target. The
  debug-client PTY injection path did not produce `key_to_paint` samples, so
  these are frame-work observations rather than a claim that terminal-to-paint
  latency was measured end to end.
- Runtime limitation: the headless debug client did not retain a new visual
  frame for every resize/debug command, although its real draw history and
  slow-frame metrics advanced. The deterministic TestBackend matrix remains
  the authoritative rendered evidence for those dimensions; no runtime frame
  was fabricated or treated as captured when the tool returned no frame.

The private daemon, PTY client, sockets, and temporary config were terminated
or removed at the end of each smoke. No persistent user configuration or shared
daemon state was changed.
