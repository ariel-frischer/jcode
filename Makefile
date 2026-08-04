WORKTREE_SCRIPT ?= scripts/worktree-setup.sh
BASE ?= $(shell git branch --show-current 2>/dev/null || echo HEAD)

.PHONY: install install-fast
install:
	@scripts/install_release.sh

install-fast:
	@scripts/install_release.sh --fast

.PHONY: worktree
worktree:
	@test -n "$(BRANCH)" || (echo "BRANCH is required: make worktree BRANCH=agent/name [BASE=$$(git branch --show-current)]" >&2; exit 1)
	@bash "$(WORKTREE_SCRIPT)" "$(BRANCH)" "$(BASE)"

.PHONY: worktree-clean
worktree-clean:
	@repo_root="$$(git rev-parse --show-toplevel)"; \
	git worktree list --porcelain | awk '/^worktree / { sub(/^worktree /, ""); print }' | while read wt; do \
		case "$$wt" in \
			"$$repo_root"/.worktrees/*) echo "removing $$wt"; git worktree remove "$$wt" ;; \
		esac; \
	done
