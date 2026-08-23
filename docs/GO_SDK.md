# Go SDK

**Status:** Current ownership and compatibility boundary

The Go SDK lives only in [`github.com/ariel-frischer/jcode-go`](https://github.com/ariel-frischer/jcode-go).

- `jcode-go` branch `dev` is the sole development source for implementation, public APIs, tests, examples, module metadata, and Go CI.
- Version tags in `jcode-go` are the release boundaries consumed with `go get`.
- `crates/jcode-harness-api` in this repository remains the authoritative Rust protocol-v1 wire contract.
- Jcode contains no embedded Go module, generated projection, vendored copy, submodule, or synchronization workflow.

## Runtime contract

The SDK is an external protocol-v1 client over the harness API. It is not Rust FFI, an ACP client, or a CLI-output parser. Its documented ownership, lifecycle, typed-event, cancellation, redaction, and unknown-event behavior is maintained in the `jcode-go` README and architecture documentation.

## Development

Clone or open the SDK repository directly and work on `dev`:

```bash
git clone https://github.com/ariel-frischer/jcode-go.git
cd jcode-go
git switch dev
```

Other Go modules import `github.com/ariel-frischer/jcode-go`. For local development, a caller may use a `replace` directive pointing to that external checkout. Do not point at a path inside the Jcode repository.

## Validation

The SDK repository owns its full quality matrix:

```bash
gofmt -l .
go mod tidy -diff
go mod verify
go vet ./...
go build ./...
go test ./... -count=1
go test -race ./... -count=1
GOOS=windows GOARCH=amd64 CGO_ENABLED=0 go build ./...
```

Jcode owns one additional read-only wire compatibility check. From this repository:

```bash
scripts/validate_jcode_go_compat.sh --jcode-go-dir /absolute/path/to/jcode-go
```

The command requires an explicit Git checkout, validates its module identity, sets `JCODE_REPO_ROOT` to this Jcode checkout, disables workspaces with `GOWORK=off`, and runs `go test -mod=readonly -count=1 ./protocol` from the supplied SDK root. It never fetches, switches branches, copies files, stages, commits, or cleans either checkout.

## Releases

Changes integrate on `jcode-go/dev`. A reviewed semantic-version tag identifies a released module version. See [`GO_SDK_RELEASE_PLAN.md`](GO_SDK_RELEASE_PLAN.md) for the release gates.
