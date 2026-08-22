#!/usr/bin/env python3
import json
import pathlib
import subprocess
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "lsp_benchmark.py"


class LspBenchmarkTests(unittest.TestCase):
    def test_default_off_and_deterministic_fields(self):
        result = subprocess.run(
            [sys.executable, str(SCRIPT), "--rounds", "3"],
            check=True,
            capture_output=True,
            text=True,
        )
        payload = json.loads(result.stdout)
        self.assertTrue(payload["success"])
        self.assertEqual(payload["rounds"], 3)
        self.assertEqual(payload["diagnostic_count"], 3)
        self.assertEqual(payload["tool_calls"], 3)
        self.assertFalse(payload["lsp_default_enabled"])
        self.assertFalse(payload["network"])


if __name__ == "__main__":
    unittest.main()
