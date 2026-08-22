# Proposal: Responsive `@` file mentions and context expansion

> Draft only. Do not post or link publicly without Ariel's explicit approval.

## Summary

Add a compact `@` file mention workflow to the TUI composer. Typing `@` discovers paths beneath the session working directory in bounded background batches. Selecting a result replaces only the active mention, and submitting an existing readable text path expands its contents into the model-facing message while retaining the compact `@path` in the visible transcript.

## Motivation

Users currently need to type repository paths manually. Existing upstream proposals demonstrate demand, but their large persistent indexes and frecency subsystems require broader UX and architecture decisions. This branch intentionally proposes a smaller implementation:

- no persistent index or frecency database
- no new popup subsystem
- no file-chip state model
- no rendered-message preview behavior
- bounded, cancellable discovery using existing composer suggestion infrastructure

## Minimal feature set

- `@` suggestions rooted at the active session working directory, with launch-CWD fallback.
- Built-in ignores for dependencies, generated output, caches, and VCS metadata.
- Additive global `file_mentions.ignore` configuration.
- `file_mentions.enabled = false` disables both discovery and submit-time expansion.
- Background batches keep input responsive and discard stale query generations.
- Root-level files are prioritized, with fuzzy ordering for the remaining paths.
- Tab completes the active mention without submitting and repeated Tab cycles suggestions.
- Submission expands readable UTF-8 files relative to the session working directory.
- Missing, binary, embedded-email-like, or oversized paths remain literal.

## Configuration

```toml
[file_mentions]
enabled = true
ignore = ["private/", "*.generated.*"]
```

Custom patterns are additive to the built-in list.

## Proven fork source

The branch ports the working integrated fork commits, then removes the later named-profile coupling so this proposal remains independently reviewable:

- `7da479dfe31d49cb28e87cebfcf3b70244dadb21` - initial CWD-rooted picker
- `c520c223ed73cc453123be78ec9f60ec4de8ab6e` - preserve built-in ignores
- `2636a4fe3e3c54210f7d64fbd72f01298e36d211` - initial acceptance tests
- `8b66a3c0c13a3241070fc6cd458a0102a99b7f82` - configurable ignores
- `a423db8fce9667679ebd53385b03579473d7a212` - bounded background discovery
- `c65dde06e0679aa4197038fe32245ae94725276c` - model-context expansion
- `16fc0290c6b5b7522a11acfd20ab54412a287a71` - completion and path prioritization

Primary paths:

- `crates/jcode-config-types/src/lib.rs`
- `crates/jcode-base/src/config.rs`
- `crates/jcode-base/src/config/default_file.rs`
- `crates/jcode-tui/src/tui/app/state_ui_input_helpers.rs`
- `crates/jcode-tui/src/tui/app/input.rs`
- `crates/jcode-tui/src/tui/app/remote/input_dispatch.rs`
- `crates/jcode-tui/src/tui/app/tests/file_mention_picker.rs`
- `specs/010-file-mention-picker/spec.yaml`

## Upstream base and branch

- Base: `upstream/master@f28f02f8d3fb004494b4a6886617bae9ee8b216b`
- Local branch: `agent/jcode-zn0.2`
- Implementation commit: `9dd27ba` (repair/isolation tip; preceding commits preserve source provenance)
- Public branch link: `[add only after Ariel authorizes publication]`

## Upstream overlap and duplicate risk

- [#570 feat(tui): file selection mentions with frecency ranking](https://github.com/1jehuang/jcode/issues/570)
- [#837 Feature: @file completion with frecency ranking in the TUI](https://github.com/1jehuang/jcode/issues/837)
- [#477 feat: @file completion system](https://github.com/1jehuang/jcode/issues/477) (closed)

Maintainer feedback on #570 and #837 highlights project boundaries, indexing cost, fuzzy reuse, and persistent frecency as architecture decisions. This branch avoids those disputed components. It uses bounded transient discovery, existing fuzzy scoring, the active session working directory, and no new persistent state.

## Compatibility and safety

- Omitted configuration preserves enabled behavior with safe built-in ignores.
- Disabling the feature leaves ordinary `@` input untouched.
- Discovery is capped and cancellable; stale results cannot replace the current query.
- Expansion accepts only bounded readable UTF-8 regular files.
- Paths resolve against session CWD, not an arbitrary process-global directory when session metadata exists.
- Remote composer submission uses the same expansion contract as local submission.
- Profile-specific ignore policy is deliberately excluded and belongs to the separate named-profile contribution.

## Validation matrix

| Requirement | Direct check | Status |
|---|---|---|
| Session-CWD discovery and ignore behavior | `at_file_suggestions_use_session_cwd_ignore_vendor_content_and_accept_selection` | Passed |
| Launch-CWD fallback | `file_mention_discovery_falls_back_to_the_launch_cwd` | Passed |
| Root-file prioritization | `file_mention_discovery_prioritizes_files_directly_in_the_root` | Passed |
| Enable/disable and no scan | `file_mentions_default_enabled_and_can_be_disabled_without_scanning` | Passed |
| Tab completion and cycling | `tab_completes_an_active_file_mention_without_submitting`; `repeated_tab_cycles_through_file_mention_suggestions` | Passed |
| Stale query cancellation | `stale_file_mention_generations_are_discarded` | Passed |
| Batching and input latency | `file_mention_discovery_is_batched_and_input_stays_within_budget` | Passed: first batch under 1.3 ms and synthetic input under 0.13 ms at up to 1,024 files on the validation host |
| Context expansion | `submitted_file_mention_keeps_compact_display_and_expands_model_context`; input expansion tests | Passed |
| Missing/binary/embedded `@` safety | input expansion tests | Passed |
| Global custom ignores | `file_mentions_skip_vendor_directories_and_honor_custom_patterns` | Passed |
| Changed-package formatting | `cargo fmt -p jcode-tui -p jcode-base -p jcode-config-types -- --check` | Passed |
| Strict Clippy | `cargo clippy -p jcode-tui --all-targets --all-features -- -D warnings` | Feature code compiles; blocked by unchanged upstream `jcode-render-core/src/markdown.rs:426` collapsible-match finding |
| Built TUI public interaction | exact worktree binary in private runtime, real TUI keystrokes | Passed: `@a` displayed `@alpha.txt`, Tab completed it; disabled config preserved literal `@a` with no suggestion |
| Workspace guardrails | `scripts/check_guardrails.sh` | Mixed: module resolution, all-target/all-feature check, lockfile, dependency boundaries, wildcard re-exports, and desktop2 frame budget passed; unchanged upstream formatting/Clippy plus stale ratchets blocked aggregate success |

The first attempted Cargo filter used the included filename rather than actual test names and therefore matched zero tests. The exact compiled test harness was then used to run all 13 mapped tests successfully. After extracting the feature from oversized monoliths, focused `file_mention`, `tab_completes`, and `repeated_tab` acceptance filters passed again. The repaired branch adds no swallowed-error patterns in changed lines; `state_ui_input_helpers.rs` is 49 lines smaller than upstream and the central test module is unchanged in size.

## Edge cases

- Empty query and partial directories.
- Root-level versus nested results.
- Large workspaces with results arriving in batches.
- Query changes while an earlier discovery worker is still active.
- Disabled picker with literal existing paths.
- Paths at the start, middle, and end of input.
- Missing, binary, oversized, and email-like `@` text.
- Local and remote submission paths.

## Proposed acceptance criteria

1. Typing `@` in an enabled TUI session produces bounded, workspace-rooted suggestions without blocking input.
2. Disabling `file_mentions.enabled` prevents discovery and submit-time expansion.
3. Built-in and configured ignore patterns are honored.
4. Selecting or cycling suggestions replaces only the active mention span.
5. Submitting an existing readable text path preserves compact transcript text and supplies bounded file contents to the model.
6. Missing, binary, oversized, and non-path `@` text remain safe and literal.
7. Local and remote input paths behave consistently.
8. Focused tests, strict changed-package checks, a built-TUI acceptance path, and workspace guardrails pass or document unrelated upstream blockers.

## Review questions

- Is bounded transient discovery preferable as a minimal first contribution while frecency and persistent indexing remain under maintainer decision?
- Should the first version remain limited to global ignore configuration, leaving profile-specific ignores to the named-profile feature?
- Is compact transcript text plus expanded model context the preferred user-visible contract?
