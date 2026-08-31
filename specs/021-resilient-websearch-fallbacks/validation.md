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

## Interim implementation evidence

- T002/T005 config contracts: `cargo test -p jcode-config-types websearch_resilience_contract_tests --lib` passed 5 tests, covering opt-in defaults, legacy aliases/decoding, bounds, explicit master round-trip, and non-secret request policy rejection.
- T003/T006 policy resolution: `cargo test -p jcode-base websearch_policy_precedence_tests --lib` passed 4 tests, covering request > environment > persisted/default resolution, value-free invalid-environment fallthrough, invalid request rejection, and missing-engine eligibility defaults.
- T004/T007/T008/T010 orchestration: `cargo test -p jcode-app-core orchestration::tests --lib` passed 14 tests, including stable order, deduplication, disabled/unavailable/suppressed skips, usable-result stop, retry bounds, health threshold/cooldown/recovery, aggregate exhaustion, partial results, and 100-run determinism.
- T009/T011 adapter integration: `cargo test -p jcode-app-core tool::websearch::tests --lib` passed 29 focused tests, including existing parser/alias fixtures, typed HTTP classification, request compatibility, trust validation, and a local trusted SearXNG TCP fixture using the shared client.
- T014/T016 diagnostics: the orchestration suite includes bounded metadata schema, clean-success trigger behavior, compact presentation, and negative privacy assertions. Metadata contains no query, credential, response body, or endpoint URL.
- T015/T017 TUI: `cargo test -p jcode-tui tui::ui::tests::tools::test_websearch --lib` passed 2 tests covering selected-engine title preference, deterministic one-line rendering, width cap, and query suppression in the status row.
- T019/T021 documentation: `cargo test -p jcode-base config::default_file::tests::default_config_template_documents_resilient_websearch_controls --lib` and `...::default_config_template_parses --lib` both passed. `docs/WEBSEARCH.md` documents both modes, precedence, bounds, outcomes, trust, privacy, and rollback boundaries.

## Benchmark receipt

The ignored acceptance benchmark was run with:

```text
cargo test -p jcode-app-core orchestration::tests::orchestration_bookkeeping_stays_within_latency_budget --lib -- --ignored --nocapture
```

Receipt: 1 test passed, 100 warm-up iterations, 1,000 pure orchestration
samples, median `2 us`, p95 `3 us`. The benchmark performs no HTTP work, no
sleep, and no client construction. It passed the target median `< 1 ms` and
p95 `< 2 ms` thresholds. For the bounded network formula, the implementation
uses `E <= 3`, `R = max_retries`, at most `E * (1 + R)` physical attempts, and
per-attempt timeout plus fixed 200 ms retry delay.

## OW6-001

Baseline evidence is recorded above. The resilient master switch is false by
default and branches before legacy execution. Existing legacy parser and alias
fixtures remain green. Final full focused legacy regression evidence will be
added by T023.

## OW6-002

The typed resolver and orchestration tests pass the request > environment >
persisted > default matrix, the special fallback-order source chain, preferred
prepend, stable first-occurrence deduplication, and no duplicate physical
attempts. Final full matrix receipt will be added by T023.

## OW6-003

The typed orchestration records disabled, unavailable, and health-suppressed
engines as skipped without calling the backend. SearXNG trust validation allows
HTTPS and loopback HTTP only, rejects userinfo/untrusted HTTP, and the local TCP
fixture proves a trusted loopback path. Final public-path receipt will be added
by T022/T023.

## OW6-004

The deterministic suite covers challenge, empty, partial, transient, timeout,
permanent, stop-on-first-usable, and aggregate exhaustion behavior. Final
complete matrix receipt will be added by T023.

## OW6-005

The retry-bound tests and benchmark receipt above prove narrow retry classes,
configured retry limits, fixed delay, finite engine count, and pure bookkeeping
budget. Final timing/attempt evidence will be added by T023.

## OW6-006

Explicit-time health tests prove one increment per terminal transient sequence,
threshold suppression, inclusive cooldown expiry, clear-before-attempt, success
recovery, and per-engine isolation. Final complete matrix receipt will be added
by T023.

## OW6-007

Diagnostic metadata and presentation tests pass separately. Metadata is emitted
for diagnostics-enabled clean success without extra text, while fallback,
retry, suppression, and aggregate failure receive one bounded summary. Disabled
diagnostics omit optional metadata and detail. Final complete matrix receipt
will be added by T023.

## OW6-008

The focused TUI tests pass selected-engine title preference, deterministic
one-line fallback/exhaustion summaries, display-width capping, and absence of
fixture queries from the status row. Final complete matrix receipt will be added
by T023.

## OW6-009

Handled aggregate exhaustion returns `ToolOutput`; only adapter infrastructure
errors are normalized/handled by the resilient path, and legacy output/event
contracts remain unchanged. Final public-path receipt will be added by
T022/T023.

## OW6-010

Documentation/template and orchestration benchmark evidence is complete. The
free-only engine set, legacy credentialed Bing path, resilient keyless Bing path,
trusted SearXNG conditions, and privacy rules are documented. Guardrail receipt
remains pending T024.

## OW6-011

No personal user configuration has been modified. Active-build verification and
post-integration enablement remain intentionally pending T024/T025/T026/T027.

## T022 isolated built-binary public-path smoke

- Candidate executable: `/home/ari/repos/jcode/.worktrees/agent/jcode-ow6-resilient-websearch/target/selfdev/jcode`, resolved with `readlink -f` to the same regular file.
- Candidate identity before the smoke: `jcode v0.81.715-dev (d8d0b184f, dirty)`, selfdev profile. The dirty state contains only the owned resilient-websearch implementation and evidence files.
- Isolation: a temporary `JCODE_HOME`, temporary minimal config, copied OAuth credential file removed by the cleanup trap, unique `/run/user/<uid>/jcode-ow6-smoke.sock`, and a loopback-only SearXNG fixture on an ephemeral port. Personal configuration was neither read nor modified.
- Effective non-secret controls: resilience and diagnostics enabled; DuckDuckGo and Bing disabled; SearXNG enabled; fallback order `searxng`; trusted endpoint restricted to the loopback fixture.
- Public command shape: `target/selfdev/jcode run --no-update --no-selfdev --socket <unique-socket> --provider openai --model gpt-5.6-sol --reasoning-effort medium --tools websearch --max-tool-steps 1 --ndjson --trace <fixture-only prompt>` with the `JCODE_WEBSEARCH_*` controls above.
- Result: PASS in 5.72 seconds. The fixture received exactly one request, `/search?q=<redacted>&format=json`; NDJSON contained `OW6 fixture result` and the bounded summary `fallback selected searxng`. No external search engine was contacted.
- Cleanup: the fixture process was terminated, the private daemon was stopped, the unique socket and temporary OAuth copy were removed, and only redacted smoke artifacts remain under `$JCODE_SCRATCH_DIR/jcode-ow6-smoke`.
- Final modular-candidate repeat: PASS in 7.54 seconds with the same candidate identity shape, exactly one fixture request, the expected result, and `fallback selected searxng`; artifacts are under `$JCODE_SCRATCH_DIR/jcode-ow6-smoke-final`.

## T023 complete focused acceptance matrix

Independent post-controller validation passed:

- `scripts/dev_cargo.sh test -p jcode-app-core websearch --lib`: 31 passed, 0 failed, 1 ignored. This includes the legacy adapters/parsers/aliases, exact disabled legacy branch, typed outcomes, bounded retry/fallback, timeout handling, health recovery/isolation, aggregate `ToolOutput`, diagnostics/privacy, trusted local SearXNG fixture, and the deterministic 100-run decision proof.
- `scripts/dev_cargo.sh test -p jcode-config-types websearch_resilience_contract_tests --lib`: 5 passed.
- `scripts/dev_cargo.sh test -p jcode-base websearch_policy_precedence_tests --lib`: 4 passed.
- `scripts/dev_cargo.sh test -p jcode-tui websearch_tools --lib`: 2 passed.
- Ignored benchmark: 1 passed over 1,000 samples, median 6 us and p95 6 us, below the accepted median 1 ms and p95 2 ms limits.

The deterministic suite records identical decisions across 100 runs without live
search. Diagnostics-enabled clean success emits structured metadata without an
extra status line; meaningful fallback/retry/suppression emits one bounded line;
aggregate failure emits one actionable line; diagnostics-disabled output omits
optional diagnostic metadata and detail. Contract privacy and presentation
privacy are asserted separately.

The finite-work bound is `E <= 3`, physical attempts `<= E * (1 + R)`, and elapsed
upper bound `E * (1 + R) * attempt_timeout + E * R * 200 ms`, plus scheduler
tolerance. Handled timeout exhaustion returns aggregate `ToolOutput`; only
infrastructure/runtime failures escape as errors. The frozen OW6-001..OW6-011
mapping remains unchanged and T023-T027 remain US-004 tasks.

## T024 formatting, affected builds, Clippy, and guardrails

Feature-owned validation after the modular size-ratchet repair:

- `scripts/dev_cargo.sh fmt --all -- --check`: PASS.
- Affected focused tests: config-types 5 passed, base policy 4 passed, app-core websearch 31 passed with the benchmark ignored, and TUI websearch rendering 2 passed.
- `scripts/dev_cargo.sh clippy -p jcode-config-types -p jcode-base -p jcode-app-core -p jcode-tui --lib`: exit 0. It reported only four pre-existing warnings outside feature-owned files: one in `jcode-base/src/auth/lifecycle.rs` and three existing TUI animation/image warnings.
- Candidate selfdev build: PASS. The initial feature-owned unused import warning was removed before final validation.
- Final post-modularization isolated public-path smoke: PASS in 7.5 seconds. The loopback fixture received exactly one request; output contained the fixture result and fallback summary. The private socket and temporary home prevented external search or personal-config access.
- `scripts/check_guardrails.sh --skip-slow`: module resolution, format, locked metadata, dependency boundaries, wildcard re-exports, and onboarding state-space invariants passed. The aggregate script remained non-zero because of existing repository-wide warning-budget, provenance, size, panic, and swallowed-error ratchet drift in unrelated files.
- The initial guardrail run correctly identified feature growth in large files. The implementation was then modularized into focused `websearch.rs` config/policy modules, adapter/orchestration test modules, and a dedicated TUI result-title helper/test module. Direct reruns of code-size, test-size, panic, and swallowed-error ratchets show no feature-owned findings.
- No `Cargo.toml`, `Cargo.lock`, or crate manifest changed. No external crate, paid service, browser dependency, second websearch tool, incompatible event contract, or unrelated TUI redesign was introduced.
- The first guardrail attempt was blocked at 9.7 GiB free by the 10 GiB disk reserve. Removing only the feature worktree's reproducible 17 GiB `target/` directory restored 19 GiB free; the supported guardrail suite then ran normally. This cleanup did not remove source or user data.

The remaining aggregate guardrail failures are recorded as pre-existing repository
state rather than hidden or rebaselined. Feature-attributable focused checks are
green.

## T025 integration handoff

- Feature commit: `d66b21771 feat(websearch): add resilient free-search fallbacks`.
- The feature worktree was clean immediately after the commit, and the root
  integration checkout remained on `dev`.
- The handoff includes focused config, orchestration, adapter, TUI, benchmark,
  isolated built-binary smoke, dependency-boundary, and guardrail evidence above.
- Personal configuration remains unchanged pending the landed-build identity
  verification required by T026.
