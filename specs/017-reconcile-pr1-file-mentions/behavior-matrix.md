# PR 1 file-mention reconciliation matrix

Bead: `jcode-5kb`  
Candidate: fork PR 1 at `49738a4d508c69a49aeaff00928db32d07609ecc`  
Authoritative baseline: current `dev` at worktree creation (`5c4ded944`)  
Alignment: 2026-08-25 multi-Bead contract. PR 2 inline previews are excluded and owned by `jcode-2hl`.

## Revision evidence

Both lines descend from merge base `993da322ebf438e21702a0cec9d9399f9e7d8307` but neither tip is an ancestor of the other. Current `dev` contains behaviorally corresponding initial file-mention commits (`7da479dfe`, `c520c223e`, `8b66a3c0c`, `a423db8fc`, `c65dde06e`) and later custom inline-opening work. PR 1 adds upstream-review hardening commits including explicit opt-in (`fae13dc6e`), opt-in test fixtures (`a51a5a590`), observable scan failures (`55c9ad88a`), stabilized context (`52542714c`), and rewind projection assertions (`49738a4d5`).

## Behavior matrix

| Scope | PR 1 behavior | Current `dev` behavior | Risk / compatibility | Disposition | Direct evidence / validation |
|---|---|---|---|---|---|
| Default and unset configuration | `FileMentionsConfig::default().enabled == false`; generated config and PR docs require explicit opt-in. | `enabled == true`; README and generated config say enabled by default. | Default-on changes ordinary `@` input into filesystem scanning and provider-bound file expansion. It weakens additive/unset compatibility and is the only clear PR-quality improvement not present in `dev`. | **PORT** | PR: `crates/jcode-config-types/src/file_mentions.rs`, commit `fae13dc6e`. Dev: `crates/jcode-config-types/src/lib.rs:16-33`, `README.md:851-869`, `crates/jcode-base/src/config/default_file.rs:111-116`. Update tests and run exact file-mention filters. |
| Explicit enable / disable | Explicit `enabled = true` enables picker and expansion; false leaves `@` literal and performs no discovery. | Same feature gate exists, but tests assume default-on for discovery. | Tests must opt in without leaking global config between parallel cases. | **PORT test adaptation / PRESERVE runtime** | Port the scoped `with_file_mentions_enabled` pattern from `a51a5a590`; retain disabled-path assertions. |
| Built-in and custom ignores | Built-in generated/dependency ignores remain active; configured ignores are additive. | Same behavior, plus profile-specific additive ignore patterns. | Wholesale PR port would regress profile-specific custom behavior. | **PRESERVE** | Current `README.md:857-869`, profile config and picker tests. Focused file-mention test filter. |
| Responsive asynchronous discovery | Background worker sends bounded batches with generations and cancellation. | Same architecture, split into `state_ui_input_helpers/file_mentions.rs`; current `dev` also distinguishes completed workers. | PR-head completion handling can interpret normal channel closure poorly. Replacing current code would regress observable lifecycle behavior. | **REJECT PR implementation / PRESERVE dev** | Current `FileMentionDiscovery.completed`, tests `disconnected_file_mention_worker...` and `completed_file_mention_worker...`. |
| Stale generation rejection | New queries cancel or supersede old generation results. | Same, with current tests. | Stale batches can corrupt the picker if generation ownership drifts. | **PRESERVE** | `stale_file_mention_generations_are_discarded`; focused tests. |
| Compact transcript and final provider expansion | Queue/history retain `@path`; provider receives escaped `<file>` content at final dispatch. | Same, with materialized provider-message caching and broader custom session behavior. | Expanded contents must never persist in user-visible history or leak through rewind/resume. | **PRESERVE** | Current `conversation_state.rs`, `input/file_mentions.rs`, compact/provider tests and rewind tests. |
| Submission size restoration | Expanded payload is checked against the normal 3 MiB submit limit; failures restore editable/queued state. | Same, with explicit queued/interleave restoration helpers. | A failed expansion must not lose typed text, queued reminders, images, or processing state. | **PRESERVE** | Current `expand_file_mentions_for_submit`, `restore_queued_file_mention_failure`, queued failure tests. |
| Working-directory resolution | PR uses connecting-client cwd for remote discovery/expansion. | Current helper prefers process cwd for remote and session cwd otherwise. | Remote authority must remain client-side and deterministic. | **PRESERVE pending focused remote evidence** | `file_mention_working_dir`; queued remote tests and isolated runtime. |
| Repository containment and unsafe paths | PR head resolves relative or absolute candidates but does not consistently enforce canonical cwd containment in the provider-expansion implementation reviewed. | Current `dev` canonicalizes working root and candidate, then requires `resolved.starts_with(&working_dir)`; missing, binary, oversized, outside-root, or unreadable paths stay literal. | Absolute, `..`, and symlink escapes can transmit unintended local files. | **REJECT PR implementation / PRESERVE safer dev** | Current `input/file_mentions.rs:17-79`; unsafe path tests. |
| Wrapper escaping | Escapes path attributes and literal `</file>` terminators. | Same or stricter current behavior. | Wrapper escaping prevents structural breakout but not semantic prompt injection in file contents. | **PRESERVE** | Input unit tests for escaped closing tags and path handling. |
| Rewind/provider projection assertion | PR tip updates rewind expectations to compare projected provider messages correctly. | Current test organization differs after later custom evolution; provider projection remains broader. | Copying assertions mechanically could test a stale representation. | **NO-OP unless focused test exposes gap** | Run state-model/rewind filters and compare semantics, not line shape. |
| Module extraction / file moves | PR splits large TUI modules to satisfy upstream-review size budgets. | Current `dev` independently split and evolved modules along different custom ownership seams. | Copying file moves would create broad conflicts and duplicate architecture. | **REJECT** | Changed-file comparison and existing module ownership. |

## Selected surgical change

Only the default/unset behavior is selected for a product change:

1. Change `FileMentionsConfig::default().enabled` from `true` to `false`.
2. Change the generated config template to document opt-in and emit `enabled = false`.
3. Update README language to opt-in.
4. Adapt discovery/picker tests to explicitly enable file mentions, while asserting absent config is disabled and explicit `enabled = true` works.

No PR 1 runtime module is copied. Current `dev` wins for canonical containment, completed-worker lifecycle, profile-specific ignore patterns, queued/interleave restoration, remote/custom session behavior, and later inline-opening behavior.

## PR 2 exclusion checklist

- Do not edit `app/inline_file_preview.rs`, `ui_inline_file_preview.rs`, preview rendering, link hit-testing, sibling-repository lookup, or preview state keys.
- Do not add preview dependencies, fixtures, or user behavior.
- Final diff must contain only config/default documentation, file-mention picker tests, this Autospec directory, and any directly required focused test fix.
- Coordinate with Bead `jcode-2hl`; no integration or merge of PR 2 occurs here.

## Validation record

| Check | Command | Result |
|---|---|---|
| Pre-change file-mention baseline | `scripts/dev_cargo.sh test -p jcode-tui file_mention --lib -- --nocapture` | PASS: 30 passed, 0 failed |
| Post-change focused file mentions | Same command | PASS: 30 passed, 0 failed |
| Config/default focused tests | `scripts/dev_cargo.sh test -p jcode-base default_config_template_parses --lib`; `scripts/dev_cargo.sh test -p jcode-config-types --lib` | PASS: 1 + 22 tests |
| Formatting | `scripts/dev_cargo.sh fmt --all -- --check` | PASS |
| TUI build | `scripts/dev_cargo.sh build --profile selfdev -p jcode --bin jcode` | PASS |
| Guardrails | `scripts/check_guardrails.sh` | PASS: format, all-target/all-feature check, Clippy `-D warnings`, ratchets, boundaries, and state-space gates |
| Private-socket runtime | Newly built binary with isolated `JCODE_HOME` and socket | Visual tester blocked because shared debug control is disabled; exact built binary and default behavior are covered by the passing build and focused TUI tests. |
| PR 2 exclusion / diff | Targeted searches plus worktree diff/status | PASS: only README, file-mention config/default/tests, and this spec directory changed; no preview surface touched. |
