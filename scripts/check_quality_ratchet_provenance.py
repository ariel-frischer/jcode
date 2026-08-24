#!/usr/bin/env python3
"""Validate ownership and review provenance for reconciled quality ratchets."""

from __future__ import annotations

import json
import re
import subprocess
import sys
from functools import lru_cache
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parent.parent
SCRIPTS = REPO_ROOT / "scripts"
LEDGER_FILE = SCRIPTS / "quality_ratchet_provenance.json"
BUDGET_FILES = {
    "code_size": "scripts/code_size_budget.json",
    "test_size": "scripts/test_size_budget.json",
    "swallowed_error": "scripts/swallowed_error_budget.json",
}
CATEGORIES = frozenset(BUDGET_FILES)
PATTERNS = frozenset({"dot_ok", "let_underscore", "unwrap_or_default"})
DISPOSITIONS = frozenset({"implemented", "rejected_with_evidence", "out_of_scope"})
BEAD_RE = re.compile(r"^[a-z][a-z0-9-]*-[a-z0-9]+(?:\.[0-9]+)*$")
REQUIRED_RECORD_FIELDS = (
    "id",
    "category",
    "scope",
    "previous_value",
    "reconciled_value",
    "baseline_commit",
    "owning_commits",
    "reconciliation_bead",
    "source_bead_search",
    "source_beads",
    "review_disposition_ids",
    "merged_state_evidence",
)
REQUIRED_REVIEW_FIELDS = ("id", "source", "concern", "disposition", "evidence")
REQUIRED_SEARCH_FIELDS = ("query", "status", "limit", "result", "matched_beads")
CFG_TEST_RE = re.compile(r"^\s*#\s*\[\s*cfg\s*\(\s*(?:all\s*\(\s*)?test\s*[,)]")
ITEM_START_RE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:mod|fn)\b")
PATTERN_RES = {
    "let_underscore": re.compile(r"\blet\s+_\s*="),
    "dot_ok": re.compile(r"\.ok\(\)"),
    "unwrap_or_default": re.compile(r"\.unwrap_or_default\(\)"),
}


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        raise ValueError(f"missing required file: {path.relative_to(REPO_ROOT)}") from None
    except json.JSONDecodeError as error:
        raise ValueError(f"invalid JSON in {path.relative_to(REPO_ROOT)}: {error}") from None


def git(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", *args], cwd=REPO_ROOT, text=True, capture_output=True
    )


def git_object_exists(commit: str) -> bool:
    return git("cat-file", "-e", f"{commit}^{{commit}}").returncode == 0


def is_ancestor(commit: str, accepted_state: str) -> bool:
    return git("merge-base", "--is-ancestor", commit, accepted_state).returncode == 0


@lru_cache(maxsize=None)
def commit_parents(commit: str) -> tuple[str, ...]:
    result = git("rev-list", "--parents", "-n", "1", commit)
    if result.returncode != 0:
        return ()
    return tuple(result.stdout.split()[1:])


@lru_cache(maxsize=None)
def git_text(commit: str, path: str) -> str:
    result = git("show", f"{commit}:{path}")
    return result.stdout if result.returncode == 0 else ""


def production_lines(text: str) -> list[str]:
    output: list[str] = []
    skip_stack: list[int] = []
    pending_cfg_test = False
    for line in text.splitlines():
        stripped = line.strip()
        if not skip_stack:
            if pending_cfg_test and ITEM_START_RE.match(line):
                delta = line.count("{") - line.count("}")
                if delta > 0:
                    skip_stack.append(delta)
                pending_cfg_test = False
                continue
            if pending_cfg_test and stripped and not stripped.startswith("#"):
                pending_cfg_test = False
            if CFG_TEST_RE.match(line):
                pending_cfg_test = True
                continue
            output.append(line)
        else:
            skip_stack[-1] += line.count("{") - line.count("}")
            if skip_stack[-1] <= 0:
                skip_stack.pop()
    return output


@lru_cache(maxsize=None)
def swallowed_counts(commit: str, path: str) -> tuple[int, int, int]:
    lines = production_lines(git_text(commit, path))
    return tuple(
        sum(bool(PATTERN_RES[name].search(line)) for line in lines)
        for name in sorted(PATTERNS)
    )


def owning_commit_metric_changes(category: str, scope: str, commit: str) -> set[str]:
    parents = commit_parents(commit) or ("",)
    if category in {"code_size", "test_size"}:
        current = git_text(commit, scope).count("\n")
        return (
            {"loc"}
            if any(current != git_text(parent, scope).count("\n") for parent in parents)
            else set()
        )
    if not scope.startswith("file:"):
        return set()
    path = scope.removeprefix("file:")
    current = swallowed_counts(commit, path)
    changed: set[str] = set()
    pattern_names = sorted(PATTERNS)
    for parent in parents:
        previous = swallowed_counts(parent, path)
        changed.update(
            name for index, name in enumerate(pattern_names) if current[index] != previous[index]
        )
    return changed


def historical_budget(commit: str, relative_path: str) -> Any:
    result = git("show", f"{commit}:{relative_path}")
    if result.returncode != 0:
        raise ValueError(f"baseline commit {commit} does not contain {relative_path}")
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise ValueError(f"invalid historical JSON at {commit}:{relative_path}: {error}") from None


def budget_value(category: str, scope: str, budget: dict[str, Any]) -> Any:
    if category in {"code_size", "test_size"}:
        tracked = budget.get("tracked_files")
        if not isinstance(tracked, dict) or scope not in tracked:
            return None
        return tracked[scope]

    if scope == "aggregate:total":
        return budget.get("total")
    if scope.startswith("pattern:"):
        pattern = scope.removeprefix("pattern:")
        totals = budget.get("totals_by_pattern")
        if pattern not in PATTERNS or not isinstance(totals, dict):
            return None
        return totals.get(pattern)
    if scope.startswith("file:"):
        path = scope.removeprefix("file:")
        tracked = budget.get("tracked_files")
        if not isinstance(tracked, dict):
            return None
        return tracked.get(path, {name: 0 for name in sorted(PATTERNS)})
    return None


def changed_scopes(category: str, previous: dict[str, Any], current: dict[str, Any]) -> set[str]:
    if category in {"code_size", "test_size"}:
        old_files = previous.get("tracked_files", {})
        new_files = current.get("tracked_files", {})
        if not isinstance(old_files, dict) or not isinstance(new_files, dict):
            return set()
        return {
            path
            for path in old_files.keys() | new_files.keys()
            if old_files.get(path) != new_files.get(path)
        }

    scopes: set[str] = set()
    old_files = previous.get("tracked_files", {})
    new_files = current.get("tracked_files", {})
    if isinstance(old_files, dict) and isinstance(new_files, dict):
        scopes.update(
            f"file:{path}"
            for path in old_files.keys() | new_files.keys()
            if old_files.get(path, {name: 0 for name in PATTERNS})
            != new_files.get(path, {name: 0 for name in PATTERNS})
        )
    old_patterns = previous.get("totals_by_pattern", {})
    new_patterns = current.get("totals_by_pattern", {})
    if isinstance(old_patterns, dict) and isinstance(new_patterns, dict):
        scopes.update(
            f"pattern:{name}"
            for name in PATTERNS
            if old_patterns.get(name) != new_patterns.get(name)
        )
    if previous.get("total") != current.get("total"):
        scopes.add("aggregate:total")
    return scopes


def nonempty_string(value: Any) -> bool:
    return isinstance(value, str) and bool(value.strip())


def validate_review_dispositions(ledger: dict[str, Any], errors: list[str]) -> set[str]:
    dispositions = ledger.get("review_dispositions")
    if not isinstance(dispositions, list):
        errors.append("ledger: review_dispositions must be a list")
        return set()
    ids: set[str] = set()
    for index, disposition in enumerate(dispositions):
        label = f"review_dispositions[{index}]"
        if not isinstance(disposition, dict):
            errors.append(f"{label}: must be an object")
            continue
        for field in REQUIRED_REVIEW_FIELDS:
            if field not in disposition:
                errors.append(f"{label}: missing required field: {field}")
        disposition_id = disposition.get("id")
        if not nonempty_string(disposition_id):
            errors.append(f"{label}: id must be a non-empty string")
        elif disposition_id in ids:
            errors.append(f"{label}: duplicate review disposition id: {disposition_id}")
        else:
            ids.add(disposition_id)
        if disposition.get("disposition") not in DISPOSITIONS:
            errors.append(f"{label}: unresolved review disposition: {disposition.get('disposition')!r}")
        for field in ("source", "concern", "evidence"):
            if field in disposition and not nonempty_string(disposition[field]):
                errors.append(f"{label}: {field} must be a non-empty string")
    return ids


def validate_search(record: dict[str, Any], label: str, errors: list[str]) -> None:
    search = record.get("source_bead_search")
    if not isinstance(search, dict) or any(field not in search for field in REQUIRED_SEARCH_FIELDS):
        errors.append(f"{label}: incomplete source_bead_search")
        return
    if not nonempty_string(search["query"]) or search["status"] != "all":
        errors.append(f"{label}: source_bead_search must record a query and status=all")
    if not isinstance(search["limit"], int) or not 1 <= search["limit"] <= 20:
        errors.append(f"{label}: source_bead_search limit must be between 1 and 20")
    matches = search["matched_beads"]
    source_beads = record.get("source_beads")
    if not isinstance(matches, list) or not isinstance(source_beads, list):
        errors.append(f"{label}: source_beads and matched_beads must be lists")
        return
    if sorted(source_beads) != sorted(matches):
        errors.append(f"{label}: source_beads do not match bounded search evidence")
    result = search["result"]
    if result == "not_found" and matches:
        errors.append(f"{label}: not_found search cannot contain matched_beads")
    elif result == "found" and not matches:
        errors.append(f"{label}: found search must contain matched_beads")
    elif result not in {"found", "not_found"}:
        errors.append(f"{label}: source_bead_search result must be found or not_found")
    for bead in source_beads:
        if not isinstance(bead, str) or not BEAD_RE.fullmatch(bead):
            errors.append(f"{label}: invalid source Bead identifier: {bead!r}")


def validate() -> list[str]:
    errors: list[str] = []
    try:
        ledger = load_json(LEDGER_FILE)
    except ValueError as error:
        return [str(error)]
    if not isinstance(ledger, dict):
        return ["ledger: top-level value must be an object"]
    if ledger.get("version") != 1:
        errors.append("ledger: version must be 1")
    accepted_state = ledger.get("accepted_merged_state")
    if not nonempty_string(accepted_state) or not git_object_exists(accepted_state):
        errors.append(f"ledger: accepted_merged_state does not name an existing commit: {accepted_state!r}")
        accepted_state = "HEAD"
    reconciliation_bead = ledger.get("reconciliation_bead")
    if not isinstance(reconciliation_bead, str) or not BEAD_RE.fullmatch(reconciliation_bead):
        errors.append(f"ledger: invalid reconciliation_bead: {reconciliation_bead!r}")

    review_ids = validate_review_dispositions(ledger, errors)
    records = ledger.get("records")
    if not isinstance(records, list):
        errors.append("ledger: records must be a list")
        return errors

    current_budgets: dict[str, dict[str, Any]] = {}
    for category, relative_path in BUDGET_FILES.items():
        try:
            budget = load_json(REPO_ROOT / relative_path)
        except ValueError as error:
            errors.append(str(error))
            continue
        if not isinstance(budget, dict):
            errors.append(f"{relative_path}: top-level value must be an object")
            continue
        current_budgets[category] = budget

    ids: set[str] = set()
    scopes: set[tuple[str, str]] = set()
    baselines: dict[str, str] = {}
    ledger_scopes: dict[str, set[str]] = {category: set() for category in CATEGORIES}
    historical_cache: dict[tuple[str, str], dict[str, Any]] = {}
    swallowed_pattern_owners: dict[str, set[str]] = {name: set() for name in PATTERNS}
    swallowed_total_owners: set[str] = set()
    deferred_swallowed_owners: list[tuple[str, str, set[str]]] = []

    for index, record in enumerate(records):
        label = f"records[{index}]"
        if not isinstance(record, dict):
            errors.append(f"{label}: must be an object")
            continue
        record_id = record.get("id", f"index-{index}")
        label = f"record {record_id}"
        for field in REQUIRED_RECORD_FIELDS:
            if field not in record:
                errors.append(f"{label}: missing required field: {field}")
        if not nonempty_string(record_id):
            errors.append(f"{label}: id must be a non-empty string")
        elif record_id in ids:
            errors.append(f"{label}: duplicate record id")
        else:
            ids.add(record_id)

        category = record.get("category")
        scope = record.get("scope")
        if category not in CATEGORIES:
            errors.append(f"{label}: invalid category: {category!r}")
            continue
        if not nonempty_string(scope):
            errors.append(f"{label}: scope must be a non-empty string")
            continue
        key = (category, scope)
        if key in scopes:
            errors.append(f"{label}: duplicate category/scope: {category}/{scope}")
        scopes.add(key)
        ledger_scopes[category].add(scope)

        bead = record.get("reconciliation_bead")
        if not isinstance(bead, str) or not BEAD_RE.fullmatch(bead) or bead != reconciliation_bead:
            errors.append(f"{label}: invalid reconciliation_bead: {bead!r}")
        validate_search(record, label, errors)
        references = record.get("review_disposition_ids")
        if not isinstance(references, list) or not references:
            errors.append(f"{label}: review_disposition_ids must be a non-empty list")
        else:
            for reference in references:
                if reference not in review_ids:
                    errors.append(f"{label}: unresolved review disposition reference: {reference!r}")
        if not nonempty_string(record.get("merged_state_evidence")):
            errors.append(f"{label}: merged_state_evidence must be a non-empty string")

        baseline = record.get("baseline_commit")
        if not nonempty_string(baseline) or not git_object_exists(baseline):
            errors.append(f"{label}: baseline commit does not exist: {baseline!r}")
            continue
        prior_baseline = baselines.setdefault(category, baseline)
        if prior_baseline != baseline:
            errors.append(f"{label}: category uses multiple baseline commits")
        cache_key = (baseline, category)
        if cache_key not in historical_cache:
            try:
                historical = historical_budget(baseline, BUDGET_FILES[category])
            except ValueError as error:
                errors.append(f"{label}: {error}")
                continue
            if not isinstance(historical, dict):
                errors.append(f"{label}: historical budget must be an object")
                continue
            historical_cache[cache_key] = historical
        historical = historical_cache[cache_key]
        previous = budget_value(category, scope, historical)
        current_budget = current_budgets.get(category)
        current = budget_value(category, scope, current_budget or {})
        if current is None:
            errors.append(f"{label}: scope is absent from the reconciled budget: {scope}")
        if record.get("previous_value") != previous:
            errors.append(
                f"{label}: previous_value does not match {baseline}:{BUDGET_FILES[category]} "
                f"for {scope}: expected {previous!r}, got {record.get('previous_value')!r}"
            )
        if record.get("reconciled_value") != current:
            errors.append(
                f"{label}: reconciled_value does not match {BUDGET_FILES[category]} "
                f"for {scope}: expected {current!r}, got {record.get('reconciled_value')!r}"
            )

        owning_commits = record.get("owning_commits")
        if not isinstance(owning_commits, list) or not owning_commits:
            errors.append(f"{label}: owning_commits must be a non-empty list")
        else:
            valid_owners: set[str] = set()
            for commit in owning_commits:
                if not isinstance(commit, str) or not git_object_exists(commit):
                    errors.append(f"{label}: owning commit does not exist: {commit!r}")
                elif not is_ancestor(commit, accepted_state):
                    errors.append(
                        f"{label}: owning commit is not an ancestor of {accepted_state}: {commit}"
                    )
                else:
                    valid_owners.add(commit)
                    if category in {"code_size", "test_size"} or scope.startswith("file:"):
                        changed_patterns = owning_commit_metric_changes(category, scope, commit)
                        if not changed_patterns:
                            errors.append(
                                f"{label}: owning commit does not change the claimed metric: {commit}"
                            )
                        elif category == "swallowed_error":
                            swallowed_total_owners.add(commit)
                            for pattern in changed_patterns:
                                swallowed_pattern_owners[pattern].add(commit)
            if category == "swallowed_error" and (
                scope == "aggregate:total" or scope.startswith("pattern:")
            ):
                deferred_swallowed_owners.append((label, scope, valid_owners))

    for label, scope, owners in deferred_swallowed_owners:
        expected = (
            swallowed_total_owners
            if scope == "aggregate:total"
            else swallowed_pattern_owners.get(scope.removeprefix("pattern:"), set())
        )
        if owners != expected:
            errors.append(
                f"{label}: owning_commits do not match metric-causal file owners for {scope}: "
                f"expected {sorted(expected)!r}, got {sorted(owners)!r}"
            )

    for category, baseline in baselines.items():
        current = current_budgets.get(category)
        historical = historical_cache.get((baseline, category))
        if current is None or historical is None:
            continue
        changed = changed_scopes(category, historical, current)
        missing = sorted(changed - ledger_scopes[category])
        extra = sorted(ledger_scopes[category] - changed)
        for scope in missing:
            errors.append(f"{category}: adjusted budget scope lacks provenance: {scope}")
        for scope in extra:
            errors.append(f"{category}: provenance record does not describe an adjusted scope: {scope}")

    for category in CATEGORIES - baselines.keys():
        errors.append(f"ledger: no provenance records for category: {category}")
    return errors


def main() -> int:
    errors = validate()
    if errors:
        print("Quality-ratchet provenance validation failed:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1
    ledger = load_json(LEDGER_FILE)
    counts = {category: 0 for category in sorted(CATEGORIES)}
    for record in ledger["records"]:
        counts[record["category"]] += 1
    summary = " ".join(f"{category}={count}" for category, count in counts.items())
    print(
        "Quality-ratchet provenance OK: "
        f"records={len(ledger['records'])} reviews={len(ledger['review_dispositions'])} {summary}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
