WORKTREE_SCRIPT ?= scripts/worktree-setup.sh
BASE ?= $(shell git branch --show-current 2>/dev/null || echo HEAD)
DISK_MIN_FREE_BYTES ?=
DISK_CLEAN_MIN_AGE_DAYS ?=

disk_clean_args = $(if $(DISK_CLEAN_MIN_AGE_DAYS),--min-age-days $(DISK_CLEAN_MIN_AGE_DAYS),)

.PHONY: i install install-fast
i: install

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

.PHONY: disk-help disk-report disk-check disk-clean disk-clean-apply
disk-help:
	@python3 scripts/disk_safety.py --help

disk-report:
	@python3 scripts/disk_safety.py report

disk-check:
	@python3 scripts/disk_safety.py check $(if $(DISK_MIN_FREE_BYTES),--min-free-bytes $(DISK_MIN_FREE_BYTES),)

disk-clean:
	@python3 scripts/disk_safety.py clean $(disk_clean_args)

disk-clean-apply:
	@python3 scripts/disk_safety.py clean --apply $(disk_clean_args)
