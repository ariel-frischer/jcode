# Go SDK architecture

**Status:** Current state
**Date:** 2026-08-23

## Ownership

There are two implementation authorities with one explicit compatibility boundary:

- [`github.com/ariel-frischer/jcode-go`](https://github.com/ariel-frischer/jcode-go) owns the Go module, exported APIs, protocol decoder, lifecycle behavior, transport, private-runtime supervision, tests, examples, CI, and release tags. Its `dev` branch is the sole development source.
- `crates/jcode-harness-api` owns Jcode server behavior and the serialized protocol-v1 Rust request/event contract.

Jcode does not contain a second Go implementation. CLI output, ACP, copied fixtures, generated projections, vendored source, and terminal parsing are not alternate wire authorities.

## Layering

```text
crates/jcode-harness-api          Rust protocol-v1 wire authority
             |
             | JCODE_REPO_ROOT during read-only parity tests
             v
github.com/ariel-frischer/jcode-go/protocol
             |
      transport + client
             |
        Session + Turn
             |
       launch + process_*
```

The SDK remains dependency-free and externally versioned. Protocol v1 preserves snake-case tags and fields, additive unknown fields, and unknown event kinds. Public lifecycle and error contracts remain owned and tested in `jcode-go`.

## Typed-event compatibility

`ApiEvent::publication_contract` in `crates/jcode-harness-api` classifies Rust events. The `jcode-go` protocol parity suite reads that Rust source through the explicit `JCODE_REPO_ROOT` test environment and checks request/event inventory plus owned-turn decoding and semantic classification.

The Go test process reads its Go implementation from its own module root, not from Jcode. This deliberately separate-root design prevents an embedded Go implementation tree from reappearing.

## Validation boundary

Jcode exposes one compatibility command:

```bash
scripts/validate_jcode_go_compat.sh --jcode-go-dir /absolute/path/to/jcode-go
```

It validates both repository identities and invokes the supplied checkout exactly once with:

```text
JCODE_REPO_ROOT=<resolved Jcode root>
GOWORK=off
go test -mod=readonly -count=1 ./protocol
```

The boundary is read-only. It performs no fetch, checkout, generation, copying, module tidy, staging, commit, or cleanup. Input and tooling errors return status 2. Protocol-test failures return the Go test status with diagnostics preserved. Success returns 0.

The complete Go quality suite, including formatting, module integrity, vet, build, full tests, race tests, and Windows amd64 compilation, remains in `jcode-go` CI.

## Development and release flow

SDK work lands on `jcode-go/dev`. Compatibility is checked against the intended Jcode wire revision. Reviewed version tags define released module versions. There is no publication manifest, projection branch, reverse synchronization path, or second source of truth.
