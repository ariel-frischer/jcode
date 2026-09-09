# Main-owned fork releases

The fork release workflow is intentionally triggered only by a push to `main`.
The `dev` branch remains the normal development branch, and no workflow merges
or promotes commits. Ariel chooses each `dev` to `main` promotion.

## Release contract

- `.github/workflows/release.yml` checks out the triggering commit using the exact
  `github.sha` value in every job.
- The prepare job derives a numeric tag as `v<commit-year>.<commit-month>.<run-number>`.
  For example, a September 2026 run numbered 17 becomes `v2026.9.17`.
- The publisher verifies that an existing tag points to the triggering commit.
  A collision fails the workflow rather than moving a tag. Re-running a run
  reuses its deterministic tag. A release that is already public is not changed.
- Release runs are serialized so an older build cannot overwrite the latest
  release after a newer build. GitHub keeps one running and one pending run per
  concurrency group, replacing the pending run if more promotions arrive.
- Publication is draft-first. Linux x86_64, Linux ARM64, macOS ARM64, macOS
  x86_64, Windows x86_64, and Windows ARM64 artifacts must all build and pass
  artifact validation before the draft is created and published.
- Assets retain the existing consumer names and include `SHA256SUMS` plus
  `BUILD-PROVENANCE.txt` with the repository, tag, source SHA, and workflow run.
- Windows artifacts are unsigned CLI binaries. This workflow does not publish
  desktop installers or update package channels.

## First promotion

This change does not create `main`, change the repository default branch, or run a
release. After the workflow lands, create or promote `main` deliberately, then
push the selected commit. A first real run is still required to validate hosted
runner availability, cross-platform toolchains, and the compatibility-container
build.

## Runtime update boundary

The installed binary's default update and source-build paths still point at the
upstream `1jehuang/jcode` repository. This release workflow deliberately does
not change those runtime or installer defaults. Fork users who need automatic
updates from fork releases require a separate, explicit updater and installer
configuration change.
