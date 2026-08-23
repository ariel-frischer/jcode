#!/usr/bin/env python3
"""Fixture tests proving the existing quality ratchets reject future growth."""

from __future__ import annotations

import json
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


SOURCE_ROOT = Path(__file__).resolve().parents[2]


class RatchetFixture:
    def __init__(self, checker_name: str) -> None:
        self.tempdir = tempfile.TemporaryDirectory()
        self.root = Path(self.tempdir.name)
        (self.root / "scripts").mkdir()
        source = SOURCE_ROOT / "scripts" / checker_name
        shutil.copy2(source, self.root / "scripts" / checker_name)
        self.checker_name = checker_name

    def close(self) -> None:
        self.tempdir.cleanup()

    def write(self, relative: str, text: str) -> None:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def write_json(self, relative: str, payload: object) -> None:
        self.write(relative, json.dumps(payload, indent=2) + "\n")

    def run(self) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["python3", f"scripts/{self.checker_name}"],
            cwd=self.root,
            text=True,
            capture_output=True,
        )


class QualityRatchetGrowthTests(unittest.TestCase):
    def test_code_size_accepts_current_value_and_rejects_positive_delta(self) -> None:
        fixture = RatchetFixture("check_code_size_budget.py")
        self.addCleanup(fixture.close)
        fixture.write_json(
            "scripts/code_size_budget.json",
            {"version": 1, "threshold_loc": 3, "tracked_files": {"src/lib.rs": 4}},
        )
        fixture.write("src/lib.rs", "one\ntwo\nthree\nfour\n")
        self.assertEqual(0, fixture.run().returncode)
        fixture.write("src/lib.rs", "one\ntwo\nthree\nfour\nfive\n")
        result = fixture.run()
        self.assertNotEqual(0, result.returncode)
        self.assertIn("oversized file grew", result.stderr)

    def test_test_size_accepts_current_value_and_rejects_positive_delta(self) -> None:
        fixture = RatchetFixture("check_test_size_budget.py")
        self.addCleanup(fixture.close)
        fixture.write_json(
            "scripts/test_size_budget.json",
            {"version": 1, "threshold_loc": 3, "tracked_files": {"tests/case.rs": 4}},
        )
        fixture.write("tests/case.rs", "one\ntwo\nthree\nfour\n")
        self.assertEqual(0, fixture.run().returncode)
        fixture.write("tests/case.rs", "one\ntwo\nthree\nfour\nfive\n")
        result = fixture.run()
        self.assertNotEqual(0, result.returncode)
        self.assertIn("oversized test file grew", result.stderr)

    def swallowed_fixture(self) -> RatchetFixture:
        fixture = RatchetFixture("check_swallowed_error_budget.py")
        self.addCleanup(fixture.close)
        fixture.write_json(
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
        fixture.write(
            "src/lib.rs",
            "fn accepted() {\n    let _ = work();\n    work().ok();\n    value.unwrap_or_default();\n}\n",
        )
        self.assertEqual(0, fixture.run().returncode)
        return fixture

    def test_swallowed_error_rejects_per_pattern_growth(self) -> None:
        fixture = self.swallowed_fixture()
        fixture.write(
            "src/lib.rs",
            "fn growth() {\n    let _ = work();\n    work().ok();\n    other().ok();\n}\n",
        )
        result = fixture.run()
        self.assertNotEqual(0, result.returncode)
        self.assertIn("dot_ok count grew", result.stderr)

    def test_swallowed_error_rejects_per_file_growth(self) -> None:
        fixture = self.swallowed_fixture()
        fixture.write_json(
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
                        "dot_ok": 0,
                        "let_underscore": 1,
                        "unwrap_or_default": 1,
                    }
                },
            },
        )
        fixture.write(
            "src/lib.rs",
            "fn growth() {\n    let _ = work();\n    work().ok();\n    value.unwrap_or_default();\n}\n",
        )
        result = fixture.run()
        self.assertNotEqual(0, result.returncode)
        self.assertIn("swallowed-error-like usage grew: src/lib.rs", result.stderr)

    def test_swallowed_error_rejects_aggregate_growth(self) -> None:
        fixture = self.swallowed_fixture()
        fixture.write_json(
            "scripts/swallowed_error_budget.json",
            {
                "total": 2,
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
        result = fixture.run()
        self.assertNotEqual(0, result.returncode)
        self.assertIn("total swallowed-error-like count grew", result.stderr)


if __name__ == "__main__":
    unittest.main()
