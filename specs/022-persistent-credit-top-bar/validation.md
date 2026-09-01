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
