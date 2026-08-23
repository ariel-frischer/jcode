#!/usr/bin/env bash
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
VALIDATOR="$ROOT/scripts/validate_jcode_go_compat.sh"
TMP=$(mktemp -d "${TMPDIR:-/tmp}/jcode-go-compat-test.XXXXXX")
trap 'rm -rf "$TMP"' EXIT

fail() {
  echo "test_validate_jcode_go_compat: $*" >&2
  exit 1
}

expect_status() {
  expected=$1
  shift
  set +e
  "$@" >"$TMP/stdout" 2>"$TMP/stderr"
  actual=$?
  set -e
  [ "$actual" -eq "$expected" ] || fail "status=$actual, want $expected: $*"
}

init_repo() {
  dir=$1
  mkdir -p "$dir"
  git -C "$dir" init -q
  git -C "$dir" config user.email fixture@example.com
  git -C "$dir" config user.name Fixture
}

commit_all() {
  dir=$1
  git -C "$dir" add .
  git -C "$dir" commit -qm fixture
}

fingerprint() {
  dir=$1
  {
    git -C "$dir" rev-parse HEAD
    git -C "$dir" symbolic-ref -q HEAD || true
    git -C "$dir" show-ref || true
    git -C "$dir" ls-files -s
    git -C "$dir" status --porcelain=v1 --untracked-files=all
    find "$dir" -path "$dir/.git" -prune -o -type f -print | LC_ALL=C sort | while IFS= read -r file; do
      printf '%s  ' "${file#"$dir"/}"
      sha256sum "$file" | cut -d' ' -f1
    done
  } | sha256sum | cut -d' ' -f1
}

JCODE="$TMP/Jcode fixture"
SDK="$TMP/jcode go fixture"
BIN="$TMP/bin"
LOG="$TMP/go.log"
init_repo "$JCODE"
mkdir -p "$JCODE/scripts" "$JCODE/crates/jcode-harness-api"
printf '[workspace]\nmembers = []\n' >"$JCODE/Cargo.toml"
cp "$VALIDATOR" "$JCODE/scripts/validate_jcode_go_compat.sh"
chmod +x "$JCODE/scripts/validate_jcode_go_compat.sh"
commit_all "$JCODE"

init_repo "$SDK"
mkdir -p "$SDK/protocol"
printf 'module github.com/ariel-frischer/jcode-go\n\ngo 1.23\n' >"$SDK/go.mod"
printf 'package protocol\n' >"$SDK/protocol/protocol.go"
commit_all "$SDK"
ln -s "$SDK" "$TMP/sdk-link"

mkdir -p "$BIN"
cat >"$BIN/go" <<'SHIM'
#!/usr/bin/env bash
{
  printf 'cwd=%s\n' "$PWD"
  printf 'jcode=%s\n' "${JCODE_REPO_ROOT-}"
  printf 'gowork=%s\n' "${GOWORK-}"
  printf 'args='; printf '<%s>' "$@"; printf '\n'
} >"$GO_SHIM_LOG"
echo "shim diagnostic" >&2
exit "${GO_SHIM_STATUS:-0}"
SHIM
chmod +x "$BIN/go"
export PATH="$BIN:$PATH" GO_SHIM_LOG="$LOG"

expect_status 2 "$JCODE/scripts/validate_jcode_go_compat.sh"
expect_status 2 "$JCODE/scripts/validate_jcode_go_compat.sh" --unknown "$SDK"
expect_status 2 "$JCODE/scripts/validate_jcode_go_compat.sh" --jcode-go-dir "$TMP/missing"
NO_GO_BIN="$TMP/no-go-bin"
mkdir -p "$NO_GO_BIN"
ln -s "$(command -v git)" "$NO_GO_BIN/git"
expect_status 2 env PATH="$NO_GO_BIN" /bin/bash "$JCODE/scripts/validate_jcode_go_compat.sh" --jcode-go-dir "$SDK"
grep -F 'go is required' "$TMP/stderr" >/dev/null || fail "missing Go diagnostic was not actionable"
mkdir "$TMP/not-git"
expect_status 2 "$JCODE/scripts/validate_jcode_go_compat.sh" --jcode-go-dir "$TMP/not-git"

WRONG="$TMP/wrong-module"
init_repo "$WRONG"
mkdir -p "$WRONG/protocol"
printf 'module example.com/wrong\n\ngo 1.23\n' >"$WRONG/go.mod"
commit_all "$WRONG"
expect_status 2 "$JCODE/scripts/validate_jcode_go_compat.sh" --jcode-go-dir "$WRONG"

NO_PROTOCOL="$TMP/no-protocol"
init_repo "$NO_PROTOCOL"
printf 'module github.com/ariel-frischer/jcode-go\n\ngo 1.23\n' >"$NO_PROTOCOL/go.mod"
commit_all "$NO_PROTOCOL"
expect_status 2 "$JCODE/scripts/validate_jcode_go_compat.sh" --jcode-go-dir "$NO_PROTOCOL"

INVALID_JCODE="$TMP/invalid-jcode"
init_repo "$INVALID_JCODE"
mkdir -p "$INVALID_JCODE/scripts"
cp "$VALIDATOR" "$INVALID_JCODE/scripts/validate_jcode_go_compat.sh"
expect_status 2 "$INVALID_JCODE/scripts/validate_jcode_go_compat.sh" --jcode-go-dir "$SDK"

printf 'preexisting dirty state\n' >>"$JCODE/Cargo.toml"
printf 'untracked jcode\n' >"$JCODE/local note.txt"
printf 'preexisting dirty state\n' >>"$SDK/protocol/protocol.go"
printf 'untracked sdk\n' >"$SDK/local note.txt"
JCODE_BEFORE=$(fingerprint "$JCODE")
SDK_BEFORE=$(fingerprint "$SDK")

export GO_SHIM_STATUS=0
expect_status 0 "$JCODE/scripts/validate_jcode_go_compat.sh" --jcode-go-dir "$TMP/sdk-link"
grep -Fx "cwd=$SDK" "$LOG" >/dev/null || fail "shim cwd mismatch"
grep -Fx "jcode=$JCODE" "$LOG" >/dev/null || fail "JCODE_REPO_ROOT mismatch"
grep -Fx 'gowork=off' "$LOG" >/dev/null || fail "GOWORK mismatch"
grep -Fx 'args=<test><-mod=readonly><-count=1><./protocol>' "$LOG" >/dev/null || fail "go arguments mismatch"
[ "$(fingerprint "$JCODE")" = "$JCODE_BEFORE" ] || fail "success mutated Jcode fixture"
[ "$(fingerprint "$SDK")" = "$SDK_BEFORE" ] || fail "success mutated jcode-go fixture"

export GO_SHIM_STATUS=1
expect_status 1 "$JCODE/scripts/validate_jcode_go_compat.sh" --jcode-go-dir "$SDK"
grep -F 'shim diagnostic' "$TMP/stderr" >/dev/null || fail "parity diagnostic was not preserved"
grep -F 'protocol compatibility failed' "$TMP/stderr" >/dev/null || fail "failure summary missing"
[ "$(fingerprint "$JCODE")" = "$JCODE_BEFORE" ] || fail "failure mutated Jcode fixture"
[ "$(fingerprint "$SDK")" = "$SDK_BEFORE" ] || fail "failure mutated jcode-go fixture"

echo "test_validate_jcode_go_compat: PASS"
