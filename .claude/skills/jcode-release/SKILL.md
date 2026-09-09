---
name: jcode-release
description: Release Ariel Frischer's custom Jcode fork only when the user explicitly requests a release and the selected dev candidate is release-ready. Guides main promotion, GitHub CI monitoring, and real artifact verification. Never auto-release after ordinary development.
disable-model-invocation: true
---

# User-requested custom Jcode releases

## Non-negotiable publication gate

This skill is for **`ariel-frischer/jcode`**, not upstream `1jehuang/jcode`.
Execute publication only when Ariel explicitly asks for a release of this custom
fork. Reading this skill, an automatic continuation, a completed feature, a clean
test run, "is it ready?", or a request to inspect/preview releases is **not**
authorization to publish.

Preview/validation does not authorize applying a publication manifest, committing
publication changes, or pushing public `main`. Obtain explicit maintainer approval
for each concrete action, unless the current explicit request already covers that
exact action and candidate. Record the request, selected commit and effects on the
release Bead. Approval for one release never carries over to the next release or
to a further main push after a failed attempt.

The frontmatter is a discovery hint, not a security boundary. The approval rule
applies even if an agent loads or auto-discovers this skill. Never reset passwords,
change credentials, enable paid runners, or weaken release checks to get a release.

## Branch and release contract

- `/home/ari/repos/jcode` stays on **`dev`**, the ordinary development branch.
- **`main` is the release branch. A push to it triggers public release CI.**
- Never merge/promote `dev` automatically. Never push `main` as routine cleanup,
  validation, synchronization, or because an agent thinks the fork is stable.
- Do not change `master`, the repository default branch, branch protection, or
  upstream remotes unless the user separately requests those changes.
- Read `.github/workflows/release.yml` and `docs/dev/main-releases.md` from the
  actual selected candidate rather than relying on this skill's remembered details.
- The current workflow builds the exact triggering SHA, uses a numeric calendar
  version `v<source-year>.<source-month>.<workflow-run-number>`, and requires all six
  CLI targets before publication. Windows artifacts are **unsigned**. It does not
  publish desktop installers, Homebrew/AUR packages, or Discord announcements.
- Runtime updater and installer defaults may still point upstream. Do not silently
  change them or claim a fork release enables automatic fork updates.

## 1. Inspect and establish release readiness

Load `/gh` and `/beads`. Search existing release work and reuse the owning task.
Use `/dev-workflow` for any implementation repair and preserve its worktree rules.
Prefix shell commands with `rtk` and use explicit GitHub repository targets.

```bash
rtk git branch --show-current
rtk git status --short
rtk git remote -v
rtk git fetch github dev
rtk proxy git ls-remote --heads github main dev master
rtk proxy gh repo view ariel-frischer/jcode --json nameWithOwner,defaultBranchRef,isPrivate
rtk proxy gh release list -R ariel-frischer/jcode --limit 5
rtk proxy gh run list -R ariel-frischer/jcode --workflow release.yml --limit 5
```

A release-ready candidate is a clean, explicitly selected commit containing only
intended changes, with relevant behavior/build checks passing and no unresolved
release-critical failures. Stability is not established by a label or a mock.
Inspect the diff since the last released main commit, relevant Beads, known
regressions, and release notes. Run focused checks before broad checks:

```bash
rtk proxy python3 scripts/test_main_release.py -v
rtk proxy python3 scripts/test_post_discord_release.py -v
rtk git diff --check
```

Run applicable repository guardrails and affected runtime tests. Disclose and
isolate known baseline failures. Never call all checks green if any failed or
timed out. An unrelated ratchet mismatch is not itself proof of a broken runtime,
but unresolved build, functional, security, or artifact-provenance failures block
publication. Do not skip or bypass the required hosted matrix. Do not spend live
AI-provider budget for release validation without separate explicit approval.

Freeze and record the full candidate SHA. If the worktree, remote branch or selected
content changes, reevaluate the candidate and its authorization before proceeding.

## 2. Promote only the explicitly approved candidate

Announce the exact candidate and that pushing main starts public build/publication.
Keep the root checkout on dev. Do not use force push, reset, rebase, or retagging.

For a first release, verify that main is absent, then use the **approved immutable
SHA**, not a moving branch name:

```bash
# Publication effect. Run only under the recorded explicit approval.
rtk git push github <approved-full-sha>:refs/heads/main
```

For later releases, fetch main and inspect ancestry. A normal main push is valid
only if the remote main commit is an ancestor of the approved candidate. If not,
prepare an explicitly owned integration worktree with the repository helper,
preserve both histories with a non-rewriting merge, validate the resulting commit,
and obtain approval for that concrete publication candidate. Never switch the root
checkout off dev. A rejected push requires diagnosis, not a force retry.

Verify the resulting remote main SHA. Do not manually create release tags: the
workflow owns deterministic tag creation and source-collision checks.

## 3. Observe the actual hosted release

Find the run by **both main and approved source SHA**, not merely "latest". Record
its URL and ID. Use native GitHub monitoring with one completion subscription:

```bash
rtk proxy gh run list -R ariel-frischer/jcode --workflow release.yml --branch main --limit 5 --json databaseId,headSha,status,conclusion,url
rtk proxy gh run watch <run-id> -R ariel-frischer/jcode --exit-status --interval 60
```

For a long-running watch, use the host's durable background/completion mechanism.
Do not install polling loops, repeatedly stack watchers, or interpret a local
watcher timeout/reload as a GitHub build failure. Confirm the real run status.

If a job fails, read its concrete failed logs once, diagnose, then perform one
bounded repair lane. Keep the release Bead open. Do not mark drafts as releases,
upload guessed binaries, publish a partial matrix, remove safety gates, or mutate
a published release. A new main push needs explicit approval of that new action.
A rerun of the same approved immutable candidate may retry a diagnosed transient
failure, but must not silently expand publication scope.

## 4. Verify the public result, not just CI status

After all required jobs succeed:

1. Read the actual GitHub release and verify it is public, not a draft. Resolve its
   tag to the exact approved main SHA. Record release and workflow URLs.
2. Check the required Linux/macOS/Windows CLI assets, unsigned Windows disclosure,
   release notes, `SHA256SUMS`, and `BUILD-PROVENANCE.txt`.
3. Download the real assets into a task-owned directory under `$JCODE_SCRATCH_DIR`
   using `gh release download -R ariel-frischer/jcode`. Independently calculate
   SHA256 hashes and compare them with the published checksum file.
4. Confirm provenance names this fork, the approved source SHA, tag and run ID.
5. On this Linux x86_64 host, unpack the actual downloaded archive in scratch and
   run its bundled wrapper with `--version`. Check the expected numeric release
   version and commit hash. Do not replace the installed local binary merely to
   smoke-test an asset. Preserve wrapper support files. Never call live providers.
6. Record hosted/native-platform coverage honestly. Cross-platform job success is
   not a claim that every platform was independently executed on this Linux host.

## 5. Close and preserve the handoff

Retain a concise HTML report with risk, exact SHA/tag/URLs, per-requirement evidence,
known failures, and rollback options. Close the owning Bead only from actual
publication and artifact evidence, not aggregate mock-test counts. Safely clean
only task-owned resources. Update this skill/runbook when verified process details
change, keeping the user-only approval rule intact.

If the request was only a preview or publication remains unapproved, record that
boundary and stop without mutating main. Do not keep an active automatic todo that
can pressure a later agent into publishing. Future releases require a new explicit
user request when the custom fork is ready.
