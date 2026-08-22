# Draft upstream issue: Click rendered file paths to preview them inline

> **Draft only. Do not post without Ariel's explicit approval.**
>
> Proposed branch link: `<fork branch link after approval>`

## Summary

Make repository-relative paths, home-relative paths, absolute paths, and `@path` references in rendered chat messages clickable. Clicking a readable text file opens a bounded inline preview beneath that message. Clicking the visible preview body again collapses it.

This is intentionally narrower than composer autocomplete. It does not add an `@` picker, file indexing, fuzzy search, frecency, chips, or context injection.

## Problem

Jcode already recognizes and opens URLs in rendered messages, but rendered file references are inert. When an assistant names `src/main.rs`, `docs/guide.md`, `~/notes.md`, or `@README.md`, users must manually copy the path and open the file elsewhere. That interrupts review and is especially awkward in terminal environments where text selection or external file delivery is unreliable.

A lightweight inline preview keeps the review inside the transcript and reuses the existing mouse hit-testing, selection, scrolling, message hashing, and rendering paths.

## Proposed minimal behavior

1. Recognize file-like targets in rendered chat text:
   - repository-relative paths such as `src/main.rs` and `README.md`
   - `./` and `../` paths
   - absolute paths
   - home-relative `~/...` paths
   - `@path` references, including references inside backticks, parentheses, brackets, braces, and quotes
2. Resolve relative paths from the session working directory, falling back to the process working directory when the session has no working directory.
3. On a left-click, open a readable text file in a bounded inline preview attached to the clicked message.
4. Preserve ordinary URL and Markdown-link opening behavior.
5. Let a click on the visible preview body collapse it, while preserving drag selection for copying.
6. Render Markdown files as Markdown and ordinary text with the normal foreground color so both light and dark terminal themes remain readable.
7. Reject directories, missing paths, files over 512 KiB, and non-UTF-8/binary content without panicking or opening an external program.

## Non-goals

- Composer `@file` completion, fuzzy search, indexing, frecency, or file chips
- Expanding `@path` into model context
- Editing files from the preview
- Previewing binary formats or directories
- Changing URL click handling or external browser policy
- Adding new configuration or protocol fields

## Upstream base and working implementation

Port base:

- `upstream/master`
- commit `a63dbc4546895ecb4d1be1a285d98e6e13fb1b74`

The implementation was ported from the proven fork behavior rather than rewritten. Relevant fork commits, in dependency order:

- `af2e205d6` `feat(tui): add inline file previews`
- `cf202de91` `feat(tui): collapse inline file previews on click`
- `c2e1c9278` `refactor(tui): isolate inline preview hit testing`
- `426f92f7d` `fix(tui): open relative inline files without session cwd`
- `1c2c94d46` `test(tui): cover relative file click fallback`
- `491157e78` `feat(tui): open file mentions inline`
- `6a9a33ea7` `fix(tui): detect mentions in inline markup`
- `2e6246a30` `fix(tui): expand home paths in inline previews`

Autospec/spec artifacts from the fork were deliberately excluded. The contribution contains only the minimal code and focused tests needed on the upstream base.

## Files changed

The code-only dependency closure is limited to `jcode-tui`:

- `crates/jcode-tui/src/tui/app.rs`
- `crates/jcode-tui/src/tui/app/inline_file_preview.rs`
- `crates/jcode-tui/src/tui/app/navigation.rs`
- `crates/jcode-tui/src/tui/app/tests.rs`
- `crates/jcode-tui/src/tui/app/tests/inline_file_preview.rs`
- `crates/jcode-tui/src/tui/app/tests/scroll_copy_03/mod.rs`
- `crates/jcode-tui/src/tui/app/tui_lifecycle.rs`
- `crates/jcode-tui/src/tui/app/tui_state.rs`
- `crates/jcode-tui/src/tui/mod.rs`
- `crates/jcode-tui/src/tui/ui.rs`
- `crates/jcode-tui/src/tui/ui/copy_selection.rs`
- `crates/jcode-tui/src/tui/ui/url.rs`
- `crates/jcode-tui/src/tui/ui_inline_file_preview.rs`
- `crates/jcode-tui/src/tui/ui_prepare.rs`
- `crates/jcode-tui/src/tui/ui_tests/basic/body_cache.rs`

## Implementation notes

- Link hit-testing first preserves Markdown links and URLs, then recognizes `@path` references and plain file-like paths.
- Chat hit-testing returns both the target and the owning message index so preview state remains attached to the correct stable message hash.
- Preview state is stored per message and contributes a version to prepared-body cache keys.
- File reads are bounded to 512 KiB and require UTF-8 text.
- Mouse-up collapse handling runs before normal copy-selection completion, but only collapses when the pointer returns to the original chat anchor. Drag selection remains available.

## Compatibility and edge cases

Covered behavior includes:

- URLs and Markdown links remain actionable.
- Invalid `@path` clicks are consumed locally with a status notice instead of being passed to an external opener.
- Missing paths and directories do not create previews.
- Relative paths work with a session working directory and with the process-CWD fallback.
- `~/...` expands through the platform home directory.
- Wide Unicode prefixes use display-column width for correct hit targets.
- Inline markup delimiters are excluded from the clickable target.
- Oversized and binary/non-UTF-8 files fail safely.
- Expanded previews participate in transcript scrolling.
- Clicking the preview body collapses it, while dragging over it preserves copy selection.
- Empty and ordinary text files use normal foreground rendering for light/dark readability.

## Related upstream issues and duplicate analysis

Research performed against the upstream repository on 2026-08-22:

- [#622](https://github.com/1jehuang/jcode/issues/622) describes broader terminal interaction, link handling, and file-delivery friction. This proposal is a focused response for rendered local file references.
- [#477](https://github.com/1jehuang/jcode/issues/477), [#570](https://github.com/1jehuang/jcode/issues/570), [#837](https://github.com/1jehuang/jcode/issues/837), and [#934](https://github.com/1jehuang/jcode/issues/934) concern composer-side `@file` completion or selection. That is deliberately out of scope here.
- Searches for `inline file`, `file preview`, `clickable file`, and `file mention` found no exact upstream issue or pull request implementing click-to-preview for file references already present in rendered transcript messages.

The main overlap risk is product-level confusion with composer autocomplete. The issue and branch should continue to state that the feature acts only on already-rendered chat content.

## Validation evidence

Test-first evidence on the actual upstream-based worktree:

1. The focused tests were applied before production code.
2. `./scripts/dev_cargo.sh test -p jcode-tui inline_file_preview --lib` failed as expected in the `.3` worktree because upstream lacked `inline_file_previews` state and `try_toggle_inline_file_preview`. Compiler paths resolved under `.worktrees/agent/jcode-zn0.3`.
3. After porting the code-only closure, all focused filters were rerun with `JCODE_IN_DEV_CARGO=1` and passed:
   - `inline_file_preview`: 4 passed
   - `process_working_directory`: 2 passed
   - `clicking_file_mentions`: 1 passed
   - `invalid_file_mention`: 1 passed
   - `visible_inline_file_body`: 1 passed
   - `dragging_over_inline_file_body`: 1 passed
   - `link_target_for_display_column`: 5 passed
   - `file_mention`: 5 passed
4. `JCODE_IN_DEV_CARGO=1 cargo fmt -p jcode-tui -- --check` passed.
5. `JCODE_IN_DEV_CARGO=1 ./scripts/dev_cargo.sh clippy -p jcode-tui --lib --no-deps` passed with no warnings in changed files.
6. `./scripts/dev_cargo.sh build --profile selfdev -p jcode --bin jcode` passed.
7. The clean pre-runtime binary reported `jcode v0.79.15-dev (96d756d9a)` and accepted `--help`.
8. Direct end-user acceptance used that exact binary on a private daemon/socket and owned 120x40 PTY. A real SGR terminal mouse click on rendered `README.md` opened the inline preview and displayed `Expanded file: README.md`; a second real mouse click inside the visible preview body restored the transcript and displayed `Collapsed inline file preview`.
9. Runtime cleanup verified the private client, PTY driver, daemon, and socket were all absent; the shared daemon was untouched.

Known unrelated upstream-base validation noise:

- Whole-workspace `cargo fmt --all -- --check` reports formatting drift in `crates/jcode-app-core/src/tool/bash.rs` and `bash_tests.rs`, which this branch does not modify.
- `clippy ... -D warnings` is blocked by pre-existing warnings in `ui_messages.rs`, `app/helpers.rs`, `auth_account_picker_saved_accounts.rs`, `session_picker/loading_tests.rs`, and `ui_tests/palette_topology.rs`. No reported warning points at the changed implementation files.

No successful live provider turn, upstream push, issue creation, pull request, or GitHub comment was performed. One unrelated isolated onboarding submission failed immediately with `credit_balance_exhausted` before producing a response; the direct mouse acceptance used no provider.

## Proposed acceptance criteria

- [x] Clicking a valid repository-relative, absolute, `~/...`, or `@path` text-file reference in rendered chat opens an inline preview attached to the correct message.
- [x] Relative paths resolve from the session CWD, with process-CWD fallback when session CWD is unavailable.
- [x] URL and Markdown-link behavior remains unchanged.
- [x] Invalid, missing, directory, oversized, and non-text targets fail safely and do not invoke an external opener.
- [x] Preview rendering remains readable in light and dark terminal themes.
- [x] Clicking the visible preview body collapses it; dragging still supports text selection.
- [x] Focused tests, `jcode-tui` formatting, a built `jcode` binary, and exact-binary PTY acceptance pass on the upstream base.
