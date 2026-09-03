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
- **Stay on your own branch** - Do not take, cherry-pick, merge, or copy code from other
  people's or other agents' branches unless the source branch belongs to a repository
  maintainer and the user explicitly asks you to integrate it. Only work from your branch
  and its base (e.g. `main`) otherwise. Never integrate branches owned by non-maintainers
  or other agents yourself; tell the user and let them decide how to proceed.

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
