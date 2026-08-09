---
name: sync-worktree
description: Sync a remote branch into dev through an isolated worktree, resolve conflicts, validate, merge, and clean up. Use when a user asks to bring a release or upstream branch into the current dev branch.
---

# Sync Worktree

Use this workflow for remote branch synchronization. It keeps conflict resolution and validation isolated until the result is ready to land.

## Request semantics

When the user says to "sync upstream into this branch," interpret the goal as
ensuring that the target branch contains the latest tip of the configured
`upstream` remote's default branch, not as requiring a merge commit. Confirm the
remote and branch first, then fetch and compare the tips:

- If the upstream tip is already an ancestor of the target branch, the branch is
  already synchronized. Do not create an empty or unnecessary merge commit, and
  report explicitly that no merge ran and therefore no conflicts were possible.
- If the target branch does not contain the upstream tip, integrate it through
  the isolated worktree workflow below, resolve any conflicts there, validate,
  and land the result into the target branch without rewriting history.

The completion report must distinguish among an actual merge, a fast-forward,
and an already-synchronized no-op. Include concrete evidence such as the fetched
upstream commit, the ancestor/divergence check, conflict status, and final
working-tree state.

## 1. Inspect remotes and preserve local state

From the repository root:

```bash
git remote -v
git status --short --branch
git worktree list --porcelain
git fetch <remote> --prune --tags
```

Identify the exact remote and branch. If multiple remotes point at the same host, do not guess. Confirm the branch tip and relevant release tag:

```bash
git rev-parse <remote>/<branch>
git log -1 --format='%H%n%ad%n%s' --date=iso <remote>/<branch>
git merge-base --is-ancestor <remote>/<branch> dev
```

Preserve unrelated dirty files and existing worktrees. Never use `git stash`, `git reset`, `git add -A`, or `git add .`.

## 2. Create an isolated worktree

Use the repository setup script:

```bash
bash scripts/worktree-setup.sh agent/sync-<short-name> dev
```

Record the absolute path returned by the script. Do all merge edits in that worktree. Do not modify the main checkout while the merge is unresolved.

## 3. Merge and resolve conflicts

```bash
cd <worktree>
git merge --no-edit <remote>/<branch>
git status --short
```

If conflicts occur, inspect each conflict with context and understand both sides before editing. Preserve compatible changes from both branches. Remove every conflict marker, then verify:

```bash
git diff --check
git grep -n -E '^<<<<<<<|^=======|^>>>>>>>' -- . ':!target' || true
git status --short
```

Stage only the explicitly resolved files:

```bash
git add <resolved-file-1> <resolved-file-2>
git diff --cached --check
git commit --no-edit
```

If the merge was already up to date, do not manufacture a merge commit.

## 4. Validate before landing

Run focused tests for changed areas first, then repository checks appropriate to the change. For Rust changes, use the narrowest relevant package/test commands and run:

```bash
cargo fmt --all -- --check
git diff --check
```

Do not claim validation if a check fails. Record pre-existing failures separately.

## 5. Land into dev and clean up

From the main checkout, after the worktree commit is validated:

```bash
cd <main-repo>
git status --short --branch
git merge-base --is-ancestor dev <worktree-commit>
git merge --ff-only agent/sync-<short-name>
git status --short --branch
git worktree remove <worktree>
git branch -d agent/sync-<short-name>
git worktree list --porcelain
```

If `dev` changed while the worktree was in progress, stop and reconcile explicitly. Never rewrite history or remove unrelated worktrees. Cleanup is allowed only after the worktree is clean and the result has landed.

## Completion checklist

- Exact remote and branch were confirmed.
- Main checkout and unrelated worktrees were preserved.
- All conflicts were resolved in the isolated worktree.
- Focused validation passed.
- The result was merged into `dev` without rewriting history.
- Only the request-specific worktree and branch were removed.
