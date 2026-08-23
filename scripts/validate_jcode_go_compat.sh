#!/usr/bin/env bash
set -u

usage() {
  echo "usage: validate_jcode_go_compat.sh --jcode-go-dir PATH" >&2
}

fail_usage() {
  echo "validate_jcode_go_compat: $*" >&2
  exit 2
}

[ "$#" -eq 2 ] || { usage; exit 2; }
[ "$1" = "--jcode-go-dir" ] || fail_usage "unknown argument: $1"
[ -n "$2" ] || fail_usage "--jcode-go-dir requires a path"

command -v git >/dev/null 2>&1 || fail_usage "git is required"
command -v go >/dev/null 2>&1 || fail_usage "go is required"

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" 2>/dev/null && pwd -P) || fail_usage "cannot resolve validator directory"
jcode_root=$(CDPATH= cd -- "$script_dir/.." 2>/dev/null && pwd -P) || fail_usage "cannot resolve Jcode checkout"
[ -f "$jcode_root/Cargo.toml" ] || fail_usage "Jcode checkout is missing Cargo.toml: $jcode_root"
[ -d "$jcode_root/crates/jcode-harness-api" ] || fail_usage "Jcode checkout is missing crates/jcode-harness-api: $jcode_root"
git -C "$jcode_root" rev-parse --is-inside-work-tree >/dev/null 2>&1 || fail_usage "Jcode root is not a Git checkout: $jcode_root"

[ -d "$2" ] || fail_usage "jcode-go path is missing or inaccessible: $2"
jcode_go_root=$(CDPATH= cd -- "$2" 2>/dev/null && pwd -P) || fail_usage "cannot resolve jcode-go checkout: $2"
git -C "$jcode_go_root" rev-parse --is-inside-work-tree >/dev/null 2>&1 || fail_usage "jcode-go path is not a Git checkout: $jcode_go_root"
[ -f "$jcode_go_root/go.mod" ] || fail_usage "jcode-go checkout is missing go.mod: $jcode_go_root"
grep -Eq '^module[[:space:]]+github\.com/ariel-frischer/jcode-go[[:space:]]*$' "$jcode_go_root/go.mod" || fail_usage "go.mod is not module github.com/ariel-frischer/jcode-go: $jcode_go_root/go.mod"
[ -d "$jcode_go_root/protocol" ] || fail_usage "jcode-go checkout is missing protocol package: $jcode_go_root"

if (cd "$jcode_go_root" && JCODE_REPO_ROOT="$jcode_root" GOWORK=off go test -mod=readonly -count=1 ./protocol); then
  echo "validate_jcode_go_compat: compatible with $jcode_go_root"
  exit 0
else
  status=$?
  echo "validate_jcode_go_compat: protocol compatibility failed for $jcode_go_root" >&2
  [ "$status" -eq 0 ] && status=1
  exit "$status"
fi
