---
name: no-autospec-bead-flow
description: Run the next important Bead through a simple tracked development flow without Autospec. Use when the user asks to pick the first or highest-priority actionable Bead, implement it in a repository worktree, validate it, merge it to dev, push it, and close the Bead.
---

# No-Autospec Bead Flow

Use this skill for the repository's explicit workflow: one important Bead, one isolated worktree, focused validation, fast-forward merge to `dev`, push, and Bead closure. **Never invoke Autospec in this flow.**

## 1. Select and scope the Bead

```bash
bd ready --json
bd list --status open --sort priority --json
bd show <bead-id> --json
bd comments <bead-id> --json
git status --short --branch
```

- Pick the highest-priority actionable open Bead, respecting dependencies.
- Prefer a prerequisite decision/contract Bead over a dependent implementation Bead.
- Record its description, design, acceptance criteria, non-goals, and dependencies.
- Preserve unrelated dirty changes. Do not create a duplicate Bead.
- If the user explicitly authorized the work, remove stale `needs-approval`, add
  `approved`, and claim it. Because this skill is explicitly no-Autospec, remove
  `autospec:required`, add `autospec:skip`, and record that the user requested
  the no-Autospec path even when the Bead would otherwise be large enough for
  Autospec. Run label-removal commands only when the label is present, since
  `bd label remove` may fail for an absent label:

```bash
bd label add <bead-id> approved
bd label add <bead-id> autospec:skip
bd update <bead-id> --claim
bd comments add <bead-id> "Starting no-Autospec implementation in an isolated worktree."
```

If the task is not clearly approved, stop and ask. If it is larger than one
focused sitting, keep the same no-Autospec contract, split the work into
explicitly scoped commits or bounded stages, and record the larger scope in the
Bead instead of silently invoking Autospec.

## 2. Create the worktree

Use the repository's setup script, not a sibling checkout or an ad hoc copy:

```bash
bash scripts/worktree-setup.sh agent/<bead-id> dev
```

The script must create or reuse `.worktrees/agent/<bead-id>`, share the canonical
`.beads` and `.codegraph` paths, and sync local agent context. Work only in the
reported absolute worktree path from this point onward.

Verify the baseline:

```bash
cd <worktree-path>
git status --short --branch
git log --oneline --decorate --max-count=3
```

## 3. Implement narrowly

- Read the repository `AGENTS.md`, applicable constitution, and relevant source
  before editing.
- Make the smallest change satisfying the Bead acceptance criteria.
- Do not use Autospec, add unrelated cleanup, change adjacent architecture, or
  edit files outside the Bead scope.
- Add focused tests for behavior changes. For documentation-only work, validate
  the artifact directly and do not run unrelated tests as a substitute.
- Add a progress comment when a meaningful decision or blocker appears.

## 4. Validate before committing

Run focused checks first, then the relevant build or repository guardrail:

```bash
cd <worktree-path>
git diff --check
# Run the narrowest relevant tests or artifact checks.
# For this Rust repo, use the relevant cargo test/check/build command.
scripts/check_guardrails.sh --skip-slow  # documentation/schema-only exception
# or scripts/check_guardrails.sh         # implementation changes
```

Do not rebaseline ratchets with `--fix` merely to make the task pass. If a gate
fails outside the changed surface, record the exact failure and prove the diff
did not cause it. Fix failures required by the Bead before proceeding.

Commit only explicit owned files:

```bash
git status --short
git add <owned-file-1> <owned-file-2>
git commit -m "<focused message>"
git status --short --branch
```

## 5. Merge to dev and push

From the main checkout, ensure it is clean and do not rewrite history:

```bash
cd <main-repo>
git status --short --branch
git merge-base --is-ancestor dev <worktree-commit>  # if checking a fresh base
# or verify the remote/base ancestry as appropriate
git merge --ff-only agent/<bead-id>
git status --short --branch
git show --stat --oneline HEAD
git push github dev
```

Use a non-rewriting fast-forward only. If the main checkout or base changed,
stop and reconcile explicitly. Do not reset, rebase, amend, squash, or stash.

After a successful merge, remove the clean temporary worktree:

```bash
git worktree remove <worktree-path>
git worktree list --porcelain
git status --short --branch
```

## 6. Record and close the Bead

Add one final evidence comment with the worktree, commit, files, validation, and
any unrelated baseline failures:

```bash
bd comments add <bead-id> "Completed and merged. Commit: <sha>. Files: <files>. Validation: <commands/results>. Known unrelated failures: <none or exact gates>."
```

Then move labels cleanly and close only when all acceptance criteria are met:

```bash
bd label remove <bead-id> approved
bd label remove <bead-id> in-review
bd label add <bead-id> validated
bd close <bead-id> --reason "Completed and merged as <sha>"
bd list --status closed --label in-review --limit 0
```

Finally verify:

```bash
git status --short --branch
# Confirm dev is synchronized with its intended remote.
bd show <bead-id> --json
```

## Required final summary

Report concisely:

- Bead ID and title, plus closed/validated status.
- Worktree path and commit.
- Files changed.
- Focused validation and guardrail result.
- Merge/push result.
- Exactly one next action, if any.

Never claim full validation when a gate failed. Distinguish changed-surface
failures from unrelated pre-existing baseline failures.
