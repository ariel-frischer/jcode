# Workflow observer runtime acceptance

Bead: `jcode-upir`. Implementation remains in progress. This is the requirement-to-check map, not a claim that checks have passed.

| Requirement | Check | Evidence/state |
| --- | --- | --- |
| FR-001/009 disabled behavior and bounds | Config default, TOML, environment and template focused tests | Foundation milestone passed, runtime disabled-work test still pending |
| FR-003 explicit ownership | Registry idempotence, conflicting owner rejection, unobserve ownership, persisted duplicate rejection | Registry identity, duplicate ownership, private persistence and transactional save tests passed |
| FR-005 reconnect and continuity | Registry last-good round trip, live phase transition, opted-in reconnect snapshot | Persistence round trip passed, runtime pending |
| FR-007/008 safe optional artifact input | Missing, malformed, oversized, leaf and ancestor symlink replacement fixtures | Bounded reads, post-registration and concurrent ancestor replacement tests passed on Linux |
| FR-004/006 evidence-based health | Separate progress checkpoint and activity clocks, quiet-not-failed, sticky credit failure, explicit retry recovery | Observer fixtures passed, native event fixtures pending |
| FR-002/NFR-001 idle main model | Built isolated-socket fake producer with model request counter staying zero, no observer command invocation | Pending |
| Protocol compatibility | Legacy Subscribe omits default-false capability, new events only on opted-in connection, resume follows current session, slow-client coalescing | Pending |
| NFR-002 bounded presentation | Actual candidate debug tester frames at 40/80/120 columns and short height | Pending |
| Delivery | Focused checks, scoped format/lint, build, repository guardrails, independent review and HTML risk report | Pending |
| Landing and data safety | Required risk authorization, fresh-base non-rewriting integration, push, install/reload, owned worktree cleanup | Pending. No installed configuration enablement without concrete approval |

## Resolved testing-path hazard

`selfdev test` installs a shell Cargo shim with `JCODE_DEV_CARGO_SCRIPT` rooted at the caller checkout. A nested `cd <worktree> && cargo test` can still invoke the root wrapper, which changes back to its own directory. Use the absolute worktree-owned `scripts/dev_cargo.sh` path for coordinated tests. Confirm the compiler package path is the candidate worktree. The first resumed attempt was cancelled after detecting root paths, and is not candidate validation evidence.

## T002 milestone evidence

Coordinated task `295033dkp3` ran the absolute worktree `scripts/dev_cargo.sh test -p jcode-app-core --lib workflow::` and passed all 15 tests on Linux. Earlier red runs failed on missing persistence, unsafe parent traversal, absent observer progress/credit health, and absent transactional registration before each implementation. Scoped rustfmt and `git diff --check` passed. Windows no-reparse handle traversal is implemented but has not been compiled or runtime-tested in this milestone. No scheduler, tool action, protocol event, renderer, installed config or daemon was changed.

The optional controller adjunct JSON is an explicit producer contract, not a claim that current Autospec already emits it: `state` is one of `running`, `waiting`, `retrying`, `blocked`, `failed`, `completed`, `stopped`; optional `error_code` maps known quota, rate-limit and authentication codes to safe observer-authored text. Other JSON fields, including raw `message`, are ignored. Missing/invalid artifacts retain prior confirmed lifecycle and task progress with a warning. First observation does not invent a recent activity timestamp. Actual later task/lifecycle changes supply activity evidence.
