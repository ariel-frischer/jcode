#!/usr/bin/env bash
set -euo pipefail

# All repository paths are explicit below. Do not let a parent process redirect
# Git discovery to an unrelated repository or index.
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_COMMON_DIR

fail() {
  echo "sync-jcode-go: $*" >&2
  exit 1
}

usage() {
  cat >&2 <<'EOF'
usage:
  sync-jcode-go.sh [preview] --source SDK_DIR --destination JCODE_GO_DIR
  sync-jcode-go.sh apply --source SDK_DIR --destination JCODE_GO_DIR --manifest FILE

Preview is the default and writes a deterministic reviewed manifest to stdout.
Apply accepts only that unchanged manifest and never targets an ineligible repository.
EOF
}

mode=preview
if [[ ${1:-} == preview || ${1:-} == apply ]]; then
  mode=$1
  shift
elif [[ ${1:-} != --* ]]; then
  usage
  fail "unsupported mode '${1:-}'"
fi

source_dir=
destination_dir=
manifest=
while (( $# > 0 )); do
  case $1 in
    --source)
      (( $# >= 2 )) || fail "--source requires a path"
      source_dir=$2
      shift 2
      ;;
    --destination)
      (( $# >= 2 )) || fail "--destination requires a path"
      destination_dir=$2
      shift 2
      ;;
    --manifest)
      (( $# >= 2 )) || fail "--manifest requires a path"
      manifest=$2
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage
      fail "unknown argument '$1'"
      ;;
  esac
done

[[ -n $source_dir ]] || fail "--source is required"
[[ -n $destination_dir ]] || fail "--destination is required"
[[ -d $source_dir ]] || fail "source directory does not exist: $source_dir"
[[ -d $destination_dir ]] || fail "destination directory does not exist: $destination_dir"
source_dir=$(cd "$source_dir" && pwd -P)
destination_dir=$(cd "$destination_dir" && pwd -P)

[[ -f $source_dir/go.mod ]] || fail "source is missing go.mod"
grep -Eq '^module[[:space:]]+github\.com/ariel-frischer/jcode-go[[:space:]]*$' "$source_dir/go.mod" ||
  fail "source go.mod is not github.com/ariel-frischer/jcode-go"

is_public_jcode_go_remote() {
  case $1 in
    https://github.com/ariel-frischer/jcode-go|https://github.com/ariel-frischer/jcode-go.git|git@github.com:ariel-frischer/jcode-go|git@github.com:ariel-frischer/jcode-go.git|ssh://git@github.com/ariel-frischer/jcode-go|ssh://git@github.com/ariel-frischer/jcode-go.git)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

git -C "$source_dir" rev-parse --is-inside-work-tree >/dev/null 2>&1 ||
  fail "source provenance cannot be established: source must be inside a Git worktree"

source_remotes=$(git -C "$source_dir" remote)
while IFS= read -r remote_name; do
  [[ -n $remote_name ]] || continue
  for remote_mode in fetch push; do
    if [[ $remote_mode == push ]]; then
      remote_urls=$(git -C "$source_dir" remote get-url --all --push "$remote_name" 2>/dev/null || true)
    else
      remote_urls=$(git -C "$source_dir" remote get-url --all "$remote_name" 2>/dev/null || true)
    fi
    while IFS= read -r remote_url; do
      [[ -n $remote_url ]] || continue
      if is_public_jcode_go_remote "$remote_url"; then
        fail "source remote '$remote_name' ($remote_mode) is the public jcode-go repository; reverse synchronization is not allowed"
      fi
    done <<<"$remote_urls"
  done
done <<<"$source_remotes"

git -C "$destination_dir" rev-parse --is-inside-work-tree >/dev/null 2>&1 ||
  fail "destination is not a Git repository"
destination_root=$(git -C "$destination_dir" rev-parse --show-toplevel)
[[ $destination_root == "$destination_dir" ]] || fail "destination must be the repository root"
[[ $(git -C "$destination_dir" branch --show-current) == main ]] || fail "destination must be on branch main"
remote=$(git -C "$destination_dir" remote get-url origin 2>/dev/null || true)
is_public_jcode_go_remote "$remote" || fail "destination origin is not github.com/ariel-frischer/jcode-go"
[[ -z $(git -C "$destination_dir" status --porcelain=v1) ]] || fail "destination worktree must be clean"

if [[ $mode == apply ]]; then
  [[ -n $manifest ]] || fail "apply requires --manifest"
  [[ -f $manifest ]] || fail "manifest does not exist: $manifest"
elif [[ -n $manifest ]]; then
  fail "--manifest is valid only with apply"
fi

tmp=$(mktemp -d "${TMPDIR:-/tmp}/jcode-go-sync.XXXXXX")
trap 'rm -rf "$tmp"' EXIT

is_include_path() {
  case $1 in
    protocol/*|transport/*|examples/*) return 0 ;;
    */*) return 1 ;;
    *.go|go.mod|go.sum|LICENSE|README.md) return 0 ;;
    *) return 1 ;;
  esac
}

protect_rule() {
  case $1 in
    .autospec/*|.beads/*|.worktrees/*|specs/*|AGENTS.md|AGENTS.override.md)
      printf '%s\n' protect-governance
      ;;
    .gitignore|.github/*)
      printf '%s\n' protect-repository-config
      ;;
    docs/*|*.md)
      printf '%s\n' protect-public-docs
      ;;
    *)
      printf '%s\n' protect-unknown
      ;;
  esac
}

file_mode() {
  if [[ -x $1 ]]; then
    printf '%s\n' 100755
  else
    printf '%s\n' 100644
  fi
}

assert_safe_destination_path() {
  local path=$1 current=$destination_dir remainder=$1 component
  while [[ $remainder == */* ]]; do
    component=${remainder%%/*}
    remainder=${remainder#*/}
    current=$current/$component
    [[ ! -L $current ]] || fail "destination path traverses a symlink: $path"
  done
  [[ ! -L $destination_dir/$path ]] || fail "destination path is a symlink: $path"
}

(
  cd "$source_dir"
  find . -maxdepth 1 -type f -print
  for subtree in protocol transport examples; do
    if [[ -d $subtree ]]; then
      find "$subtree" -type f -print
    fi
  done
) | sed 's#^\./##' | LC_ALL=C sort -u >"$tmp/source-candidates"

: >"$tmp/source-paths"
while IFS= read -r path; do
  [[ -n $path ]] || continue
  [[ $path != *$'\t'* && $path != *$'\n'* ]] || fail "source contains an unsafe path"
  if is_include_path "$path"; then
    printf '%s\n' "$path" >>"$tmp/source-paths"
  fi
done <"$tmp/source-candidates"

git -C "$destination_dir" ls-files | LC_ALL=C sort >"$tmp/destination-paths"
while IFS= read -r path; do
  [[ $path != *$'\t'* && $path != *$'\n'* ]] || fail "destination contains an unsafe path"
done <"$tmp/destination-paths"

: >"$tmp/source-records"
while IFS= read -r path; do
  mode_value=$(file_mode "$source_dir/$path")
  digest=$(git hash-object "$source_dir/$path")
  printf '%s\t%s\t%s\n' "$mode_value" "$digest" "$path" >>"$tmp/source-records"
done <"$tmp/source-paths"
source_fingerprint=$(git hash-object "$tmp/source-records")
destination_fingerprint=$(git -C "$destination_dir" ls-files -s -z | git hash-object --stdin)

: >"$tmp/ordered-actions"
while IFS=$'\t' read -r mode_value digest path; do
  assert_safe_destination_path "$path"
  if [[ ! -e $destination_dir/$path ]]; then
    action=add
  else
    destination_digest=$(git hash-object "$destination_dir/$path")
    destination_mode=$(git -C "$destination_dir" ls-files -s -- "$path" | awk '{print $1}')
    if [[ $digest == "$destination_digest" && $mode_value == "$destination_mode" ]]; then
      continue
    fi
    action=update
  fi
  printf '%s\toperation\t%s\t%s\t%s\t%s\n' "$path" "$action" "$mode_value" "$digest" "$path" >>"$tmp/ordered-actions"
done <"$tmp/source-records"

while IFS= read -r path; do
  if is_include_path "$path"; then
    if ! grep -Fqx "$path" "$tmp/source-paths"; then
      destination_mode=$(git -C "$destination_dir" ls-files -s -- "$path" | awk '{print $1}')
      destination_digest=$(git hash-object "$destination_dir/$path")
      printf '%s\toperation\tremove\t%s\t%s\t%s\n' "$path" "$destination_mode" "$destination_digest" "$path" >>"$tmp/ordered-actions"
    fi
  else
    rule=$(protect_rule "$path")
    printf '%s\tretain\t%s\t%s\n' "$path" "$rule" "$path" >>"$tmp/ordered-actions"
  fi
done <"$tmp/destination-paths"

LC_ALL=C sort -t $'\t' -k1,1 "$tmp/ordered-actions" | cut -f2- >"$tmp/actions"

{
  printf '%s\n' jcode-go-sync-manifest-v1
  printf 'source_fingerprint\t%s\n' "$source_fingerprint"
  printf 'destination_fingerprint\t%s\n' "$destination_fingerprint"
  printf 'rule\tinclude-root-sdk\tinclude\troot *.go, go.mod, go.sum, LICENSE, README.md\n'
  printf 'rule\tinclude-protocol\tinclude\tprotocol/**\n'
  printf 'rule\tinclude-transport\tinclude\ttransport/**\n'
  printf 'rule\tinclude-examples\tinclude\texamples/**\n'
  printf 'rule\tprotect-governance\tprotect\t.autospec/** .beads/** .worktrees/** specs/** AGENTS*.md\n'
  printf 'rule\tprotect-repository-config\tprotect\t.git/** .gitignore .github/**\n'
  printf 'rule\tprotect-public-docs\tprotect\tdocs/** root Markdown except README.md\n'
  printf 'rule\tprotect-unknown\tprotect\tunclassified destination paths\n'
  cat "$tmp/actions"
} >"$tmp/generated-manifest"

if [[ $mode == preview ]]; then
  cat "$tmp/generated-manifest"
  exit 0
fi

[[ $(sed -n '1p' "$manifest") == jcode-go-sync-manifest-v1 ]] || fail "manifest format is malformed or unsupported"
if ! cmp -s "$manifest" "$tmp/generated-manifest"; then
  fail "manifest is malformed, unsafe, duplicate, or stale for the current source/destination fingerprints"
fi

while IFS=$'\t' read -r record action mode_value digest path extra; do
  [[ $record == operation ]] || continue
  [[ -z ${extra:-} ]] || fail "manifest operation has extra fields"
  [[ -n $path && $path != /* && $path != .. && $path != ../* && $path != */../* && $path != */.. ]] ||
    fail "manifest contains unsafe operation path"
  case $action in
    add|update)
      mkdir -p "$destination_dir/$(dirname "$path")"
      cp "$source_dir/$path" "$destination_dir/$path"
      case $mode_value in
        100644) chmod 644 "$destination_dir/$path" ;;
        100755) chmod 755 "$destination_dir/$path" ;;
        *) fail "manifest contains unsupported file mode" ;;
      esac
      ;;
    remove)
      rm -f "$destination_dir/$path"
      ;;
    *) fail "manifest contains unsupported operation" ;;
  esac
done <"$manifest"

echo "sync-jcode-go: applied reviewed manifest" >&2
