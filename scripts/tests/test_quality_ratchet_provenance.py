#!/usr/bin/env python3
"""Deterministic tests for the quality-ratchet provenance validator."""

from __future__ import annotations

import copy
import json
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


SOURCE_ROOT = Path(__file__).resolve().parents[2]
VALIDATOR = SOURCE_ROOT / "scripts" / "check_quality_ratchet_provenance.py"


class ProvenanceFixture:
    def __init__(self) -> None:
        self.tempdir = tempfile.TemporaryDirectory()
        self.root = Path(self.tempdir.name)
        (self.root / "scripts").mkdir()
        shutil.copy2(VALIDATOR, self.root / "scripts" / VALIDATOR.name)
        self.git("init", "-q")
        self.git("config", "user.email", "fixture@example.com")
        self.git("config", "user.name", "Fixture")

        self.write_json(
            "scripts/code_size_budget.json",
            {"version": 1, "threshold_loc": 10, "tracked_files": {"src/lib.rs": 12}},
        )
        self.write_json(
            "scripts/test_size_budget.json",
            {"version": 1, "threshold_loc": 10, "tracked_files": {"tests/case.rs": 13}},
        )
        self.write_json(
            "scripts/swallowed_error_budget.json",
            {
                "total": 3,
                "totals_by_pattern": {
                    "dot_ok": 1,
                    "let_underscore": 1,
                    "unwrap_or_default": 1,
                },
                "tracked_files": {
                    "src/lib.rs": {
                        "dot_ok": 1,
                        "let_underscore": 1,
                        "unwrap_or_default": 1,
                    }
                },
            },
        )
        self.git("add", "scripts")
        self.git("commit", "-qm", "baseline")
        self.baseline = self.git("rev-parse", "HEAD").stdout.strip()

        (self.root / "intent.txt").write_text("intentional merged state\n", encoding="utf-8")
        self.git("add", "intent.txt")
        self.git("commit", "-qm", "intentional change")
        self.owner = self.git("rev-parse", "HEAD").stdout.strip()

        self.write_json(
            "scripts/code_size_budget.json",
            {"version": 1, "threshold_loc": 10, "tracked_files": {"src/lib.rs": 14}},
        )
        self.write_json(
            "scripts/test_size_budget.json",
            {"version": 1, "threshold_loc": 10, "tracked_files": {"tests/case.rs": 15}},
        )
        self.write_json(
            "scripts/swallowed_error_budget.json",
            {
                "total": 6,
                "totals_by_pattern": {
                    "dot_ok": 2,
                    "let_underscore": 2,
                    "unwrap_or_default": 2,
                },
                "tracked_files": {
                    "src/lib.rs": {
                        "dot_ok": 2,
                        "let_underscore": 2,
                        "unwrap_or_default": 2,
                    }
                },
            },
        )
        self.ledger = {
            "version": 1,
            "accepted_merged_state": "HEAD",
            "reconciliation_bead": "jcode-l89.5",
            "review_dispositions": [
                {
                    "id": "REVIEW-1",
                    "source": "bead:jcode-l89.5/comment/fixture",
                    "concern": "Fixture changes require complete ownership evidence.",
                    "disposition": "implemented",
                    "evidence": "Fixture budgets and negative tests exercise the contract.",
                }
            ],
            "records": [
                self.record("CODE-1", "code_size", "src/lib.rs", 12, 14),
                self.record("TEST-1", "test_size", "tests/case.rs", 13, 15),
                self.record(
                    "SW-FILE-1",
                    "swallowed_error",
                    "file:src/lib.rs",
                    {"dot_ok": 1, "let_underscore": 1, "unwrap_or_default": 1},
                    {"dot_ok": 2, "let_underscore": 2, "unwrap_or_default": 2},
                ),
                self.record("SW-PATTERN-1", "swallowed_error", "pattern:dot_ok", 1, 2),
                self.record("SW-PATTERN-2", "swallowed_error", "pattern:let_underscore", 1, 2),
                self.record("SW-PATTERN-3", "swallowed_error", "pattern:unwrap_or_default", 1, 2),
                self.record("SW-TOTAL-1", "swallowed_error", "aggregate:total", 3, 6),
            ],
        }

    def close(self) -> None:
        self.tempdir.cleanup()

    def git(self, *args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["git", *args], cwd=self.root, text=True, capture_output=True, check=check
        )

    def write_json(self, relative: str, payload: object) -> None:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")

    def record(
        self, record_id: str, category: str, scope: str, previous: object, reconciled: object
    ) -> dict[str, object]:
        return {
            "id": record_id,
            "category": category,
            "scope": scope,
            "previous_value": previous,
            "reconciled_value": reconciled,
            "baseline_commit": self.baseline,
            "owning_commits": [self.owner],
            "reconciliation_bead": "jcode-l89.5",
            "source_bead_search": {
                "query": "fixture ownership",
                "status": "all",
                "limit": 5,
                "result": "not_found",
                "matched_beads": [],
            },
            "source_beads": [],
            "review_disposition_ids": ["REVIEW-1"],
            "merged_state_evidence": "The fixture commit is an ancestor of HEAD and its budget values are active.",
        }

    def run(self, ledger: dict[str, object] | None = None) -> subprocess.CompletedProcess[str]:
        self.write_json("scripts/quality_ratchet_provenance.json", ledger or self.ledger)
        return subprocess.run(
            ["python3", "scripts/check_quality_ratchet_provenance.py"],
            cwd=self.root,
            text=True,
            capture_output=True,
        )


class QualityRatchetProvenanceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.fixture = ProvenanceFixture()

    def tearDown(self) -> None:
        self.fixture.close()

    def assert_rejected(self, ledger: dict[str, object], message: str) -> None:
        result = self.fixture.run(ledger)
        self.assertNotEqual(0, result.returncode, result.stdout)
        self.assertIn(message, result.stderr)

    def test_complete_records_and_review_dispositions_pass(self) -> None:
        result = self.fixture.run()
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertIn("Quality-ratchet provenance OK", result.stdout)

    def test_missing_required_field_is_rejected(self) -> None:
        ledger = copy.deepcopy(self.fixture.ledger)
        del ledger["records"][0]["merged_state_evidence"]
        self.assert_rejected(ledger, "missing required field: merged_state_evidence")

    def test_duplicate_category_and_scope_is_rejected(self) -> None:
        ledger = copy.deepcopy(self.fixture.ledger)
        duplicate = copy.deepcopy(ledger["records"][0])
        duplicate["id"] = "CODE-DUPLICATE"
        ledger["records"].append(duplicate)
        self.assert_rejected(ledger, "duplicate category/scope")

    def test_mismatched_previous_and_reconciled_values_are_rejected(self) -> None:
        for field, message in (
            ("previous_value", "previous_value does not match"),
            ("reconciled_value", "reconciled_value does not match"),
        ):
            with self.subTest(field=field):
                ledger = copy.deepcopy(self.fixture.ledger)
                ledger["records"][0][field] = 999
                self.assert_rejected(ledger, message)

    def test_missing_budget_membership_is_rejected(self) -> None:
        ledger = copy.deepcopy(self.fixture.ledger)
        ledger["records"][0]["scope"] = "src/missing.rs"
        self.assert_rejected(ledger, "scope is absent from the reconciled budget")

    def test_invalid_and_non_ancestor_commits_are_rejected(self) -> None:
        ledger = copy.deepcopy(self.fixture.ledger)
        ledger["records"][0]["owning_commits"] = ["not-a-commit"]
        self.assert_rejected(ledger, "owning commit does not exist")

        self.fixture.git("checkout", "-qb", "side", self.fixture.baseline)
        (self.fixture.root / "side.txt").write_text("side\n", encoding="utf-8")
        self.fixture.git("add", "side.txt")
        self.fixture.git("commit", "-qm", "side commit")
        side_commit = self.fixture.git("rev-parse", "HEAD").stdout.strip()
        self.fixture.git("checkout", "-q", "master")
        ledger = copy.deepcopy(self.fixture.ledger)
        ledger["records"][0]["owning_commits"] = [side_commit]
        self.assert_rejected(ledger, "owning commit is not an ancestor")

    def test_invalid_reconciliation_bead_is_rejected(self) -> None:
        ledger = copy.deepcopy(self.fixture.ledger)
        ledger["records"][0]["reconciliation_bead"] = "not a bead"
        self.assert_rejected(ledger, "invalid reconciliation_bead")

    def test_fabricated_source_bead_ownership_is_rejected(self) -> None:
        ledger = copy.deepcopy(self.fixture.ledger)
        ledger["records"][0]["source_beads"] = ["jcode-fake"]
        self.assert_rejected(ledger, "source_beads do not match bounded search evidence")

    def test_incomplete_bounded_search_is_rejected(self) -> None:
        ledger = copy.deepcopy(self.fixture.ledger)
        del ledger["records"][0]["source_bead_search"]["limit"]
        self.assert_rejected(ledger, "incomplete source_bead_search")

    def test_unresolved_review_disposition_is_rejected(self) -> None:
        ledger = copy.deepcopy(self.fixture.ledger)
        ledger["review_dispositions"][0]["disposition"] = "pending"
        self.assert_rejected(ledger, "unresolved review disposition")


if __name__ == "__main__":
    unittest.main()
