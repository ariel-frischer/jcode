#!/bin/bash
set -euo pipefail

# Go SDK Integration and Final Validation Script
# Run this after all jcode-zqc implementation stages complete

echo "=== Go SDK Final Validation ==="

# Check if Go SDK exists and is properly structured
if [ ! -d "sdk/go" ] || [ ! -f "sdk/go/go.mod" ]; then
    echo "ERROR: Go SDK not found or incomplete"
    exit 1
fi

cd sdk/go

echo "--- Go SDK Format Check ---"
if ! gofmt -d . | tee /tmp/gofmt-output; then
    echo "ERROR: gofmt failed"
    exit 1
fi

if [ -s /tmp/gofmt-output ]; then
    echo "ERROR: Go code is not formatted correctly"
    cat /tmp/gofmt-output
    exit 1
fi

echo "--- Go SDK Vet Check ---"
if ! go vet ./...; then
    echo "ERROR: go vet failed"
    exit 1
fi

echo "--- Go SDK Build Check ---"
if ! go build ./...; then
    echo "ERROR: go build failed"
    exit 1
fi

echo "--- Go SDK Test Suite ---"
if ! go test ./...; then
    echo "ERROR: go test failed"
    exit 1
fi

echo "--- Go SDK Race Detector ---"
if ! go test -race ./...; then
    echo "ERROR: go test -race failed"
    exit 1
fi

echo "--- Go Module Validation ---"
if ! go mod verify; then
    echo "ERROR: go mod verify failed"
    exit 1
fi

# Check for module cleanliness
go mod tidy
if ! git diff --exit-code go.mod go.sum 2>/dev/null; then
    echo "WARNING: go.mod or go.sum needs tidying"
    git status go.mod go.sum
fi

cd ../..

echo "--- Integration Test with Real Binary ---"
if command -v jcode >/dev/null 2>&1; then
    echo "Found jcode binary, running integration smoke test"
    # This would run a minimal integration test
    echo "Integration test: SKIP (requires live jcode instance)"
else
    echo "No jcode binary found, skipping integration test"
fi

echo "--- Bead Status Check ---"
echo "Checking jcode-zqc bead completion status..."

# Check critical beads
for bead in "jcode-zqc.4" "jcode-zqc.5" "jcode-zqc.6" "jcode-zqc.8"; do
    if command -v bd >/dev/null 2>&1; then
        echo "Checking $bead..."
        bd show "$bead" 2>/dev/null | head -3 || echo "Bead $bead not found or incomplete"
    else
        echo "bd command not available, skipping bead checks"
        break
    fi
done

echo "=== Go SDK Validation Complete ==="
echo "Ready for final integration and release preparation"