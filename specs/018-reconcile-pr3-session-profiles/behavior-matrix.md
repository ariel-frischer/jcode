# PR 3 session-profile reconciliation matrix

Baseline: current `dev` at `c46c53a36`. Candidate: fork PR 3 tip `781511c6d37412410c6d49c44f663f1ee06b25c8`, including hardening commit `38c082a8a`. Owner: Bead `jcode-anw`.

| Behavior | PR 3 evidence | Current dev evidence | Decision | Risk and direct check |
|---|---|---|---|---|
| Static profile instruction placement | PR 3 final hardening keeps instructions in static prompt order. | Dev already has `profile_prompt_overlay_is_static_ordered_and_exactly_once` and broader prompt composition. | **Preserve dev** | High prompt/cache risk. Run the existing exact-order test. |
| Invocation > environment > profile > base > default | PR 3 explicitly compares selector source precedence. | Dev masks profile fields when environment is active and clears profile fields for explicit invocation, but cross-selector clearing was incomplete. | **Port surgical gap** | High routing risk. Run all `cli::profile::tests` serially. |
| Explicit `--provider` over lower profile `provider_profile` | PR 3 clears the lower named route. | Dev could retain and later activate the lower profile route. | **Port** | High wrong-provider risk. `explicit_provider_clears_lower_precedence_profile_provider_profile`. |
| Explicit `--provider-profile` over lower profile `provider` | PR 3 selects the named route and its provider family. | Dev could retain contradictory lower provider metadata. | **Port** | High wrong-protocol risk. `explicit_provider_profile_selects_its_runtime_and_clears_profile_provider`. |
| Both explicit selectors at the same precedence | PR 3 enforces mutual exclusion before activation and names both flags. | Dev lacked the cross-selector trust-boundary failure. | **Port** | High credential/network boundary risk. Matching and mismatching pairs are tested plus a built-binary offline check. |
| Named provider runtime family | PR 3 maps `anthropic-compatible` to Anthropic API and other named profiles to OpenAI-compatible. | Dev dispatch forced every named profile to `OpenaiCompatible`. | **Port** | High protocol risk. Unit mapping assertion and exact binary build. |
| Persisted profile compatibility rules | PR 3's older architecture rejects same-source provider/provider_profile more broadly. | Dev intentionally allows compatible persisted combinations and rejects incompatible direct providers in `validate_provider_profile_combination`. | **Preserve dev** | Medium compatibility risk. Existing config session-profile tests remain authoritative. |
| Unknown or malformed profile failure | PR 3 fails before provider setup. | Dev already has stricter named choices, corrective diagnostics, and provider-free inspection. | **Preserve dev** | Medium trust-boundary risk. Existing profile tests and unknown-profile binary check. |
| Interactive `/profile` switching and next-turn application | PR 3 supplies initial switching behavior. | Dev has broader picker, queueing, restoration, and session-local state. | **Preserve dev** | High state-isolation risk. Run profile shared-state isolation tests. |
| Handoff, protocol, harness, startup metadata | PR 3 contains earlier versions. | Dev has later specs and typed protocol/harness coverage. | **Preserve dev** | High integration risk. No PR3 code copied in these areas. |
| Documentation/default template | PR 3 adds a dedicated older session-profile document. | Dev default config already documents supported fields, precedence, early failure, inspection, and interactive commands. | **No-op** | Low documentation risk. Review current template; avoid duplicate stale docs. |
| PR 2 inline previews | Not part of PR 3 routing hardening. | Owned by another agent. | **Reject/exclude** | High coordination risk. Final changed-file review must contain no PR2 implementation. |

## Selected change surface

- `src/cli/profile.rs`: derive a named profile's runtime provider family, clear lower-precedence cross-selector values, fail incompatible explicit selectors, and keep dispatch composition flat.
- `src/cli/profile/tests.rs`: house the existing profile tests plus focused precedence regressions outside the previously oversized production file.
- `src/cli/dispatch.rs`: activate the selected named profile through the profile owner instead of forcing OpenAI-compatible.
- `crates/jcode-config-types/src/lib.rs`: derive the unchanged false/empty `FileMentionsConfig` default to satisfy current Clippy after PR1 landed.
- This acceptance record and its Autospec-compatible YAML artifacts.

## Validation record

- `JCODE_IN_DEV_CARGO=1 CARGO_TARGET_DIR=$JCODE_SCRATCH_DIR/jcode-anw-pr3-target scripts/dev_cargo.sh test -p jcode --lib cli::profile::tests -- --nocapture --test-threads=1`: **PASS, 23 tests** on 2026-08-25, including matching and mismatching direct-selector conflict coverage.
- Exact-worktree preservation checks: **PASS** for `jcode-base` session profiles, static profile prompt order, and `jcode-app-core` profile session prompt/tool/shared-state isolation.
- Exact-worktree selfdev binary: **BUILD PASS**, identity `jcode v0.79.677-dev (192f9bb19, dirty)`.
- Offline exact built-binary checks: **PASS** for matching and mismatching direct selector pairs, a named session profile plus explicit selectors, and an unknown provider profile. Every case failed before provider initialization with the expected corrective diagnostic.
- Final exact-worktree guardrails: **PASS** on 2026-08-25, including formatting, all-target/all-feature check, Clippy with warnings denied, dependency and quality ratchets, desktop frame budget, and onboarding invariants. `cargo machete` was skipped because it is not installed.
- Final scope review: **PASS**. Product edits are limited to the matrix-owned profile resolver/dispatch path, extracted profile tests, and the behavior-neutral PR1 `Default` derive needed by current Clippy. No PR2 implementation is present.
- Fresh-base validation on `dev` at `c46c53a36`: **PASS** for all 23 serial profile tests and the complete repository guardrail suite.
- Remaining: land on `dev`, push, install, reload, and close the Bead.

## Risk review

**Risk level: High before validation, Medium after focused and fresh-base checks.** Provider selection sits at a credential and wire-protocol boundary. A wrong choice can send a request through the wrong API shape or credential path. Blast radius is limited to runs selecting a session profile or named provider profile. Mitigation is a surgical routing change, strict-config lookup before activation, extracted tests that shrink the oversized production owner, serial focused tests, exact-worktree builds, offline failure-path checks, and fresh-base integration. The `FileMentionsConfig` cleanup is behavior-neutral because Rust's derived default remains `enabled = false` and `ignore = []`. Rollback is the focused merge commit. Required review action is to confirm every product edit maps to a **Port** row or documented delivery-gate repair and that PR2, prompt composition, switching, handoff, protocol, and harness behavior are untouched.
