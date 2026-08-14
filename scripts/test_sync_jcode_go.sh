#!/usr/bin/env bash
set -euo pipefail

unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_COMMON_DIR

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
SYNC="$ROOT/scripts/sync-jcode-go.sh"
TMP=$(mktemp -d "/tmp/jcode-go-sync-test.XXXXXX")
trap 'rm -rf "$TMP"' EXIT

fail() {
  echo "test_sync_jcode_go: $*" >&2
  exit 1
}

expect_failure() {
  label=$1
  shift
  if "$@" >"$TMP/$label.out" 2>"$TMP/$label.err"; then
    fail "$label unexpectedly succeeded"
  fi
  test -s "$TMP/$label.err" || fail "$label produced no diagnostic"
}

expect_failure_with_git_dir() {
  label=$1
  shift
  if GIT_DIR="$TMP/ambient-git-dir" "$@" >"$TMP/$label.out" 2>"$TMP/$label.err"; then
    fail "$label unexpectedly succeeded"
  fi
  test -s "$TMP/$label.err" || fail "$label produced no diagnostic"
}

make_source() {
  dir=$1
  mkdir -p "$dir/protocol" "$dir/transport" "$dir/examples/demo"
  printf 'module github.com/ariel-frischer/jcode-go\n\ngo 1.23\n' >"$dir/go.mod"
  printf 'package jcode\n\nconst Version = "new"\n' >"$dir/client.go"
  printf 'package protocol\n' >"$dir/protocol/event.go"
  printf 'package transport\n' >"$dir/transport/socket.go"
  printf 'package main\nfunc main() {}\n' >"$dir/examples/demo/main.go"
  printf '# Canonical SDK\n' >"$dir/README.md"
  printf 'license\n' >"$dir/LICENSE"
  printf '#!/bin/sh\nexit 0\n' >"$dir/examples/demo/helper.sh"
  chmod 755 "$dir/examples/demo/helper.sh"
  git -C "$dir" init -q -b main
  git -C "$dir" config user.name fixture
  git -C "$dir" config user.email fixture@example.invalid
  git -C "$dir" add .
  git -C "$dir" commit -q -m fixture
}

make_destination() {
  dir=$1
  remote=${2:-https://github.com/ariel-frischer/jcode-go.git}
  mkdir -p "$dir"
  git -C "$dir" init -q -b main
  git -C "$dir" config user.name fixture
  git -C "$dir" config user.email fixture@example.invalid
  git -C "$dir" remote add origin "$remote"
  printf 'module github.com/ariel-frischer/jcode-go\n\ngo 1.23\n' >"$dir/go.mod"
  mkdir -p "$dir/docs" "$dir/specs/kept" "$dir/.autospec"
  printf 'package jcode\n\nconst Version = "old"\n' >"$dir/client.go"
  printf 'package jcode\n' >"$dir/obsolete.go"
  printf '# Public SDK\n' >"$dir/README.md"
  printf 'public docs\n' >"$dir/docs/architecture.md"
  printf 'public spec\n' >"$dir/specs/kept/spec.yaml"
  printf 'public governance\n' >"$dir/.autospec/constitution.yaml"
  printf 'public agents\n' >"$dir/AGENTS.md"
  printf '.beads/\n' >"$dir/.gitignore"
  git -C "$dir" add AGENTS.md .autospec/constitution.yaml .gitignore README.md client.go docs/architecture.md go.mod obsolete.go specs/kept/spec.yaml
  git -C "$dir" commit -q -m fixture
}

status_of() {
  git -C "$1" status --porcelain=v1 --branch
}

make_source "$TMP/source"
make_destination "$TMP/destination"
git init -q "$TMP/ambient-git-dir"

cp -a "$TMP/source" "$TMP/non-git-source"
rm -rf "$TMP/non-git-source/.git"
expect_failure non-git-source "$SYNC" preview --source "$TMP/non-git-source" --destination "$TMP/destination"
expect_failure_with_git_dir ambient-git-dir "$SYNC" preview --source "$TMP/non-git-source" --destination "$TMP/destination"

make_source "$TMP/renamed-origin-source"
git -C "$TMP/renamed-origin-source" remote add origin git@github.com:ariel-frischer/jcode-go.git
git -C "$TMP/renamed-origin-source" remote rename origin upstream
expect_failure renamed-origin-source "$SYNC" preview --source "$TMP/renamed-origin-source" --destination "$TMP/destination"

make_source "$TMP/missing-origin-source"
git -C "$TMP/missing-origin-source" remote add upstream https://github.com/ariel-frischer/jcode-go.git
expect_failure missing-origin-source "$SYNC" preview --source "$TMP/missing-origin-source" --destination "$TMP/destination"

variant_index=0
while IFS= read -r public_remote; do
  variant_source="$TMP/url-variant-$variant_index"
  make_source "$variant_source"
  git -C "$variant_source" remote add upstream "$public_remote"
  expect_failure "url-variant-$variant_index" "$SYNC" preview --source "$variant_source" --destination "$TMP/destination"
  variant_index=$((variant_index + 1))
done <<'EOF'
https://github.com/ariel-frischer/jcode-go.git/
https://mirror@github.com/ariel-frischer/jcode-go/
git@github.com:ariel-frischer/jcode-go.git/
ssh://git@github.com/ariel-frischer/jcode-go/
ssh://deploy@github.com/ariel-frischer/jcode-go.git
EOF

reverse_before=$(status_of "$TMP/destination")
expect_failure reverse-sync "$SYNC" preview --source "$TMP/destination" --destination "$TMP/destination"
test "$reverse_before" = "$(status_of "$TMP/destination")" || fail "reverse-sync rejection mutated destination"

before=$(status_of "$TMP/destination")
"$SYNC" preview --source "$TMP/source" --destination "$TMP/destination" >"$TMP/preview-1.manifest"
"$SYNC" --source "$TMP/source" --destination "$TMP/destination" >"$TMP/preview-2.manifest"
cmp "$TMP/preview-1.manifest" "$TMP/preview-2.manifest"
test "$before" = "$(status_of "$TMP/destination")" || fail "preview mutated destination"

grep -q '^jcode-go-sync-manifest-v1$' "$TMP/preview-1.manifest"
grep -q $'^rule\tinclude-root-sdk\tinclude\t' "$TMP/preview-1.manifest"
grep -q $'^rule\tprotect-governance\tprotect\t' "$TMP/preview-1.manifest"
grep -q $'^operation\tupdate\t100644\t[^\t]*\tclient.go$' "$TMP/preview-1.manifest"
grep -q $'^operation\tremove\t100644\t[^\t]*\tobsolete.go$' "$TMP/preview-1.manifest"
grep -q $'^retain\tprotect-public-docs\tdocs/architecture.md$' "$TMP/preview-1.manifest"
grep -q $'^retain\tprotect-governance\tspecs/kept/spec.yaml$' "$TMP/preview-1.manifest"

expect_failure invalid-mode "$SYNC" nonsense --source "$TMP/source" --destination "$TMP/destination"
expect_failure missing-source "$SYNC" preview --source "$TMP/missing" --destination "$TMP/destination"

make_destination "$TMP/wrong-identity" https://example.invalid/not-jcode-go.git
expect_failure wrong-identity "$SYNC" preview --source "$TMP/source" --destination "$TMP/wrong-identity"

make_destination "$TMP/spoofed-identity" https://evilgithub.com/ariel-frischer/jcode-go.git
expect_failure spoofed-identity "$SYNC" preview --source "$TMP/source" --destination "$TMP/spoofed-identity"

make_destination "$TMP/wrong-branch"
git -C "$TMP/wrong-branch" checkout -q -b other
expect_failure wrong-branch "$SYNC" preview --source "$TMP/source" --destination "$TMP/wrong-branch"

make_destination "$TMP/dirty"
printf 'dirty\n' >"$TMP/dirty/untracked"
dirty_before=$(status_of "$TMP/dirty")
expect_failure dirty-destination "$SYNC" preview --source "$TMP/source" --destination "$TMP/dirty"
test "$dirty_before" = "$(status_of "$TMP/dirty")" || fail "dirty rejection mutated destination"

make_destination "$TMP/symlink-destination"
mkdir -p "$TMP/outside"
ln -s "$TMP/outside" "$TMP/symlink-destination/protocol"
git -C "$TMP/symlink-destination" add protocol
git -C "$TMP/symlink-destination" commit -q -m symlink
expect_failure symlink-destination "$SYNC" preview --source "$TMP/source" --destination "$TMP/symlink-destination"
test ! -e "$TMP/outside/event.go" || fail "symlink rejection wrote outside destination"

printf 'bad\n' >"$TMP/malformed.manifest"
expect_failure malformed "$SYNC" apply --source "$TMP/source" --destination "$TMP/destination" --manifest "$TMP/malformed.manifest"
expect_failure direct-apply "$SYNC" apply --source "$TMP/source" --destination "$TMP/destination"
sed '1s/v1/v2/' "$TMP/preview-1.manifest" >"$TMP/unsupported.manifest"
expect_failure unsupported "$SYNC" apply --source "$TMP/source" --destination "$TMP/destination" --manifest "$TMP/unsupported.manifest"
cp "$TMP/preview-1.manifest" "$TMP/unsafe.manifest"
printf 'operation\tadd\t100644\tdeadbeef\t../escape\n' >>"$TMP/unsafe.manifest"
expect_failure unsafe-path "$SYNC" apply --source "$TMP/source" --destination "$TMP/destination" --manifest "$TMP/unsafe.manifest"
cp "$TMP/preview-1.manifest" "$TMP/duplicate.manifest"
grep $'^operation\t' "$TMP/preview-1.manifest" | head -1 >>"$TMP/duplicate.manifest"
expect_failure duplicate "$SYNC" apply --source "$TMP/source" --destination "$TMP/destination" --manifest "$TMP/duplicate.manifest"

cp "$TMP/source/client.go" "$TMP/client.go.saved"
printf 'package jcode\n\nconst Version = "stale"\n' >"$TMP/source/client.go"
expect_failure stale-source "$SYNC" apply --source "$TMP/source" --destination "$TMP/destination" --manifest "$TMP/preview-1.manifest"
mv "$TMP/client.go.saved" "$TMP/source/client.go"

make_destination "$TMP/stale-destination"
"$SYNC" preview --source "$TMP/source" --destination "$TMP/stale-destination" >"$TMP/stale-destination.manifest"
printf 'changed public docs\n' >"$TMP/stale-destination/docs/architecture.md"
git -C "$TMP/stale-destination" add docs/architecture.md
git -C "$TMP/stale-destination" commit -q -m changed
expect_failure stale-destination "$SYNC" apply --source "$TMP/source" --destination "$TMP/stale-destination" --manifest "$TMP/stale-destination.manifest"

"$SYNC" apply --source "$TMP/source" --destination "$TMP/destination" --manifest "$TMP/preview-1.manifest"
cmp "$TMP/source/client.go" "$TMP/destination/client.go"
cmp "$TMP/source/protocol/event.go" "$TMP/destination/protocol/event.go"
cmp "$TMP/source/transport/socket.go" "$TMP/destination/transport/socket.go"
cmp "$TMP/source/examples/demo/main.go" "$TMP/destination/examples/demo/main.go"
test -x "$TMP/destination/examples/demo/helper.sh" || fail "executable mode was not preserved"
test ! -e "$TMP/destination/obsolete.go" || fail "reviewed removal was not applied"
grep -q 'public docs' "$TMP/destination/docs/architecture.md"
grep -q 'public spec' "$TMP/destination/specs/kept/spec.yaml"
grep -q 'public governance' "$TMP/destination/.autospec/constitution.yaml"
grep -q 'public agents' "$TMP/destination/AGENTS.md"

echo "test_sync_jcode_go: PASS"
