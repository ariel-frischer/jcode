#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
sdk_dir="$ROOT/sdk/go"

usage() {
  echo "usage: validate_go_sdk.sh [--sdk-dir DIR]" >&2
}

while (( $# > 0 )); do
  case $1 in
    --sdk-dir)
      (( $# >= 2 )) || { usage; exit 2; }
      sdk_dir=$2
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage
      echo "validate-go-sdk: unknown argument '$1'" >&2
      exit 2
      ;;
  esac
done

[[ -d $sdk_dir && -f $sdk_dir/go.mod ]] || {
  echo "validate-go-sdk: SDK directory or go.mod is missing: $sdk_dir" >&2
  exit 2
}
sdk_dir=$(cd "$sdk_dir" && pwd -P)
cd "$sdk_dir"

failed_categories=()

run_category() {
  name=$1
  shift
  echo "[RUN] $name"
  if "$@"; then
    echo "[PASS] $name"
  else
    echo "[FAIL] $name"
    failed_categories+=("$name")
  fi
}

check_formatting() {
  result=0
  while IFS= read -r file; do
    output=$(gofmt -l "$file") || result=1
    if [[ -n $output ]]; then
      printf '%s\n' "$output"
      result=1
    fi
  done < <(find . -type f -name '*.go' -print | LC_ALL=C sort)
  return "$result"
}

check_module_consistency() {
  result=0
  go mod tidy -diff || result=1
  go mod verify || result=1
  return "$result"
}

check_vet() { go vet ./...; }
check_build() { go build ./...; }
check_tests() { go test ./...; }
check_race_tests() { go test -race ./...; }
check_windows_build() { GOOS=windows GOARCH=amd64 go build ./...; }

run_category formatting check_formatting
run_category module-consistency check_module_consistency
run_category vet check_vet
run_category build check_build
run_category tests check_tests
run_category race-tests check_race_tests
run_category windows-amd64-build check_windows_build

echo "Go SDK validation summary: $((7 - ${#failed_categories[@]})) passed, ${#failed_categories[@]} failed"
if (( ${#failed_categories[@]} > 0 )); then
  printf 'Failed categories:'
  printf ' %s' "${failed_categories[@]}"
  printf '\n'
  exit 1
fi

echo "Go SDK validation: PASS"
