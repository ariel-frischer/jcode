# Jcode fork research: ShawnSantiago, grigio, and sheikhsajid69

Date: 2026-09-09

Status: descriptive research, retained for reference only. **No adoption, code copying, cherry-picking, merging, installation, or execution is authorized by this note.**

## Summary

| Fork | Observed purpose | Important qualification |
| --- | --- | --- |
| [ShawnSantiago/jcode](https://github.com/ShawnSantiago/jcode) | PR monitoring, workflow infrastructure, and autonomous-development procedures | Some features are foundations rather than fully integrated runtime behavior. |
| [grigio/jcode](https://github.com/grigio/jcode) | Nix/NixOS packaging, upstream-release automation, and binary caching | Builds pinned official upstream source, not a different agent implementation. |
| [sheikhsajid69/jcode](https://github.com/sheikhsajid69/jcode) | Older upstream snapshot | No commits unique to its default branch relative to upstream at inspection time. |

Stars at inspection were 2, 2, and 3 respectively. Stars and total commit counts are not reliable indicators of original feature work.

## Scope and evidence limits

This was read-only inspection of public GitHub repository metadata, branches, comparisons, commit history, source, documentation, and release records. No fork was built or run, and runtime correctness is not established. No comparison against every feature already present in Ariel's custom fork was performed.

GitHub aggregate comparisons can be misleading when forks lag upstream. Merge commits, replayed commits, and automated updates inflate ahead counts. Large compare responses can also hit the 300-file cap. Feature identification therefore used representative commit-level evidence and current files, not aggregate diff size alone.

Branch URLs below are mutable. The commit references identify historical changes, while descriptions of current behavior refer to the inspection date.

## ShawnSantiago: what is actually there

The default `master` head was [3f7339d](https://github.com/ShawnSantiago/jcode/commit/3f7339d28c9cb7aad96896510c5101262954cc4d), an upstream merge dated July 29. An [August 19 integration branch](https://github.com/ShawnSantiago/jcode/tree/integration/upstream-20260819) also existed.

The default-branch comparison reported 95 commits ahead and 1,145 behind upstream. Of those 95 commit objects, 38 were explicitly upstream merges, and there were only 56 distinct first-line titles. Do not interpret this as 95 features.

### PR watcher and webhook work

The [initial PR-watch implementation](https://github.com/ShawnSantiago/jcode/commit/ce2a8b94c1ec639398106f2f35a1a0300673c4ae) added a core crate, tool implementation, skill, tests, and integration documentation. A [later commit](https://github.com/ShawnSantiago/jcode/commit/d03fc862cd93da92e51824060117367497ebfe02) added structured monitoring.

The separate [feature/pr-watch-native-webhook branch](https://github.com/ShawnSantiago/jcode/tree/feature/pr-watch-native-webhook), observed at [fbf7a91](https://github.com/ShawnSantiago/jcode/commit/fbf7a91e7b1f8141eef2f4123c518d0c94ee495c), implements event-driven watch refreshes:

1. GitHub sends an event to a local HTTP receiver exposed through `jcode pr-watch webhook serve`.
2. The receiver validates request shape, body size, delivery headers, HMAC-SHA256 signature, and JSON.
3. Delivery IDs are persistently deduplicated. The reviewed implementation retains up to 10,000 IDs for seven days.
4. Matching watched PRs receive a deduplicated follow-up with a roughly 10-second debounce, combining nearby events.
5. A read-only `webhook_heartbeat` acquires the watch lock, validates watch identity and state, and fetches authoritative PR information through GitHub CLI collection.
6. The watch state is refreshed. Webhook mode suppresses normal monitor polling, while hybrid mode retains polling.

Events include PR changes, reviews, review comments, PR issue comments, and relevant checks/status updates. Unroutable events are ignored or logged rather than treated as instructions.

**Boundary:** this is not a complete “review arrives, agent automatically fixes code” feature. The inspected webhook path refreshes state, does not grant write scopes, does not push or comment, and does not directly launch an LLM turn. Any subsequent agent work would depend on existing watch handoff machinery.

Useful analogy: a notification bell and refreshed dashboard, not the developer responding to the bell.

Sources:

- [Webhook tool implementation](https://github.com/ShawnSantiago/jcode/blob/feature/pr-watch-native-webhook/crates/jcode-app-core/src/tool/pr_watch.rs)
- [PR-watch core](https://github.com/ShawnSantiago/jcode/blob/feature/pr-watch-native-webhook/crates/jcode-pr-watch-core/src/lib.rs)
- [Fork PR #9](https://github.com/ShawnSantiago/jcode/pull/9)
- [Published final review](https://github.com/ShawnSantiago/jcode/blob/feature/pr-watch-native-webhook/docs/reviews/pr-watch-native-github-webhook-implementation-claude-final-review.md)

The published review records remaining work including a live tunnel probe, clearer collapsed/dropped-event reporting, concurrency limits, and retry/backoff. Those are reported review findings, not validations performed in this research.

### Consensus planning: correct the maturity claim

The fork added [native OMX workflow skills](https://github.com/ShawnSantiago/jcode/commit/4f1621482ba334a7ae5b982f99a260bde0b296b1), a [`jcode-workflow` crate](https://github.com/ShawnSantiago/jcode/commit/d47dee03e9e2db15b8a80bfaf58f39927b7ffdac), and a [`ralplan` runtime](https://github.com/ShawnSantiago/jcode/commit/0423f352c03fb54556f344fb97fb27e08a27eb69).

Its intended sequence is:

```text
Draft plan -> validate dependency graph -> architect review -> critic review
     ^                                                           |
     +---------------- revise if requested ----------------------+
```

- Drafts contain structured plan items and dependencies.
- Validation rejects empty plans, duplicate IDs, dependency cycles, unresolved dependencies, and plans with no runnable items.
- The architect review precedes the critic review.
- The critic chooses `Approve`, `Iterate`, or `Reject`. Its verdict controls termination, so this is not majority voting or a requirement that every reviewer independently approve.
- The default limit is three rounds. Executor errors and invalid plans terminate with explicit statuses.
- The workflow library recognizes `/ralplan`, `/ulw`, and `/ultrawork` activation. Recognizing a command is not proof of a complete model-backed execution path.

**Important correction:** initial discussion described this too strongly as a working native consensus-planning feature. The inspected public implementation provides a generic `RalplanConsensusExecutor` interface and `FakeExecutor` tests. The bounded source investigation found no production executor connecting the planner/architect/critic stages to model calls, nor explicit production model selection for those roles. Treat it as **native planning-loop infrastructure**, not a verified finished multi-agent workflow.

Sources: [workflow library](https://github.com/ShawnSantiago/jcode/blob/master/crates/jcode-workflow/src/lib.rs), [consensus runtime and tests](https://github.com/ShawnSantiago/jcode/blob/master/crates/jcode-workflow/src/ralplan.rs).

### Autonomous-development tooling

#### Visual QA loop

The [visual-qa-loop skill](https://github.com/ShawnSantiago/jcode/blob/master/.jcode/skills/visual-qa-loop/SKILL.md) directs an agent to:

1. Define target routes, reference designs, viewports, and acceptance criteria.
2. Capture screenshots and inspect DOM/interactive behavior on mobile and desktop.
3. Identify layout, typography, interaction, and basic accessibility problems.
4. Apply scoped fixes and repeat the same observations.
5. Run project checks and route smoke tests, then commit and prepare appropriate PR follow-up.

This is a reusable skill operating existing browser/editing tools, not a new vision engine. Its suggested 90+ visual-parity score is a workflow rubric, not an independently calibrated benchmark. It requires evidence before declaring success and restricts overwriting concurrent changes, committing scratch artifacts, and unauthorized merges or deletion.

#### Overnight operation

The [overnight runbook](https://github.com/ShawnSantiago/jcode/blob/master/docs/OVERNIGHT_AUTONOMOUS_RUNBOOK.md) describes long-running, time-bounded development with durable manifests, checkpoints, validation records, and handoffs. It calls for 30-minute watchdog checks, recovery of stale PR watches, authoritative GitHub checks before PR gates, and recovery when work becomes idle. Continuous-progress mode can generate a new bounded backlog when the old one finishes before the time budget.

Its PR quiet-cycle procedure uses three five-minute quiet cycles followed by a final ten-minute quiet period. A new push resets the quiet-cycle accounting. These are that fork's operating procedures, not authorization to apply them here or proof that quiet time alone makes a merge safe.

The [overnight_status.py helper](https://github.com/ShawnSantiago/jcode/blob/master/scripts/overnight_status.py) is **read-only reporting**. It reads checkpoints, Git status, and PR-watch state. It does not itself enforce watchdog invariants or act as the autonomous controller.

Other additions include a [PR-creation helper](https://github.com/ShawnSantiago/jcode/commit/3aea7ce7d347898cbcb858806cdfe28f71de6373) and a [stdin detection fix](https://github.com/ShawnSantiago/jcode/commit/9738eccc995a841dd0778f14067162f9f6a76d5f).

## Grigio: upstream engine, Nix distribution

Grigio's [flake](https://github.com/grigio/jcode/blob/master/flake.nix) packages a pinned official upstream source input, observed as `github:1jehuang/jcode/v0.84.0`. It does not build a separate modified agent implementation from the fork's checkout.

The added value is distribution infrastructure:

- Nix/NixOS package definition and Rust development shell.
- Locked dependencies and build, formatting, lint, and test checks.
- Automated upstream-release pin updates.
- Signed binary-cache publishing, including the Crane dependency layer to avoid recompiling dependencies.

Think “a Linux distribution packaging Firefox,” not “someone building a different browser.” Same agent engine, different installation/build/cache route.

The [nix-v0.84.0 release](https://github.com/grigio/jcode/releases/tag/nix-v0.84.0) was published September 7. The [release workflow](https://github.com/grigio/jcode/blob/master/.github/workflows/nix-tag-release.yml) describes the automation. The observed 48 ahead commits included 24 automated pin updates and one merge, with the rest focused on packaging, CI, docs, and cache setup.

## Sheikhsajid69: older snapshot

The default head was [50d2c68](https://github.com/sheikhsajid69/jcode/commit/50d2c68be98f26bbb686658c4dd8463027a5f5f8), an upstream-authored May 11 commit. The [comparison](https://github.com/1jehuang/jcode/compare/master...sheikhsajid69:master) reported zero ahead and 4,125 behind. No fork-specific implementation was identified on the inspected default branch. This does not claim the owner never contributed anywhere else.

## Retained assessment, not an adoption plan

The webhook receiver is the most concrete engineering candidate for a future closer look. The visual-QA skill is the simplest operating-procedure idea to study. Consensus planning needs production wiring before it should be described as a finished feature. Nix packaging is useful primarily for Nix users.

**Next action: keep this note for reference.** Any later evaluation, copying, porting, or integration requires a new explicit request, a fresh source check, and comparison with the current custom fork. Nothing in this research was adopted.
