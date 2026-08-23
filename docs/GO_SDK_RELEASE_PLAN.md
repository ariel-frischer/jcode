# Go SDK release runbook

**Status:** Current release procedure

The Go SDK is developed and released from [`github.com/ariel-frischer/jcode-go`](https://github.com/ariel-frischer/jcode-go). Branch `dev` is the sole development source. Reviewed semantic-version tags are the public module release boundaries. Jcode does not publish or synchronize an embedded SDK copy.

## 1. Establish the candidate

Record the `jcode-go` checkout path, branch, HEAD, and working-tree state. The release candidate must contain the intended reviewed changes on `dev`. Do not switch or clean a caller-owned checkout implicitly.

## 2. Run the repository-owned Go quality matrix

From `jcode-go`:

```bash
test -z "$(gofmt -l .)"
go mod tidy -diff
go mod verify
go vet ./...
go build ./...
go test ./... -count=1
go test -race ./... -count=1
GOOS=windows GOARCH=amd64 CGO_ENABLED=0 go build ./...
```

CI runs the supported Go 1.23.x and 1.24.x matrix. Linux/macOS use Unix sockets. Windows remains a compile boundary unless separately documented.

## 3. Validate Jcode wire compatibility

From the intended Jcode checkout:

```bash
scripts/validate_jcode_go_compat.sh --jcode-go-dir /absolute/path/to/jcode-go
```

Record both repository revisions and the result. The command must leave both checkout states unchanged.

## 4. Review the release

Before tagging:

1. inspect the complete `jcode-go` diff and public API impact;
2. confirm protocol-v1 compatibility and release notes;
3. choose the next semantic version;
4. require explicit maintainer authorization for the tag and GitHub release;
5. verify the tag, module proxy metadata, origin commit, and checksum after publication.

Do not rewrite or delete a published tag. Ship a corrective version if a released artifact is defective.

## 5. Record evidence

The owning Bead should contain the candidate commit, quality-matrix results, Jcode compatibility result, tag or release URL when authorized, module-proxy verification, and any deferred external boundary.

Historical reconciliation specifications may describe the removed projection workflow. They are point-in-time records, not current release instructions.
