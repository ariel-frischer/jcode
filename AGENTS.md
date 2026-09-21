# Repository Guidelines

## Project Identity

- This checkout is Ariel Frischer's custom development version of Jcode, not the
  official upstream Jcode distribution.
- Keep the custom branch reasonably synchronized with upstream
  (`1jehuang/jcode`) while preserving Ariel-specific improvements and clearly
  treating experimental features as intentional local work.
- When evaluating upstream issues or proposing work, first check whether the
  issue is already fixed locally, conflicts with an intentional customization,
  or is especially relevant to Ariel's workflows such as `jcode run` and named
  session profiles.

## Development Workflow

- **Release only when Ariel explicitly asks.** Use `/jcode-release` from
  `.claude/skills/jcode-release/SKILL.md` for stable custom-fork releases. Ordinary
  development, validation, automatic continuations, and completed tasks do not
  authorize promoting `dev`, pushing public `main`, or publishing a release.

- **Preserve explicit swarm routing during upstream syncs.** Keep the local
  routing contract and both CI/local gate invocations. Run
  `bash scripts/check_swarm_routing_contract.sh` against the synchronized tree.
  Do not replace it with upstream's operator-only model assertions or accept a
  zero-test run. See `docs/CAPABILITY_ROUTING_CONTRACT.md` for the local contract.
- **Keep the root checkout on `dev`.** `/home/ari/repos/jcode` is the integration
  checkout, not a disposable review checkout. Agents must not create a local
  review branch for use in this checkout or switch it to a feature, pull-request,
  review, or temporary branch unless Ariel explicitly asks to change the root
  branch. At the start and end of work performed from the root checkout, verify
  `git branch --show-current` is `dev`. If it is not, stop mutation, preserve any
  dirty state, and restore `dev` only after confirming the switch is safe.
- **Inspect pull requests without changing the root checkout.** For read-only
  review, prefer `gh pr view`, `gh pr diff`, and `git show <oid>:<path>`. If a PR
  must be checked out, use `scripts/worktree-setup.sh` to create a dedicated
  worktree and branch under `.worktrees/`. Never fetch a pull-request head into a
  local branch and then check that branch out in the root checkout. Fetch exact PR
  heads into a remote-tracking ref such as `refs/remotes/github/pr/<n>` when
  checkout is unnecessary.
- **Stay on your own branch.** Do not take, cherry-pick, merge, or copy code from
  other people's or other agents' branches unless the source branch belongs to a
  repository maintainer and the user explicitly asks you to integrate it. Only
  work from your branch and its base otherwise. Never integrate branches owned
  by non-maintainers or other agents yourself; tell the user and let them decide
  how to proceed. This restriction does not prevent read-only review of any
  contribution on its merits. Preserve unrelated work and use the documented
  worktree/integration workflow.

## Repository Scope

- Jcode Desktop is in a separate repository.

- **Use the user's Git identity** - Create commits with the configured
  `user.name` and `user.email`. Do not override them with `Jcode`, `Jcode agent`,
  or a fabricated agent email. Preserve existing contributor attribution when
  integrating work. If no identity is configured, ask rather than inventing one.
- **Welcome pull requests from everyone** - Review contributions on their merits,
  regardless of whether the author is a maintainer, an existing contributor, a
  first-time contributor, or an agent. Good PRs can be merged directly after review
  and validation. Do not require a maintainer-authored rewrite merely because of
  who submitted the change. See `CONTRIBUTING.md` for the contribution policy.
- **Keep work scoped** - Work on your own branch and preserve unrelated work. When
  the user asks you to review or integrate a PR or branch, you may inspect, test,
  and integrate that contribution regardless of author status. Do not pull in
  unrelated branches or merge a PR without user authorization.

## Worktree Hygiene

Every worktree carries its own Cargo `target/`. Cargo shares nothing between
them, and a single built worktree runs 12-33 GB. A fleet of 20+ review worktrees
silently reached 382 GB and filled the disk, so cleanup is part of finishing a
task, not a separate chore.

- **Remove your worktree when the work has landed.** Before starting new work in
  a fresh worktree, remove any of your own worktrees whose branch is already
  merged. Do not leave merged worktrees parked "just in case" - the branch and
  its commits survive in the repository after the worktree is gone.
- **Verify merged status against `github/dev`, not `origin/*`.** This repository
  has **no `origin` remote**; it uses `github`, `gitlab`, and `upstream`. A base
  of `origin/main` silently resolves to nothing and makes `git cherry` report
  every branch as fully merged:

  ```bash
  git cherry github/dev HEAD | grep -c '^+'   # 0 == fully merged (survives squash merges)
  ```

- **Remove with `git worktree remove`, never `rm -rf`.** Omit `--force` so git
  refuses any worktree with uncommitted or untracked changes; that refusal is the
  safety net, so investigate rather than re-running with `--force`.

  ```bash
  git worktree remove .worktrees/agent/<name>
  ```

- **`git worktree list` is the only authority on what is a worktree.** Do not
  infer from directory layout. `.worktrees/agent/` is a *container* holding many
  live worktrees, not a worktree itself, and stale directories left under
  `.worktrees/` are not registered at all - `git -C` inside one silently walks up
  and reports the *parent* checkout's branch and HEAD.
- **Reclaim space without deleting a worktree you still need.** Deleting only
  `target/` keeps the branch and sources intact; `cargo build` regenerates it:

  ```bash
  scripts/clean_target.sh --sweep 7            # dry-run; add --apply to act
  ```

  Run it from inside the worktree you want cleaned. It resolves `target/` from
  its own checkout and has no cross-worktree awareness, so a fleet of worktrees
  needs one invocation each. It already skips profiles with an active
  `cargo`/`rustc` process or recent writes, so it is safe alongside other agents.
- **Prune registry entries** after removing directories by hand:
  `git worktree prune`.

## Cargo Build Cache Budget

- **Budget across the whole repository, not per worktree.** Count allocated bytes
  in the root and all worktree `target/` directories plus every Jcode-owned
  `CARGO_TARGET_DIR` under `~/.jcode/scratch` or elsewhere. Aim to retain at most
  **10 GiB of inactive build caches**. **20 GiB aggregate** is the cleanup trigger,
  not permission for each worktree to retain 20 GiB. Active builds may temporarily
  exceed it, but record the exception and reclaim eligible caches when they finish.
- **Retain the newest compatible build cache for fast incremental builds**, plus
  caches still used by active work. Prefer deleting older redundant build-only
  targets, not the warm cache the next build will reuse. If this protected set
  alone exceeds the budget, report the exception instead of deleting it to hit
  a number. Recency is based on descendant activity, not directory names.
- **Check before every Cargo-heavy build/test/Clippy run and at task completion.**
  Above the trigger, reclaim verified inactive, disposable, task-owned caches
  before allocating another target directory. If protected work prevents cleanup,
  report the measured usage and owner/blocker and avoid starting additional
  cache-heavy work until there is a safe capacity plan. Do not wait for disk-full.
- **Do not mint a new scratch target for every validation retry.** Reuse the
  task's compatible target directory for sequential checks. Use a fresh target
  only when isolation or clean-build evidence requires it, record its owner and
  path, and clean up that task-owned output after evidence is retained and no
  process uses it. Do not repoint another agent's target or share concurrent builds.
- **Make cleanup part of every validation/landing handoff.** Record aggregate
  before/after usage, removed cache paths, and protected exceptions. Keep reports,
  plans, source changes, and required binaries outside disposable cache trees.
  The root/orchestrator owns cross-task cleanup, not workers.
- **Never use the budget as a blind deletion rule.** Check resolved paths,
  newest descendant timestamps, open files/processes, unfinished work and repo
  ownership immediately before cleanup. Protect active/shared caches, installed
  or mapped executables, source snapshots, documents, databases and Dolt spool
  files. Unknown ownership means retain. Recent shared scratch artifacts retain
  the cleanup skill's 14-day protection. Only an explicitly task-owned disposable
  target may be cleaned promptly after its completed validation and quiescence
  are verified. Historical or other-agent caches require a reviewed candidate
  manifest and approval under `/jcode-scratch-cleanup`.
- This is an agent workflow requirement, **not an installed background sweeper**.
  Use existing verified repo cleanup tools within their actual scope. Never run
  blanket scratch deletion or remove worktrees just to satisfy the cache budget.

## Install Notes
- After landing a merge into `dev`, automatically install the newer build with
  `scripts/install_release.sh --fast` and gracefully reload the shared server.
  Keep effects on running Jcode minimal; a brief client disconnect/reconnect is
  allowable, but do not force-stop the server.
- `~/.local/bin/jcode` is the launcher symlink used from `PATH`.
- `~/.jcode/builds/current/jcode` is the active local/source-build channel; self-dev builds and `scripts/install_release.sh` point the launcher here.
- `~/.jcode/builds/stable/jcode` is the stable release channel; `scripts/install.sh` installs this and points the launcher here.
- `~/.jcode/builds/versions/<version>/jcode` stores immutable binaries.
- `~/.jcode/builds/canary/jcode` still exists for canary/testing flows, but it is not the primary self-dev install path.
- On Windows, the equivalents are `%LOCALAPPDATA%\\jcode\\bin\\jcode.exe` for the launcher, `%LOCALAPPDATA%\\jcode\\builds\\stable\\jcode.exe` for stable, and `%LOCALAPPDATA%\\jcode\\builds\\versions\\<version>\\jcode.exe` for immutable installs; `scripts/install.ps1` currently installs the stable channel.
- Ensure `~/.local/bin` is **before** `~/.cargo/bin` in `PATH`.

## Go SDK Ownership

- `github.com/ariel-frischer/jcode-go` branch `dev` is the sole Go SDK development source. Version tags in that repository are the release boundaries consumed by Go modules.
- `crates/jcode-harness-api` remains Jcode's authoritative Rust protocol-v1 wire contract. Do not add a second Go implementation, vendored copy, generated projection, submodule, or synchronization path under this repository.
- For wire compatibility, run `scripts/validate_jcode_go_compat.sh --jcode-go-dir /absolute/path/to/jcode-go`. The validator is read-only and requires an explicit checkout.
- Run the complete formatting, module, vet, build, test, race, and Windows compile matrix in `jcode-go` itself before integrating or tagging SDK changes.

## Swarm and Agent Limits

- On this resource-constrained development machine, default to **1 active
  implementation worker** and never exceed **2 concurrent workers** without
  Ariel's explicit approval.
- Serialize Rust builds, tests, Clippy, and guardrail runs across workers. Do not
  run multiple Cargo-heavy commands in parallel.
- Each Jcode thread may spawn at most **2 direct swarm workers**, subject to the
  stricter one-worker default above.
- **Sub-subagents are forbidden.** Workers must not spawn, assign, or delegate to
  additional agents. Only the root Jcode instance may create swarm workers.
- Keep swarm work bounded and cancel or stop workers promptly when their task is
  complete. Do not create large fan-out bursts or parallel build commands.

## Verifying a change at runtime

`cargo build` alone proves nothing about behavior. `jcode run` and interactive
sessions are served by the long-lived daemon at
`~/.jcode/builds/shared-server/jcode`, which is a symlink into
`~/.jcode/builds/versions/<version>/`. Until that symlink is repointed and the
daemon restarted (`jcode self-dev --build`), a freshly built binary is inert and
every runtime check silently measures the old code.

To test a change without disturbing the shared daemon or the caller's session,
run your build against its own socket:

```bash
cargo build --profile selfdev
./target/selfdev/jcode run --no-update --socket /run/user/1000/jcode-mytest.sock '<prompt>'
```

Two things that waste time otherwise:

- `crate::logging::info` writes to a log file, not stderr, so instrumenting a
  code path with it produces no visible output under `--trace`. Use `eprintln!`
  for throwaway diagnostics and delete it before committing.
- Confirm which binary you are actually inspecting. `strings` on
  `builds/shared-server/jcode` reads a 70-byte symlink, not a program; resolve it
  with `readlink -f` first.
