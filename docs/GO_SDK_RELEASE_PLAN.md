# Go SDK release and publication runbook

**Status:** Current publication-readiness procedure

The canonical source is `sdk/go`; the public destination is `github.com/ariel-frischer/jcode-go`. Publication is externally visible and requires explicit maintainer authorization. This reconciliation does not authorize applying to or pushing the live public repository.

## One-way ownership and evidence contract

`sdk/go` is the sole source of the typed-event implementation. The public
`jcode-go` repository is a protected publication projection, not a second
source and never an upstream for reverse synchronization. The only apply
boundary is the deterministic, fingerprint-bound manifest emitted by
`scripts/sync-jcode-go.sh`; direct public edits, reverse sync attempts, stale
manifests, dirty or ineligible destinations, unsafe paths, and unreviewed apply
commands must be rejected before the first write.

A publication-readiness record must include the canonical source revision, two
byte-identical non-mutating previews, the reviewed manifest fingerprint,
`scripts/validate_go_sdk.sh` results, the sync and validator fixture results,
and equivalent typed-event, race, module, and Windows compile checks in a
controlled projection. It must also record the public commit, push confirmation
on `origin/main`, and downstream module/version-update evidence when (and only
when) those actions are separately authorized and completed. Previewing,
validating, or applying to a controlled temporary projection never authorizes
public apply, commit, push, or live downstream validation.

## Supported validation matrix

| Go | Linux | macOS | Windows |
| --- | --- | --- | --- |
| 1.23.x | tests and race | package/build support | amd64 compile boundary |
| 1.24.x | tests and race | package/build support | amd64 compile boundary |

The protocol-v1 runtime transport is Unix-domain sockets on Linux/macOS. Windows is compile-only. Validation with another installed Go version is supplementary and must be labeled as such.

## 1. Capture eligibility and baseline

Before comparison or publication, record the public repository's resolved path, branch, HEAD, clean status, tracked path/mode list, and deterministic tracked-content fingerprint:

```bash
realpath /absolute/path/to/jcode-go
git -C /absolute/path/to/jcode-go branch --show-current
git -C /absolute/path/to/jcode-go rev-parse HEAD
git -C /absolute/path/to/jcode-go status --porcelain=v1 --branch
git -C /absolute/path/to/jcode-go ls-files -s
git -C /absolute/path/to/jcode-go ls-files -s -z | sha256sum
```

Stop unless the destination is the intended `github.com/ariel-frischer/jcode-go` repository, on `main`, and clean. Do not switch its branch or repair it implicitly.

## 2. Review the complete inventory

Compare canonical payload and all public tracked paths. Every difference must have category, authority, disposition, rationale, and a named recurring rule or one-time reconciliation action. `crates/jcode-harness-api` resolves wire questions. Public-only governance/specifications/docs are intentional protected differences.

The reviewed inventory for this reconciliation is [`../specs/006-sdk-go-reconcile/divergence-inventory.md`](../specs/006-sdk-go-reconcile/divergence-inventory.md).

## 3. Produce deterministic preview evidence

From the Jcode repository root:

```bash
scripts/sync-jcode-go.sh preview \
  --source sdk/go \
  --destination /absolute/path/to/jcode-go > /tmp/jcode-go.manifest

scripts/sync-jcode-go.sh preview \
  --source sdk/go \
  --destination /absolute/path/to/jcode-go > /tmp/jcode-go.second.manifest

cmp /tmp/jcode-go.manifest /tmp/jcode-go.second.manifest
```

Preview/default mode is read-only. Review the manifest header/version, both fingerprints, complete ordered rules, every operation, and each retained exclusion. Expected protection includes `.git`, `.github`, `.gitignore`, `.autospec`, `.beads`, `.worktrees`, `specs`, `AGENTS*.md`, public `docs/**`, and unknown paths.

## 4. Validate canonical readiness

Run:

```bash
scripts/test_sync_jcode_go.sh
scripts/test_validate_go_sdk.sh
scripts/validate_go_sdk.sh
```

The validator must report 7/7 passes: formatting, module consistency (`go mod tidy -diff` and `go mod verify`), vet, build, tests, race tests, and Windows amd64 build. CI uses this entry point for both supported Go versions. No required check uses OAuth, provider credentials, a live daemon, production infrastructure, or a paid model call.

Run repository delivery guardrails before landing:

```bash
scripts/check_guardrails.sh
```

## 5. Exercise apply only on a controlled copy

The fixture suite proves exact add/update/remove behavior, file modes, exclusions, repository eligibility, stale fingerprints, malformed/duplicate/unsafe manifests, and zero mutation on refusal. During reconciliation, never pass the live public repository to `apply`.

For a future separately authorized publication, apply the exact reviewed manifest only while both inputs remain unchanged:

```bash
scripts/sync-jcode-go.sh apply \
  --source sdk/go \
  --destination /absolute/path/to/jcode-go \
  --manifest /tmp/jcode-go.manifest
```

Apply must reject changed source or destination fingerprints, dirty/wrong-branch/wrong-repository destinations, malformed manifests, unsafe paths, duplicates, and unsupported versions before the first write.

## 6. Review, validate, and publish

After an authorized apply:

1. inspect every destination diff and confirm protected paths are unchanged;
2. run the public repository's complete formatting, module, vet, build, test, race, and Windows compile checks;
3. confirm the intended semantic version and release notes;
4. commit and push only after review;
5. confirm `origin/main` and the pushed commit;
6. verify the tag/module proxy if a release was authorized;
7. record the public commit and exact validation commands in the owning Bead.

Do not rewrite history or delete a defective published tag. Hold publication and ship a corrective semantic version after review.

## 7. Recheck protected state

Repeat the baseline commands and require exact branch, HEAD (unless an authorized publication intentionally advanced it), tracked paths/modes, clean status, and fingerprint agreement with the expected post-action state. For reconciliation-only work, all values must equal the initial baseline because the live public repository was never mutated.
