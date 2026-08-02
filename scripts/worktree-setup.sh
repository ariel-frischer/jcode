#!/usr/bin/env bash
set -euo pipefail

BRANCH="${1:?Usage: worktree-setup.sh <branch-name> [base-branch]}"
BASE="${2:-HEAD}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKTREE_DIR="$REPO_ROOT/.worktrees/$BRANCH"

sync_agent_context() {
  local src_root="$1"
  local dst_root="$2"
  local path

  for path in skills .agents .opencode .claude; do
    [ -e "$src_root/$path" ] || continue
    if [ -e "$dst_root/$path" ]; then
      echo "agent context exists, skipping: $path" >&2
      continue
    fi

    echo "syncing agent context: $path" >&2
    if command -v rsync >/dev/null 2>&1; then
      rsync -a \
        --exclude '.git' \
        --exclude '.beads' \
        --exclude '.env' \
        --exclude '.env.*' \
        --exclude 'node_modules' \
        --exclude '.venv' \
        --exclude 'venv' \
        --exclude 'dist' \
        --exclude 'build' \
        --exclude 'target' \
        --exclude '.cache' \
        --exclude '.pytest_cache' \
        --exclude '.mypy_cache' \
        --exclude '.ruff_cache' \
        "$src_root/$path" "$dst_root/"
    else
      tar -C "$src_root" \
        --exclude='.git' \
        --exclude='.beads' \
        --exclude='.env' \
        --exclude='.env.*' \
        --exclude='node_modules' \
        --exclude='.venv' \
        --exclude='venv' \
        --exclude='dist' \
        --exclude='build' \
        --exclude='target' \
        --exclude='.cache' \
        --exclude='.pytest_cache' \
        --exclude='.mypy_cache' \
        --exclude='.ruff_cache' \
        -cf - "$path" | tar -C "$dst_root" -xf -
    fi
  done
}

link_shared_path() {
  local name="$1"
  local src="$REPO_ROOT/$name"
  local dst="$WORKTREE_DIR/$name"

  [ -e "$src" ] || return 0
  if [ -e "$dst" ] || [ -L "$dst" ]; then
    echo "shared path exists, preserving: $dst" >&2
    return 0
  fi

  echo "linking shared path: $name" >&2
  ln -s "$src" "$dst"
}

exclude_local_paths() {
  local exclude_file
  exclude_file="$(git -C "$WORKTREE_DIR" rev-parse --git-path info/exclude)"
  mkdir -p "$(dirname "$exclude_file")"
  grep -qxF '.beads' "$exclude_file" 2>/dev/null || printf '\n.beads\n' >>"$exclude_file"
  grep -qxF '.codegraph' "$exclude_file" 2>/dev/null || printf '.codegraph\n' >>"$exclude_file"
}

trust_mise_config() {
  [ -f "$WORKTREE_DIR/mise.toml" ] || return 0
  command -v mise >/dev/null 2>&1 || return 0
  echo "trusting worktree mise config" >&2
  mise trust --yes "$WORKTREE_DIR/mise.toml" >/dev/null
}

cd "$REPO_ROOT"
mkdir -p "$(dirname "$WORKTREE_DIR")"

if [ ! -d "$WORKTREE_DIR" ]; then
  git worktree add "$WORKTREE_DIR" -b "$BRANCH" "$BASE" 2>/dev/null || \
    git worktree add "$WORKTREE_DIR" "$BRANCH"
fi

link_shared_path .beads
link_shared_path .codegraph
exclude_local_paths
sync_agent_context "$REPO_ROOT" "$WORKTREE_DIR"
trust_mise_config

printf '%s\n' "$(cd "$WORKTREE_DIR" && pwd)"
