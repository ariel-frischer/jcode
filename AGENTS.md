# Repository Guidelines

## Development Workflow

- **Stay on your own branch** - Do not take, cherry-pick, merge, or copy code from other
  people's or other agents' branches unless the source branch belongs to a repository
  maintainer and the user explicitly asks you to integrate it. Only work from your branch
  and its base (e.g. `main`) otherwise. Never integrate branches owned by non-maintainers
  or other agents yourself; tell the user and let them decide how to proceed.

## Install Notes
- `~/.local/bin/jcode` is the launcher symlink used from `PATH`.
- `~/.jcode/builds/current/jcode` is the active local/source-build channel; self-dev builds and `scripts/install_release.sh` point the launcher here.
- `~/.jcode/builds/stable/jcode` is the stable release channel; `scripts/install.sh` installs this and points the launcher here.
- `~/.jcode/builds/versions/<version>/jcode` stores immutable binaries.
- `~/.jcode/builds/canary/jcode` still exists for canary/testing flows, but it is not the primary self-dev install path.
- On Windows, the equivalents are `%LOCALAPPDATA%\\jcode\\bin\\jcode.exe` for the launcher, `%LOCALAPPDATA%\\jcode\\builds\\stable\\jcode.exe` for stable, and `%LOCALAPPDATA%\\jcode\\builds\\versions\\<version>\\jcode.exe` for immutable installs; `scripts/install.ps1` currently installs the stable channel.
- Ensure `~/.local/bin` is **before** `~/.cargo/bin` in `PATH`.

## Go SDK Synchronization

- When a Bead affecting the Go SDK is completed, update the published SDK repository at `github.com/ariel-frischer/jcode-go` from the corresponding `sdk/go/` source. Work from that repository's `main` branch: fetch `origin`, merge or fast-forward the relevant changes into `main`, and inspect the resulting diff before publishing.
- Before pushing the SDK update, run the repository's complete validation (`scripts/validate_go_sdk.sh`, or the equivalent formatting, vet, build, test, race, and module checks), and do not publish if validation fails.
- Push the validated result to `origin/main`. Confirm the remote branch and commit after pushing, and record the SDK commit and validation commands in the completed Bead.

## Swarm and Agent Limits

- Run at most **3-5 Jcode threads/instances concurrently**. Keep the total
  bounded by host capacity rather than opening unbounded sessions.
- Each Jcode thread may spawn at most **3 direct swarm workers**. Prefer 2 when
  the task does not clearly benefit from a third independent worker.
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
