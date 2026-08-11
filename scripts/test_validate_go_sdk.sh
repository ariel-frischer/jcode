#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
VALIDATE="$ROOT/scripts/validate_go_sdk.sh"
TMP=$(mktemp -d "${TMPDIR:-/tmp}/validate-go-sdk-test.XXXXXX")
trap 'rm -rf "$TMP"' EXIT

fail() {
  echo "test_validate_go_sdk: $*" >&2
  exit 1
}

mkdir -p "$TMP/sdk" "$TMP/bin"
printf 'module github.com/ariel-frischer/jcode-go\n\ngo 1.23\n' >"$TMP/sdk/go.mod"
printf 'package jcode\n' >"$TMP/sdk/sample.go"

cat >"$TMP/bin/gofmt" <<'SH'
#!/usr/bin/env bash
printf 'gofmt\t%s\n' "$*" >>"$COMMAND_LOG"
if [[ ${FAIL_CATEGORY:-} == formatting ]]; then
  exit 1
fi
exit 0
SH

cat >"$TMP/bin/go" <<'SH'
#!/usr/bin/env bash
printf 'go\tGOOS=%s\tGOARCH=%s\t%s\n' "${GOOS:-}" "${GOARCH:-}" "$*" >>"$COMMAND_LOG"
category=
case $* in
  'mod tidy -diff'|'mod verify') category=module-consistency ;;
  'vet ./...') category=vet ;;
  'build ./...')
    if [[ ${GOOS:-} == windows && ${GOARCH:-} == amd64 ]]; then
      category=windows-amd64-build
    else
      category=build
    fi
    ;;
  'test ./...') category=tests ;;
  'test -race ./...') category=race-tests ;;
  *) exit 91 ;;
esac
if [[ ${FAIL_CATEGORY:-} == "$category" ]]; then
  exit 1
fi
exit 0
SH
chmod +x "$TMP/bin/gofmt" "$TMP/bin/go"

snapshot() {
  git hash-object "$TMP/sdk/go.mod" "$TMP/sdk/sample.go"
}

run_validator() {
  COMMAND_LOG=$1 FAIL_CATEGORY=${2:-} PATH="$TMP/bin:$PATH" \
    "$VALIDATE" --sdk-dir "$TMP/sdk"
}

before=$(snapshot)
run_validator "$TMP/success.log" >"$TMP/success.out"
test "$before" = "$(snapshot)" || fail "successful validation mutated SDK files"

grep -q $'^gofmt\t.*sample.go$' "$TMP/success.log"
grep -q $'^go\tGOOS=\tGOARCH=\tmod tidy -diff$' "$TMP/success.log"
grep -q $'^go\tGOOS=\tGOARCH=\tmod verify$' "$TMP/success.log"
grep -q $'^go\tGOOS=\tGOARCH=\tvet ./\.\.\.$' "$TMP/success.log"
grep -q $'^go\tGOOS=\tGOARCH=\tbuild ./\.\.\.$' "$TMP/success.log"
grep -q $'^go\tGOOS=\tGOARCH=\ttest ./\.\.\.$' "$TMP/success.log"
grep -q $'^go\tGOOS=\tGOARCH=\ttest -race ./\.\.\.$' "$TMP/success.log"
grep -q $'^go\tGOOS=windows\tGOARCH=amd64\tbuild ./\.\.\.$' "$TMP/success.log"
test "$(grep -c '^\[PASS\]' "$TMP/success.out")" -eq 7 || fail "success summary did not contain seven passes"

for category in formatting module-consistency vet build tests race-tests windows-amd64-build; do
  log="$TMP/fail-$category.log"
  output="$TMP/fail-$category.out"
  if run_validator "$log" "$category" >"$output" 2>&1; then
    fail "$category failure unexpectedly succeeded"
  fi
  grep -q "^\[FAIL\] $category" "$output" || fail "$category failure was not visible"
  grep -q $'^go\tGOOS=windows\tGOARCH=amd64\tbuild ./\.\.\.$' "$log" || fail "$category stopped before the final category"
  test "$before" = "$(snapshot)" || fail "$category failure mutated SDK files"
done

echo "test_validate_go_sdk: PASS"
