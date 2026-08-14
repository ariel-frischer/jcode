#!/usr/bin/env python3
"""Foundational contract tests for the standalone session-feedback skill.

These tests intentionally define the dependency-free API that T003-T006 must
implement. They use only synthetic data and isolate HOME and executable lookup
from the developer environment.
"""

from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import stat
from pathlib import Path
import tempfile
import unittest
from unittest import mock


SKILL_DIR = Path(__file__).resolve().parents[1]
HELPER_PATH = SKILL_DIR / "session_feedback.py"
SCHEMA_NAMES = (
    "evidence-v1",
    "proposal-v1",
    "generator-response-v1",
)


def load_helper():
    """Load the copy-local helper without relying on repository import paths."""
    spec = importlib.util.spec_from_file_location("session_feedback_under_test", HELPER_PATH)
    if spec is None or spec.loader is None:
        raise AssertionError(f"unable to load session-feedback helper: {HELPER_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def valid_evidence() -> dict[str, object]:
    return {
        "contract_version": "evidence-v1",
        "source": "fallback",
        "items": [
            {
                "reference": "outcome-1",
                "category": "visible_outcome",
                "summary": "Focused synthetic validation passed.",
            }
        ],
        "accounting": {
            "serialized_bytes": 0,
            "estimated_tokens": 0,
            "by_category": {},
        },
    }


def valid_proposal() -> dict[str, object]:
    return {
        "contract_version": "proposal-v1",
        "target": {
            "category": "skills",
            "scope": "project-jcode",
            "concrete_target": ".jcode/skills/example/SKILL.md",
        },
        "evidence_references": ["outcome-1"],
        "problem": "The synthetic workflow repeats one bounded lookup.",
        "hypothesis": "Reusing the existing receipt will remove the lookup.",
        "suggested_behavior": "Reuse the cited receipt before reading another target.",
        "expected_benefit": "One fewer bounded lookup per invocation.",
        "token_context_impact": "Estimated input decreases by 32 tokens.",
        "risk": "low",
        "blast_radius": "Only the example skill workflow.",
        "validation_plan": ["Run the deterministic unit fixture."],
        "confidence": "high",
        "fingerprint": "0" * 64,
        "non_goals": ["Do not modify the target automatically."],
    }


def valid_generator_response() -> dict[str, object]:
    return {
        "contract_version": "generator-response-v1",
        "proposals": [valid_proposal()],
    }


class IsolatedSessionFeedbackTestCase(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp_dir.cleanup)
        self.home = Path(self.temp_dir.name) / "home"
        self.bin_dir = Path(self.temp_dir.name) / "bin"
        self.home.mkdir()
        self.bin_dir.mkdir()
        self.invocation_log = Path(self.temp_dir.name) / "unexpected-invocation.log"

        for executable in ("jcode", "bd"):
            path = self.bin_dir / executable
            path.write_text(
                "#!/usr/bin/env python3\n"
                "import os\n"
                "from pathlib import Path\n"
                "Path(os.environ['SESSION_FEEDBACK_INVOCATION_LOG']).write_text('called')\n"
                "raise SystemExit(97)\n",
                encoding="utf-8",
            )
            path.chmod(path.stat().st_mode | stat.S_IXUSR)

        self.environment = mock.patch.dict(
            os.environ,
            {
                "HOME": str(self.home),
                "PATH": str(self.bin_dir),
                "SESSION_FEEDBACK_INVOCATION_LOG": str(self.invocation_log),
            },
            clear=False,
        )
        self.environment.start()
        self.addCleanup(self.environment.stop)
        self.network_connection = mock.patch(
            "socket.create_connection",
            side_effect=AssertionError("foundational tests must not access the network"),
        )
        self.network_connection.start()
        self.addCleanup(self.network_connection.stop)
        self.urlopen = mock.patch(
            "urllib.request.urlopen",
            side_effect=AssertionError("foundational tests must not access the network"),
        )
        self.urlopen.start()
        self.addCleanup(self.urlopen.stop)
        self.feedback = load_helper()

    def tearDown(self) -> None:
        self.assertFalse(
            self.invocation_log.exists(),
            "foundational pure tests must not invoke jcode, bd, a provider, or a network path",
        )


class SchemaContractTests(IsolatedSessionFeedbackTestCase):
    def test_loads_all_versioned_schemas_relative_to_the_skill(self) -> None:
        for name in SCHEMA_NAMES:
            with self.subTest(schema=name):
                schema = self.feedback.load_schema(name)
                self.assertEqual(schema["$schema"], "https://json-schema.org/draft/2020-12/schema")
                self.assertFalse(schema["additionalProperties"])

    def test_rejects_unsupported_contract_versions_for_every_schema(self) -> None:
        documents = {
            "evidence-v1": valid_evidence(),
            "proposal-v1": valid_proposal(),
            "generator-response-v1": valid_generator_response(),
        }
        for schema_name, document in documents.items():
            with self.subTest(schema=schema_name):
                document["contract_version"] = f"{schema_name.removesuffix('-v1')}-v2"
                with self.assertRaises(self.feedback.ContractValidationError) as raised:
                    self.feedback.validate_contract(schema_name, document)
                message = str(raised.exception)
                self.assertIn(schema_name, message)
                self.assertIn("/contract_version", message)
                self.assertIn("supported", message.lower())

    def test_rejects_unknown_fields_with_an_actionable_json_pointer(self) -> None:
        document = valid_evidence()
        document["unexpected_payload"] = "must not be accepted"

        with self.assertRaises(self.feedback.ContractValidationError) as raised:
            self.feedback.validate_contract("evidence-v1", document)

        message = str(raised.exception)
        self.assertIn("evidence-v1", message)
        self.assertIn("/unexpected_payload", message)
        self.assertIn("unknown", message.lower())

    def test_canonical_json_is_utf8_stable_and_location_independent(self) -> None:
        value = {"z": [3, 2, 1], "a": {"unicode": "é", "enabled": True}}
        expected = '{"a":{"enabled":true,"unicode":"é"},"z":[3,2,1]}'

        first = self.feedback.canonical_json(value)
        second = self.feedback.canonical_json(value)

        self.assertEqual(first, expected)
        self.assertEqual(second, expected)
        self.assertEqual(first.encode("utf-8"), expected.encode("utf-8"))
        self.assertNotIn(str(SKILL_DIR), first)
        self.assertNotIn(str(self.home), first)


class DeterministicCoreTests(IsolatedSessionFeedbackTestCase):
    def test_normalizes_scope_category_and_concrete_target_identity(self) -> None:
        self.assertEqual(self.feedback.normalize_scope(" Project_Jcode "), "project-jcode")
        self.assertEqual(
            self.feedback.normalize_category(" SDK / Public Surfaces "),
            "sdk-public-surfaces",
        )
        self.assertEqual(
            self.feedback.normalize_concrete_target("  .jcode//skills/example/SKILL.md  "),
            ".jcode/skills/example/SKILL.md",
        )

    def test_measures_exact_utf8_bytes_and_deterministic_labeled_tokens(self) -> None:
        first = self.feedback.measure_text("evidence", "abcdé")
        second = self.feedback.measure_text("evidence", "abcdé")

        self.assertEqual(first, second)
        self.assertEqual(
            first,
            {"label": "evidence", "bytes": 6, "estimated_tokens": 2},
        )
        self.assertEqual(self.feedback.measure_text("empty", "")["estimated_tokens"], 0)

    def test_builds_stable_sha256_fingerprint_from_normalized_material(self) -> None:
        material = {
            "scope": " Project_Jcode ",
            "category": " SDK / Public Surfaces ",
            "concrete_target": " crates/jcode-sdk/src/lib.rs ",
            "problem": "  Duplicated   contract lookup ",
            "intended_outcome": " Reuse ONE canonical contract ",
        }
        normalized = {
            "category": "sdk-public-surfaces",
            "concrete_target": "crates/jcode-sdk/src/lib.rs",
            "intended_outcome": "reuse one canonical contract",
            "problem": "duplicated contract lookup",
            "scope": "project-jcode",
        }
        expected_json = json.dumps(
            normalized,
            ensure_ascii=False,
            sort_keys=True,
            separators=(",", ":"),
        )
        expected = hashlib.sha256(expected_json.encode("utf-8")).hexdigest()

        self.assertEqual(self.feedback.fingerprint_material(material), normalized)
        self.assertEqual(self.feedback.proposal_fingerprint(material), expected)
        self.assertEqual(len(expected), 64)

    def test_normalization_errors_name_the_field_and_invalid_value(self) -> None:
        with self.assertRaises(self.feedback.ValidationError) as raised:
            self.feedback.normalize_scope("team-shared")

        message = str(raised.exception)
        self.assertIn("scope", message.lower())
        self.assertIn("team-shared", message)
        self.assertIn("personal-global", message)
        self.assertIn("project-jcode", message)


if __name__ == "__main__":
    unittest.main()
