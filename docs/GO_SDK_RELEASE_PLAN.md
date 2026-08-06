# Go SDK CI and Release Integration

## CI Pipeline Extension

Add Go SDK checks to the existing CI workflow:

```yaml
# Addition to .github/workflows/ci.yml
  go-sdk:
    name: Go SDK Quality
    runs-on: ubuntu-latest
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@v4
        with:
          ssh-key: ${{ secrets.DEPLOY_KEY }}
          submodules: recursive

      - uses: actions/setup-go@v4
        with:
          go-version: '1.21'  # Pin to supported version
          
      - name: Go SDK format check
        working-directory: sdk/go
        run: |
          if [ ! -f go.mod ]; then
            echo "Go SDK not present, skipping"
            exit 0
          fi
          gofmt -d . | tee /tmp/gofmt-output
          [ ! -s /tmp/gofmt-output ]

      - name: Go SDK vet
        working-directory: sdk/go
        run: |
          if [ ! -f go.mod ]; then exit 0; fi
          go vet ./...

      - name: Go SDK build
        working-directory: sdk/go
        run: |
          if [ ! -f go.mod ]; then exit 0; fi
          go build ./...

      - name: Go SDK test
        working-directory: sdk/go
        run: |
          if [ ! -f go.mod ]; then exit 0; fi
          go test ./...

      - name: Go SDK test with race detector
        working-directory: sdk/go
        run: |
          if [ ! -f go.mod ]; then exit 0; fi
          go test -race ./...

      - name: Go SDK module validation
        working-directory: sdk/go
        run: |
          if [ ! -f go.mod ]; then exit 0; fi
          go mod verify
          go mod tidy
          git diff --exit-code go.mod go.sum
```

## Go Version Matrix

Test against multiple Go versions once implementation stabilizes:
- Go 1.21 (minimum supported)
- Go 1.22 (current stable)
- Go 1.23 (latest)

## Release Checklist Template

```markdown
# Go SDK Release Checklist

## Pre-Release Validation
- [ ] All jcode-zqc beads completed and closed
- [ ] Go SDK tests pass (`go test ./...`)
- [ ] Race detector clean (`go test -race ./...`)
- [ ] Format check passes (`gofmt -d .`)
- [ ] Vet passes (`go vet ./...`)
- [ ] Module validation passes (`go mod verify && go mod tidy`)
- [ ] Examples compile and run
- [ ] Documentation reviewed
- [ ] Protocol compatibility verified with Rust/TypeScript SDKs

## Version and Compatibility
- [ ] Semantic version chosen (e.g., v0.1.0)
- [ ] Protocol version compatibility documented
- [ ] Breaking change policy documented
- [ ] Minimum Go version tested and documented

## Release Artifacts
- [ ] CHANGELOG.md entry added
- [ ] Release notes drafted
- [ ] License files present
- [ ] README.md complete with installation and examples
- [ ] Security review completed

## Publication (Manual Gate)
- [ ] Human approval for publication
- [ ] Module tagged: `git tag sdk/go/v0.1.0`
- [ ] Tag pushed: `git push origin sdk/go/v0.1.0`
- [ ] Verify module proxy availability
- [ ] Update main documentation to reference Go SDK
- [ ] Announce in appropriate channels

## Post-Release
- [ ] Monitor for initial adoption issues
- [ ] Update integration documentation
- [ ] Plan next iteration based on feedback
```

## Module Organization

Keep Go SDK in monorepo under `sdk/go/` with module path:
```
module github.com/1jehuang/jcode/sdk/go
```

This allows:
- Shared CI and tooling
- Coordinated releases with main project
- Cross-SDK compatibility testing
- Unified documentation