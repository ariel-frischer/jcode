# Go SDK CI and Release Plan

## Decision and module ownership

The SDK remains in this repository under `sdk/go/` as the independently versioned Go module:

```text
module github.com/1jehuang/jcode-go
```

Publishing is a separate human-approved release action. No module is published by CI automatically.

## Supported matrix

| Go | Linux | macOS | Windows |
| --- | --- | --- | --- |
| 1.23.x | test and race | test before release | compile boundary only |
| 1.24.x | test and race | test before release | compile boundary only |

The v1 transport contract supports Linux and macOS Unix-domain sockets. Windows has no supported production transport in v1, but the package must compile and return `transport.ErrUnsupported` from `transport.UnixSocket`.

## Required CI gates

The `go-sdk` job in `.github/workflows/ci.yml` runs on every push and pull request. It performs:

- `gofmt` cleanliness
- `go mod verify`
- `go build ./...`
- `go test ./...`
- `go test -race ./...`
- `go vet ./...`
- Windows cross-compilation on the latest supported Go version

All checks are local and deterministic. They do not require provider credentials, a live daemon, or a paid model call. Protocol parity tests read the checked-in Rust schema source and are supplemented by fake-server tests.

## Version and compatibility policy

- The first public SDK release is expected to use semantic version `v0.1.0` after a human release review.
- The Go module path is immutable. A breaking public API or protocol-major contract requires a new major module path or an explicitly approved migration.
- Protocol v1 accepts server major version 1 and additive minor fields/events. Unknown event kinds are preserved as `UnknownEvent`; unknown major versions are rejected.
- Release notes must identify the minimum Go version, supported OS transports, protocol compatibility, and any known limitations.
- CI must never publish to the public module proxy or mutate production release channels.

## Release checklist

### Pre-release

- [ ] All implementation and validation jcode-zqc beads are complete and closed.
- [ ] `cd sdk/go && go test ./...` passes.
- [ ] `cd sdk/go && go test -race ./...` passes.
- [ ] `cd sdk/go && gofmt -l .` is empty.
- [ ] `cd sdk/go && go vet ./...` passes.
- [ ] `cd sdk/go && go mod verify` passes.
- [ ] Linux and macOS Unix-socket checks pass.
- [ ] Windows compile-boundary check passes.
- [ ] Connect, launch cleanup, reconnect, cancellation, malformed-frame, and parity tests pass.
- [ ] Documentation examples compile.
- [ ] Security review confirms credential inheritance and logging behavior.

### Human publication gate

- [ ] Maintainer approves the release scope and version.
- [ ] Release notes and changelog entry are reviewed.
- [ ] The tag `sdk/go/v0.1.0` is created only after approval.
- [ ] The tag is pushed and module proxy availability is verified manually.
- [ ] No credentials, prompts, or sensitive payloads appear in release artifacts or logs.

### Rollback

Do not delete tags or rewrite history. If a release is found to be defective, hold subsequent publication, document the issue, and ship a corrective semantic version after review. Existing source remains available for inspection.

## Ownership and security review

The Jcode maintainers own the module and release process. Changes to protocol tags, credential inheritance, process cleanup, transport support, or public error codes require maintainer review. Never enable credential inheritance or log raw prompts and tokens implicitly in examples or tests.
