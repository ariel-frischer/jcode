# Validation

## Acceptance evidence

- Quoted and plain relative Markdown paths remain clickable and render inline.
- Relative paths resolve only against the session working directory.
- Missing local paths remain unavailable even when an identical path exists in a parent or sibling directory.
- Parent-relative, home-relative, absolute, HTML-mode, collapse, copy, scroll, missing, oversized, binary, and line/column-suffix behavior remains covered.
- Preview resolution and bounded reads run in background work. Exact `(message index, stable hash)` ownership prevents duplicate-message collisions, and stale results are discarded after transcript changes.

## Commands

- `cargo test -p jcode-tui inline_file_preview` passed: 9 tests.
- Exact session-CWD, no-sibling-search, suffix, duplicate-owner, and stale-result regression tests passed.
- `cargo check -p jcode-tui` passed.
- `cargo clippy -p jcode-tui --lib -- -D warnings` passed.
- `scripts/check_code_size_budget.py` passed and improved affected oversized modules.
- `scripts/check_swallowed_error_budget.py` passed and removed preview-module swallowed-error growth.
- `scripts/dev_cargo.sh build --profile selfdev -p jcode --bin jcode` passed.
- Built binary identified as `jcode v0.79.679-dev (989be0346, dirty)` before the implementation commit.
- A real TUI launched from `/home/ari/repos/locus` reports `~/repos/locus` as its working directory, so `docs/locus-cloud-architecture.md` must resolve directly from that session CWD.

## Known unrelated gate

`scripts/check_guardrails.sh` passes format, all-target/all-feature check, lockfile, warning budget, provenance, code-size, test-size, panic, swallowed-error, dependency, wildcard-export, desktop frame, and onboarding gates. Its only failure is a pre-existing `clippy::derivable_impls` error in `crates/jcode-config-types/src/lib.rs` for `FileMentionsConfig`, a file untouched by this change.
