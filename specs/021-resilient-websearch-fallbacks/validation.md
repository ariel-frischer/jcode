# Resilient Websearch Fallbacks Validation Evidence

This file is the evidence ledger for the frozen Bead acceptance checks for
`jcode-ow6` / `021-resilient-websearch-fallbacks`. Evidence is appended as tasks
complete. This initial entry is intentionally limited to the T001 baseline and
acceptance-evidence plan.

## T001 baseline

- Branch: `021-resilient-websearch-fallbacks`
- Candidate base revision before implementation: `1625d9bb1d4438380adb7c8500c19162c7699da6`
- Baseline HEAD after the setup-only documentation commit: `28387940088b039de98a481fb0f673361ecf74c9`
- Working tree at inspection: clean before the T001 status update and this file
  were created.
- Project/runtime context: Rust 2024 workspace, Tokio, reqwest, ratatui; the
  existing websearch tool is owned by `jcode-app-core`, config types by
  `jcode-config-types`, persisted/environment config by `jcode-base`, and tool
  presentation by `jcode-tui`.

### Existing focused commands

| Area | Command | Baseline result |
|---|---|---|
| Legacy adapters, parsers, aliases | `cargo test -p jcode-app-core websearch --lib` | PASS, 12 passed, 0 failed |
| Config type filter | `cargo test -p jcode-config-types websearch --lib` | No matching tests; repository dev-cargo intentionally returned exit 97 |
| Config tests | `cargo test -p jcode-base config --lib` | 194 passed, 2 pre-existing failures |
| Existing TUI summary | `cargo test -p jcode-tui tui::ui::tests::tools::test_tool_summary_covers_action_shaped_tools_and_fallback --lib` | PASS, 1 passed, 0 failed |
| TUI inventory | `cargo test -p jcode-tui --lib -- --list \| rg -i '(tool_summary\|websearch\|tool_message\|render_tool)'` | Existing summary/render tests listed; no resilient websearch rendering test exists |

The unrelated baseline failures in `jcode-base` were:

1. `config::tests::config_env_fingerprint_tracks_every_apply_env_override_var`:
   the baseline expected `CONFIG_ENV_KEYS` to include `JCODE_WAKE_MODE`.
2. `config::tests::test_generated_default_config_has_expected_user_defaults`:
   the baseline expected the generated config to enable OpenAI fast mode.

The command output also showed existing non-fatal warnings in the TUI crate
(`animated_tool_color` unused import/dead code) and Cargo profile-package
warnings. These are recorded as baseline observations, not attributed to this
feature.

## Legacy compatibility inventory

### Engines, names, and aliases

The canonical `WebSearchEngine` enum currently has exactly:

- `Duckduckgo`, serialized as `duckduckgo`, with input alias `ddg`.
- `Bing`, serialized and parsed as `bing`.
- `Searxng`, serialized as `searxng`, with input alias `searx`.

Unknown engine strings are rejected by the existing parser. The public tool
name is `websearch` and its existing schema requires `query`; optional fields
are `num_results`, `engine`, and `bing_market`.

### Legacy persisted config and defaults

`WebSearchConfig` currently contains the following fields and defaults:

- `engine`: `duckduckgo`.
- `fallback_engines`: `[bing]`.
- `bing_api_key`: absent.
- `bing_api_key_env`: `JCODE_BING_API_KEY`.
- `bing_market`: `en-US`.
- `searxng_url`: absent.
- `searxng_url_env`: `JCODE_SEARXNG_URL`.

The resilience section must remain additive. An absent section must decode
without migration and must preserve this legacy shape and behavior.

### Request and adapter behavior

`WebSearchInput` currently preserves the canonical request fields `query`,
`num_results`, `engine`, and `bing_market`. Results are `SearchResult { title,
url, snippet }`. Result text is headed by `Search results for: <query>` and
renders numbered Markdown entries containing title, URL, and snippet.

Execution currently builds the preferred engine followed by persisted fallback
engines, stable-deduplicates them, and stops on the first non-empty result set.
Bing API credentials are considered only for the first engine position; Bing
fallback execution uses keyless HTML. DuckDuckGo uses the shared reqwest client
and HTML parsing with anti-bot challenge detection. SearXNG uses the configured
or `JCODE_SEARXNG_URL` endpoint and JSON parsing. Existing errors, ToolOutput
construction, and event delivery must remain compatible in the legacy branch.

`ToolOutput` currently has the public fields `output`, `title`, `metadata`, and
`images`, with `new`, `with_title`, and `with_metadata` builders. No new event
or output contract is required by this baseline.

### Existing TUI behavior

The current TUI tool summary renders a websearch query, intentionally exposing
that query in the existing tool-call presentation. The resilient feature must
use a targeted result-title preference for selected-engine status and must keep
query, credentials, response bodies, and private endpoints out of resilient
status text and metadata. Existing tool-card rendering and transcript behavior
remain the compatibility surface.

## Frozen acceptance evidence map

The following mapping is copied from the accepted spec/plan/tasks artifacts and
must remain unchanged. Each check receives its evidence at the corresponding
`validation.md#OW6-*` anchor as implementation and validation tasks complete.

| Frozen check | Task IDs | Evidence anchor |
|---|---|---|
| OW6-001 | T001, T002, T009, T011, T023 | `validation.md#OW6-001` |
| OW6-002 | T003, T006, T007, T012, T013, T023 | `validation.md#OW6-002` |
| OW6-003 | T004, T007, T009, T012, T013, T022, T023 | `validation.md#OW6-003` |
| OW6-004 | T004, T008, T010, T011, T023 | `validation.md#OW6-004` |
| OW6-005 | T008, T010, T018, T020, T023 | `validation.md#OW6-005` |
| OW6-006 | T004, T007, T010, T023 | `validation.md#OW6-006` |
| OW6-007 | T014, T016, T023 | `validation.md#OW6-007` |
| OW6-008 | T015, T017, T023 | `validation.md#OW6-008` |
| OW6-009 | T011, T016, T022, T023 | `validation.md#OW6-009` |
| OW6-010 | T019, T020, T021, T024 | `validation.md#OW6-010` |
| OW6-011 | T022, T024, T025, T026, T027 | `validation.md#OW6-011` |

## Explicit mode and configuration boundary

The accepted target requires the resilient master switch to be absent/false by
default. False or absent resilience must execute the exact existing legacy path.
When explicitly enabled, fallback, retry, health suppression, and diagnostics
subcontrols default enabled with bounded values: 10,000 ms timeout, one retry,
threshold two, and 30,000 ms cooldown. Personal user configuration must not be
modified before T027, and only after active-build verification and the required
post-integration sequence.
