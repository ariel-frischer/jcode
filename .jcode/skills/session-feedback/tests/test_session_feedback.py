#!/usr/bin/env python3
"""Dependency-free tests for the standalone session-feedback skill.

These tests intentionally define the dependency-free API that T003-T006 must
implement. They use only synthetic data and isolate HOME and executable lookup
from the developer environment.
"""

from __future__ import annotations

import copy
import hashlib
import importlib.util
import json
import os
import shutil
import stat
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


SKILL_DIR = Path(__file__).resolve().parents[1]
HELPER_PATH = SKILL_DIR / "session_feedback.py"
ENTRYPOINT_PATH = SKILL_DIR / "__main__.py"
PRIVACY_FIXTURE_PATH = SKILL_DIR / "fixtures" / "privacy-sentinels.json"
GENERATOR_FIXTURE_PATH = SKILL_DIR / "fixtures" / "generator-responses.json"
DEDUP_FIXTURE_PATH = SKILL_DIR / "fixtures" / "dedup-records.json"
CONFIG_FIXTURE_PATH = SKILL_DIR / "fixtures" / "config-precedence.json"
SCHEMA_NAMES = (
    "evidence-v1",
    "proposal-v1",
    "generator-response-v1",
)


def load_helper():
    """Load the copy-local helper without relying on repository import paths."""
    spec = importlib.util.spec_from_file_location(
        "session_feedback_under_test", HELPER_PATH
    )
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


def jcode_json_report(
    text: str, *, input_tokens: int = 321, output_tokens: int = 17
) -> str:
    return json.dumps(
        {
            "text": text,
            "usage": {
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
                "cache_read_input_tokens": 0,
            },
        },
        separators=(",", ":"),
    )


class IsolatedSessionFeedbackTestCase(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp_dir.cleanup)
        self.home = Path(self.temp_dir.name) / "home"
        self.bin_dir = Path(self.temp_dir.name) / "bin"
        self.home.mkdir()
        self.bin_dir.mkdir()
        legacy_auth = self.home / ".codex" / "auth.json"
        legacy_auth.parent.mkdir()
        legacy_auth.write_text(
            json.dumps({"tokens": {"access_token": "synthetic-test-token"}}),
            encoding="utf-8",
        )
        legacy_auth.chmod(0o600)
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
            side_effect=AssertionError(
                "session-feedback tests must not access the network"
            ),
        )
        self.network_connection.start()
        self.addCleanup(self.network_connection.stop)
        self.urlopen = mock.patch(
            "urllib.request.urlopen",
            side_effect=AssertionError(
                "session-feedback tests must not access the network"
            ),
        )
        self.urlopen.start()
        self.addCleanup(self.urlopen.stop)
        self.feedback = load_helper()

    def tearDown(self) -> None:
        self.assertFalse(
            self.invocation_log.exists(),
            "pure tests must not invoke jcode, bd, a provider, or a network path",
        )


class SchemaContractTests(IsolatedSessionFeedbackTestCase):
    def test_loads_all_versioned_schemas_relative_to_the_skill(self) -> None:
        for name in SCHEMA_NAMES:
            with self.subTest(schema=name):
                schema = self.feedback.load_schema(name)
                self.assertEqual(
                    schema["$schema"], "https://json-schema.org/draft/2020-12/schema"
                )
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
        self.assertEqual(
            self.feedback.normalize_scope(" Project_Jcode "), "project-jcode"
        )
        self.assertEqual(
            self.feedback.normalize_category(" SDK / Public Surfaces "),
            "sdk-public-surfaces",
        )
        self.assertEqual(
            self.feedback.normalize_concrete_target(
                "  .jcode//skills/example/SKILL.md  "
            ),
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


class FirstRunFallbackTests(IsolatedSessionFeedbackTestCase):
    def visible_items(self) -> list[dict[str, str]]:
        return [
            {
                "reference": "outcome-1",
                "category": "visible_outcome",
                "summary": "The requested synthetic change was completed.",
            },
            {
                "reference": "tool-1",
                "category": "tool_invocation_receipt",
                "name": "functions.read",
                "outcome": "succeeded",
                "summary": "Read one already-selected synthetic file.",
            },
        ]

    def prepare(self, requested_session_id: str | None = None):
        return self.feedback.prepare_feedback_invocation(
            requested_session_id=requested_session_id,
            current_session_id="session-current-1",
            visible_session_ids=("session-current-1", "session-visible-2"),
            visible_items=self.visible_items(),
            librarian_summary_path=None,
        )

    def test_current_session_uses_supplied_visible_evidence_on_clean_first_run(
        self,
    ) -> None:
        self.assertFalse((self.home / ".jcode" / "feedback").exists())
        self.assertFalse(
            (self.home / ".jcode" / "skills" / "session-librarian").exists()
        )
        self.assertFalse((self.home / ".jcode" / "skills" / "jcode-zor").exists())

        invocation = self.prepare()

        self.assertEqual(invocation["session_id"], "session-current-1")
        self.assertEqual(invocation["evidence"]["source"], "fallback")
        self.assertEqual(invocation["evidence"]["items"], self.visible_items())
        self.feedback.validate_contract("evidence-v1", invocation["evidence"])
        self.assertFalse((self.home / ".jcode" / "feedback").exists())

    def test_valid_named_session_is_selected_and_reported(self) -> None:
        invocation = self.prepare("session-visible-2")

        self.assertEqual(invocation["session_id"], "session-visible-2")
        self.assertEqual(invocation["evidence"]["source"], "fallback")
        self.assertEqual(invocation["evidence"]["items"], self.visible_items())

    def test_missing_current_session_fails_actionably_without_creating_a_run(
        self,
    ) -> None:
        with self.assertRaises(self.feedback.ValidationError) as raised:
            self.feedback.prepare_feedback_invocation(
                requested_session_id=None,
                current_session_id=None,
                visible_session_ids=("session-visible-2",),
                visible_items=self.visible_items(),
                librarian_summary_path=None,
            )

        message = str(raised.exception).lower()
        self.assertIn("current session", message)
        self.assertIn("session id", message)
        self.assertFalse((self.home / ".jcode" / "feedback").exists())

    def test_invisible_named_session_fails_before_generation_or_persistence(
        self,
    ) -> None:
        with self.assertRaises(self.feedback.ValidationError) as raised:
            self.prepare("session-private-9")

        message = str(raised.exception).lower()
        self.assertIn("session-private-9", message)
        self.assertIn("visible", message)
        self.assertFalse(self.invocation_log.exists())
        self.assertFalse((self.home / ".jcode" / "feedback").exists())

    def test_fallback_rejects_non_allowlisted_visible_evidence(self) -> None:
        prohibited = self.visible_items() + [
            {
                "reference": "transcript-1",
                "category": "transcript",
                "summary": "A full transcript must never enter fallback evidence.",
            }
        ]

        with self.assertRaises(self.feedback.ContractValidationError) as raised:
            self.feedback.prepare_feedback_invocation(
                requested_session_id=None,
                current_session_id="session-current-1",
                visible_session_ids=("session-current-1",),
                visible_items=prohibited,
                librarian_summary_path=None,
            )

        self.assertIn("allowlisted", str(raised.exception).lower())
        self.assertFalse((self.home / ".jcode" / "feedback").exists())

    def test_review_outcomes_keep_success_failure_and_persistence_distinct(
        self,
    ) -> None:
        zero = self.feedback.build_review_outcome(
            session_id="session-current-1",
            proposal_locations=(),
        )
        persisted = self.feedback.build_review_outcome(
            session_id="session-current-1",
            proposal_locations=("~/.jcode/feedback/proposals/proposal-1.json",),
        )
        failed = self.feedback.build_review_outcome(
            session_id="session-current-1",
            failure="Synthetic generator output was invalid.",
        )

        self.assertEqual(zero["status"], "zero_proposals")
        self.assertEqual(zero["proposal_count"], 0)
        self.assertEqual(persisted["status"], "proposals_persisted")
        self.assertEqual(persisted["proposal_count"], 1)
        self.assertEqual(failed["status"], "failed")
        self.assertEqual(failed["proposal_count"], 0)
        self.assertIn("invalid", failed["failure"].lower())


class PrivacyNormalizationAndExcerptTests(IsolatedSessionFeedbackTestCase):
    def setUp(self) -> None:
        super().setUp()
        self.fixture = json.loads(PRIVACY_FIXTURE_PATH.read_text(encoding="utf-8"))
        self.session_id = self.fixture["session"]["current_session_id"]
        self.fallback_items = self.fixture["inputs"]["fallback"]["visible_items"]
        self.sentinels = tuple(self.fixture["sentinels"].values())

    def write_librarian_summary(self, document: dict[str, object]) -> Path:
        path = Path(self.temp_dir.name) / "visible-summary.json"
        path.write_text(json.dumps(document), encoding="utf-8")
        return path

    def acquire(
        self,
        *,
        librarian: dict[str, object] | None,
        visible_items: list[dict[str, object]] | None = None,
    ) -> dict[str, object]:
        path = (
            self.write_librarian_summary(librarian) if librarian is not None else None
        )
        return self.feedback.acquire_evidence(
            session_id=self.session_id,
            librarian_summary_path=path,
            visible_items=self.fallback_items
            if visible_items is None
            else visible_items,
        )

    def assert_sentinels_absent(self, value: object, surface: str) -> None:
        serialized = self.feedback.canonical_json(value)
        for sentinel in self.sentinels:
            with self.subTest(surface=surface, sentinel=sentinel):
                self.assertNotIn(sentinel, serialized)

    def test_real_librarian_artifact_normalizes_to_bounded_v1_items(self) -> None:
        librarian = self.acquire(
            librarian=self.fixture["inputs"]["compatible_librarian"]
        )
        fallback = self.acquire(librarian=None)

        self.assertEqual(librarian["contract_version"], "evidence-v1")
        self.assertEqual(fallback["contract_version"], "evidence-v1")
        self.assertEqual(librarian["source"], "librarian")
        self.assertEqual(fallback["source"], "fallback")
        self.assertEqual(
            [item["reference"] for item in librarian["items"]],
            [
                "librarian-goal",
                "librarian-outcome-1",
                "librarian-decision-1",
                "librarian-unresolved-1",
                "librarian-risk-1",
                "librarian-next-step-1",
                "librarian-path-1",
                "librarian-path-2",
            ],
        )
        self.assertEqual(librarian["items"][3]["status"], "pending")
        self.assertEqual(librarian["items"][5]["status"], "pending")
        rendered = self.feedback.canonical_json(librarian)
        self.assertNotIn("gpt-5.6-luna", rendered)
        self.assertNotIn("handoff_brief", rendered)
        self.assertNotIn("source_fingerprint", rendered)
        self.feedback.validate_contract("evidence-v1", librarian)
        self.feedback.validate_contract("evidence-v1", fallback)

    def test_unsupported_librarian_falls_back_only_with_sufficient_visible_evidence(
        self,
    ) -> None:
        unsupported = copy.deepcopy(self.fixture["inputs"]["compatible_librarian"])
        unsupported["format_version"] = "session-summary.v2"

        fallback = self.acquire(librarian=unsupported)
        self.assertEqual(fallback["source"], "fallback")
        self.assertEqual(fallback["items"], self.fallback_items)

        with self.assertRaises(self.feedback.ValidationError) as raised:
            self.acquire(librarian=unsupported, visible_items=[])

        message = str(raised.exception).lower()
        self.assertIn("librarian", message)
        self.assertIn("fallback", message)
        self.assertIn("evidence", message)

    def test_prohibited_sentinels_never_reach_any_downstream_surface(self) -> None:
        evidence = self.acquire(
            librarian=self.fixture["inputs"]["compatible_librarian"]
        )
        shortlist = self.feedback.shortlist_targets(evidence)
        prompt = self.feedback.canonical_json(
            {"evidence": evidence, "shortlist": shortlist, "excerpts": []}
        )
        generator_recording = {"request_input": prompt, "request_count": 1}
        generator_output = {
            "contract_version": "generator-response-v1",
            "proposals": [],
        }
        run_accounting = {
            "evidence_bytes": evidence["accounting"]["serialized_bytes"],
            "excerpt_bytes": 0,
            "request_input": self.feedback.measure_text("request_input", prompt),
        }
        surfaces = {
            "normalized evidence": evidence,
            "excerpts": [],
            "prompt": prompt,
            "generator recording": generator_recording,
            "generator output": generator_output,
            "persisted run accounting": run_accounting,
        }
        for surface, value in surfaces.items():
            self.assert_sentinels_absent(value, surface)

    def test_shortlist_is_stable_and_does_not_open_targets(self) -> None:
        evidence = self.acquire(librarian=None)
        reordered = copy.deepcopy(evidence)
        reordered["items"] = list(reversed(reordered["items"]))
        reordered = self.feedback.with_evidence_accounting(reordered)

        with mock.patch(
            "builtins.open", side_effect=AssertionError("pre-shortlist target read")
        ):
            first = self.feedback.shortlist_targets(evidence)
            second = self.feedback.shortlist_targets(reordered)

        self.assertEqual(first, second)
        self.assertGreater(len(first), 0)
        self.assertEqual(
            [entry["concrete_target"] for entry in first],
            sorted(entry["concrete_target"] for entry in first),
        )

    def test_only_shortlisted_skill_or_instruction_targets_yield_excerpts(self) -> None:
        target_root = Path(self.temp_dir.name) / "targets"
        skill_path = target_root / ".jcode" / "skills" / "example" / "SKILL.md"
        instructions_path = target_root / "AGENTS.md"
        config_path = target_root / ".jcode" / "config.json"
        for path, content in (
            (skill_path, "skill excerpt with é and bounded trailing content"),
            (instructions_path, "instruction excerpt"),
            (config_path, '{"must_not_be_read": true}'),
        ):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content, encoding="utf-8")

        shortlist = [
            {
                "category": "skills",
                "scope": "project-jcode",
                "concrete_target": ".jcode/skills/example/SKILL.md",
                "selection_evidence": ["skill-1"],
            },
            {
                "category": "global-instructions",
                "scope": "project-jcode",
                "concrete_target": "AGENTS.md",
                "selection_evidence": ["path-1"],
            },
            {
                "category": "hooks-config",
                "scope": "project-jcode",
                "concrete_target": ".jcode/config.json",
                "selection_evidence": ["path-1"],
            },
        ]
        opened: list[Path] = []

        def recording_open(path: str | Path, *args, **kwargs):
            opened.append(Path(path).resolve())
            return open(path, *args, **kwargs)

        excerpts = self.feedback.load_shortlisted_excerpts(
            shortlist=shortlist,
            target_root=target_root,
            per_excerpt_byte_limit=20,
            total_excerpt_byte_limit=40,
            opener=recording_open,
        )

        self.assertEqual(opened, [skill_path.resolve(), instructions_path.resolve()])
        self.assertNotIn(config_path.resolve(), opened)
        self.assertEqual(
            [entry["concrete_target"] for entry in excerpts],
            [".jcode/skills/example/SKILL.md", "AGENTS.md"],
        )
        for entry in excerpts:
            measurement = self.feedback.measure_text("excerpt", entry["excerpt"])
            self.assertEqual(entry["accounting"], measurement)
            self.assertLessEqual(entry["accounting"]["bytes"], 20)
        self.assertEqual(
            sum(entry["accounting"]["bytes"] for entry in excerpts),
            self.feedback.excerpt_accounting(excerpts)["serialized_bytes"],
        )
        evidence = self.acquire(librarian=None)
        request_accounting = {
            "evidence_bytes": evidence["accounting"]["serialized_bytes"],
            "excerpt_bytes": self.feedback.excerpt_accounting(excerpts)[
                "serialized_bytes"
            ],
        }
        self.assertEqual(
            request_accounting["excerpt_bytes"],
            self.feedback.excerpt_accounting(excerpts)["serialized_bytes"],
        )

    def test_both_acquisition_paths_reconcile_privacy_reads_and_accounting(
        self,
    ) -> None:
        target_root = Path(self.temp_dir.name) / "targets"
        skill_path = target_root / ".jcode" / "skills" / "session-feedback" / "SKILL.md"
        skill_path.parent.mkdir(parents=True)
        skill_path.write_text(
            "bounded session feedback excerpt with é", encoding="utf-8"
        )

        acquired = {
            "librarian": self.acquire(
                librarian=self.fixture["inputs"]["compatible_librarian"]
            ),
            "fallback": self.acquire(librarian=None),
        }
        for source, evidence in acquired.items():
            with self.subTest(source=source):
                opened_before_excerpt: list[Path] = []

                def reject_pre_excerpt_open(path: str | Path, *args, **kwargs):
                    opened_before_excerpt.append(Path(path))
                    raise AssertionError("target opened before excerpt loading")

                with mock.patch("builtins.open", side_effect=reject_pre_excerpt_open):
                    shortlist = self.feedback.shortlist_targets(evidence)

                self.assertEqual(opened_before_excerpt, [])
                self.assertEqual(evidence["contract_version"], "evidence-v1")
                self.assertEqual(
                    evidence["accounting"],
                    self.feedback.evidence_accounting(evidence["items"]),
                )
                self.assert_sentinels_absent(evidence, f"{source} evidence")
                self.assert_sentinels_absent(shortlist, f"{source} shortlist")

                excerpts = self.feedback.load_shortlisted_excerpts(
                    shortlist=shortlist,
                    target_root=target_root,
                    per_excerpt_byte_limit=128,
                    total_excerpt_byte_limit=128,
                )
                excerpt_accounting = self.feedback.excerpt_accounting(excerpts)
                self.assertEqual(
                    excerpt_accounting["serialized_bytes"],
                    sum(entry["accounting"]["bytes"] for entry in excerpts),
                )
                self.assertEqual(
                    excerpt_accounting["estimated_tokens"],
                    sum(entry["accounting"]["estimated_tokens"] for entry in excerpts),
                )
                self.assert_sentinels_absent(excerpts, f"{source} excerpts")

    def test_normalization_shortlisting_and_fingerprints_are_byte_stable_100_times(
        self,
    ) -> None:
        fingerprint_input = {
            "scope": " Project Jcode ",
            "category": " Skills ",
            "concrete_target": ".jcode//skills/session-feedback/SKILL.md",
            "problem": "  Repeated   bounded lookup. ",
            "intended_outcome": "Reuse the visible receipt.",
        }
        baseline: bytes | None = None

        for _ in range(100):
            evidence = self.acquire(librarian=None)
            shortlist = self.feedback.shortlist_targets(evidence)
            result = self.feedback.canonical_bytes(
                {
                    "evidence": evidence,
                    "shortlist": shortlist,
                    "fingerprint_material": self.feedback.fingerprint_material(
                        fingerprint_input
                    ),
                    "fingerprint": self.feedback.proposal_fingerprint(
                        fingerprint_input
                    ),
                }
            )
            if baseline is None:
                baseline = result
            self.assertEqual(result, baseline)

    def test_boundary_failures_are_actionable_and_have_no_external_effects(
        self,
    ) -> None:
        home_before = sorted(
            path.relative_to(self.home) for path in self.home.rglob("*")
        )
        malformed = copy.deepcopy(self.fixture["inputs"]["compatible_librarian"])
        malformed["summary"] = {"unexpected": "not-a-summary"}

        with self.assertRaises(self.feedback.ValidationError) as acquisition_error:
            self.acquire(librarian=malformed, visible_items=[])
        self.assertIn("librarian", str(acquisition_error.exception).lower())
        self.assertIn("fallback", str(acquisition_error.exception).lower())

        opener = mock.Mock(
            side_effect=AssertionError("invalid shortlist must not read files")
        )
        with self.assertRaises(self.feedback.ValidationError) as shortlist_error:
            self.feedback.load_shortlisted_excerpts(
                shortlist=[{"category": "skills", "concrete_target": 7}],
                target_root=Path(self.temp_dir.name),
                per_excerpt_byte_limit=64,
                total_excerpt_byte_limit=64,
                opener=opener,
            )
        self.assertIn("concrete_target", str(shortlist_error.exception))
        opener.assert_not_called()

        with self.assertRaises(self.feedback.ValidationError) as accounting_error:
            self.feedback.excerpt_accounting(
                [{"excerpt": "abc", "accounting": {"bytes": 2, "estimated_tokens": 1}}]
            )
        self.assertIn("accounting", str(accounting_error.exception).lower())

        self.assertEqual(
            sorted(path.relative_to(self.home) for path in self.home.rglob("*")),
            home_before,
        )
        self.assertFalse(self.invocation_log.exists())


class GenerationBoundaryAndReviewOnlyTests(IsolatedSessionFeedbackTestCase):
    def setUp(self) -> None:
        super().setUp()
        self.fixture = json.loads(GENERATOR_FIXTURE_PATH.read_text(encoding="utf-8"))
        complete = self.fixture["valid_responses"]["complete_taxonomy"]["response"]
        references = sorted(
            {
                reference
                for proposal in complete["proposals"]
                for reference in proposal["evidence_references"]
            }
        )
        self.evidence = self.feedback.with_evidence_accounting(
            {
                "contract_version": "evidence-v1",
                "source": "fallback",
                "items": [
                    {
                        "reference": reference,
                        "category": "visible_outcome",
                        "summary": f"Synthetic visible outcome for {reference}.",
                    }
                    for reference in references
                ],
                "accounting": {
                    "serialized_bytes": 0,
                    "estimated_tokens": 0,
                    "by_category": {},
                },
            }
        )
        self.shortlist = copy.deepcopy(self.fixture["shortlisted_targets"])
        self.request_input = {
            "session_id": "session-generation-1",
            "evidence": self.evidence,
            "shortlisted_targets": self.shortlist,
            "excerpts": [],
        }

    def budget(self, **overrides: object) -> dict[str, object]:
        budget: dict[str, object] = {
            "model": "gpt-5.6-sol",
            "effort": "medium",
            "max_input_tokens": 100_000,
            "max_output_tokens": 100_000,
            "max_proposals": 8,
            "max_elapsed_seconds": 30.0,
            "max_estimated_cost_usd": 1.0,
        }
        budget.update(overrides)
        return budget

    def runner_for(
        self,
        response: dict[str, object],
        *,
        elapsed_seconds: float = 0.25,
        estimated_cost_usd: float = 0.01,
        observed_input_tokens: int | None = None,
        observed_output_tokens: int | None = None,
    ) -> mock.Mock:
        receipt = {
            "returncode": 0,
            "stdout": self.feedback.canonical_json(response),
            "stderr": "",
            "elapsed_seconds": elapsed_seconds,
            "estimated_cost_usd": estimated_cost_usd,
        }
        if observed_input_tokens is not None:
            receipt["observed_input_tokens"] = observed_input_tokens
        if observed_output_tokens is not None:
            receipt["observed_output_tokens"] = observed_output_tokens
        return mock.Mock(return_value=receipt)

    def generate(
        self,
        response: dict[str, object],
        *,
        budget: dict[str, object] | None = None,
        runner: mock.Mock | None = None,
    ) -> tuple[dict[str, object], mock.Mock]:
        injected_runner = runner or self.runner_for(response)
        result = self.feedback.generate_review_proposals(
            request_input=self.request_input,
            evidence=self.evidence,
            shortlisted_targets=self.shortlist,
            effective_config=budget or self.budget(),
            runner=injected_runner,
        )
        return result, injected_runner

    def invalid_response(self, name: str) -> dict[str, object]:
        case = self.fixture["invalid_responses"][name]
        response = copy.deepcopy(
            self.fixture["valid_responses"][case["base"]]["response"]
        )

        def resolve(pointer: str) -> tuple[object, str | int]:
            parts = [
                part.replace("~1", "/").replace("~0", "~")
                for part in pointer.split("/")[1:]
            ]
            current: object = response
            for part in parts[:-1]:
                current = (
                    current[int(part)] if isinstance(current, list) else current[part]
                )
            final = int(parts[-1]) if isinstance(current, list) else parts[-1]
            return current, final

        for pointer, value in case.get("replace", {}).items():
            parent, key = resolve(pointer)
            parent[key] = copy.deepcopy(value)
        for pointer, value in case.get("add", {}).items():
            parent, key = resolve(pointer)
            parent[key] = copy.deepcopy(value)
        append_copy = case.get("append_copy")
        if append_copy:
            destination, destination_key = resolve(append_copy["pointer"])
            source, source_key = resolve(append_copy["source_pointer"])
            destination[destination_key].append(copy.deepcopy(source[source_key]))
        return response

    def test_records_one_request_native_oauth_no_tools_json_command(self) -> None:
        zero = self.fixture["valid_responses"]["zero_proposals"]["response"]
        result, runner = self.generate(zero)

        self.assertEqual(result["proposals"], [])
        self.assertEqual(result["rendered_proposals"], [])
        self.assertEqual(result["accounting"]["request_count"], 1)
        runner.assert_called_once()
        command = runner.call_args.args[0]
        self.assertEqual(command[0:2], ["jcode", "run"])
        required_pairs = {
            "--model": "openai-oauth:gpt-5.6-sol",
            "--reasoning-effort": "medium",
            "--tool-profile": "none",
            "--max-turns": "1",
            "--token-budget": "200000",
        }
        for flag, value in required_pairs.items():
            self.assertIn(flag, command)
            self.assertEqual(command[command.index(flag) + 1], value)
        self.assertIn("--json", command)
        self.assertNotIn("--schema", command)
        self.assertNotIn("--provider", command)
        self.assertNotIn("--tools", command)
        self.assertIn('"contract_version"', command[-1])
        self.assertIn('"generator-response-v1"', command[-1])
        self.assertEqual(
            result["accounting"]["command"]["model"],
            "openai-oauth:gpt-5.6-sol",
        )
        self.assertEqual(result["accounting"]["command"]["output_mode"], "json")

    def test_enforces_observed_provider_token_usage_without_retry(self) -> None:
        zero = self.fixture["valid_responses"]["zero_proposals"]["response"]
        cases = (
            ("max_input_tokens", {"observed_input_tokens": 100_001}),
            ("max_output_tokens", {"observed_output_tokens": 100_001}),
        )
        for field, observed in cases:
            with self.subTest(field=field):
                runner = self.runner_for(zero, **observed)
                with self.assertRaisesRegex(self.feedback.ValidationError, field):
                    self.generate(zero, runner=runner)
                runner.assert_called_once()

    def test_renders_validated_proposals_as_bounded_review_only_json_and_markdown(
        self,
    ) -> None:
        complete = self.fixture["valid_responses"]["complete_taxonomy"]["response"]
        result, runner = self.generate(complete)

        self.assertEqual(len(result["rendered_proposals"]), len(complete["proposals"]))
        runner.assert_called_once()
        for proposal, rendered in zip(
            result["proposals"], result["rendered_proposals"], strict=True
        ):
            with self.subTest(category=proposal["target"]["category"]):
                document = json.loads(rendered["json"])
                self.assertEqual(document["proposal"], proposal)
                self.assertTrue(document["review_only"])
                self.assertEqual(document["review_state"], "needs-approval")
                self.assertIn("**State:** review-only", rendered["markdown"])
                self.assertIn("**Review state:** needs-approval", rendered["markdown"])
                self.assertIn(
                    f"**Category:** {proposal['target']['category']}",
                    rendered["markdown"],
                )
                self.assertIn(
                    f"**Scope:** {proposal['target']['scope']}", rendered["markdown"]
                )
                self.assertIn(proposal["fingerprint"], rendered["markdown"])
                self.assertIn(rendered["json"], rendered["markdown"])
                self.assertEqual(
                    rendered["accounting"]["json"]["bytes"],
                    len(rendered["json"].encode("utf-8")),
                )
                self.assertEqual(
                    rendered["accounting"]["markdown"]["bytes"],
                    len(rendered["markdown"].encode("utf-8")),
                )

    def test_complete_taxonomy_reconciles_parity_fingerprints_and_accounting(
        self,
    ) -> None:
        complete = self.fixture["valid_responses"]["complete_taxonomy"]["response"]
        result, runner = self.generate(complete)

        expected_categories = {
            "global-instructions",
            "skills",
            "hooks-config",
            "model-profile-choices",
            "context-skill-routing",
            "harness-setup-contracts",
            "sdk-public-surfaces",
            "jcode",
        }
        required_fields = {
            "contract_version",
            "target",
            "evidence_references",
            "problem",
            "hypothesis",
            "suggested_behavior",
            "expected_benefit",
            "token_context_impact",
            "risk",
            "blast_radius",
            "validation_plan",
            "confidence",
            "fingerprint",
            "non_goals",
        }
        self.assertEqual(
            {proposal["target"]["category"] for proposal in result["proposals"]},
            expected_categories,
        )
        self.assertEqual(
            {proposal["target"]["scope"] for proposal in result["proposals"]},
            {"personal-global", "project-jcode"},
        )

        runner.assert_called_once()
        prompt = runner.call_args.args[0][-1]
        output = self.feedback.canonical_json(complete)
        accounting = result["accounting"]
        self.assertEqual(accounting["request_count"], 1)
        self.assertEqual(accounting["proposal_count"], len(result["proposals"]))
        self.assertEqual(
            accounting["request_input"],
            self.feedback.measure_text("request_input", prompt),
        )
        self.assertEqual(
            accounting["request_output"],
            self.feedback.measure_text("request_output", output),
        )

        for proposal, rendered in zip(
            result["proposals"], result["rendered_proposals"], strict=True
        ):
            with self.subTest(category=proposal["target"]["category"]):
                self.assertEqual(set(proposal), required_fields)
                document = json.loads(rendered["json"])
                self.assertEqual(document["proposal"], proposal)
                for value in (
                    proposal["problem"],
                    proposal["hypothesis"],
                    proposal["suggested_behavior"],
                    proposal["expected_benefit"],
                    proposal["token_context_impact"],
                    proposal["blast_radius"],
                    *proposal["validation_plan"],
                    *proposal["non_goals"],
                ):
                    self.assertIn(value, rendered["markdown"])
                self.assertEqual(
                    rendered["accounting"]["json"],
                    self.feedback.measure_text("proposal_json", rendered["json"]),
                )
                self.assertEqual(
                    rendered["accounting"]["markdown"],
                    self.feedback.measure_text(
                        "proposal_markdown", rendered["markdown"]
                    ),
                )

        common_material = {
            "category": "skills",
            "concrete_target": ".jcode/skills/session-feedback/SKILL.md",
            "problem": "The same synthetic problem appears in two ownership scopes.",
            "intended_outcome": "Keep personal and project proposals distinct.",
        }
        personal = self.feedback.proposal_fingerprint(
            {**common_material, "scope": "personal-global"}
        )
        project = self.feedback.proposal_fingerprint(
            {**common_material, "scope": "project-jcode"}
        )
        self.assertNotEqual(personal, project)

    def test_fake_generator_scenarios_make_exactly_zero_or_one_request(self) -> None:
        complete = self.fixture["valid_responses"]["complete_taxonomy"]["response"]
        zero = self.fixture["valid_responses"]["zero_proposals"]["response"]

        for name, response, expected_proposals in (
            ("complete", complete, len(complete["proposals"])),
            ("zero", zero, 0),
        ):
            with self.subTest(case=name):
                result, runner = self.generate(response)
                self.assertEqual(result["accounting"]["request_count"], 1)
                self.assertEqual(
                    result["accounting"]["proposal_count"], expected_proposals
                )
                runner.assert_called_once()

        malformed_runner = self.runner_for(self.invalid_response("malformed"))
        with self.assertRaises(self.feedback.ContractValidationError):
            self.generate({}, runner=malformed_runner)
        malformed_runner.assert_called_once()

        timeout_runner = self.runner_for(complete, elapsed_seconds=31.0)
        with self.assertRaises(self.feedback.ValidationError):
            self.generate(complete, runner=timeout_runner)
        timeout_runner.assert_called_once()

        preflight_runner = self.runner_for(complete)
        with self.assertRaises(self.feedback.ValidationError):
            self.generate(
                complete,
                budget=self.budget(max_input_tokens=1),
                runner=preflight_runner,
            )
        preflight_runner.assert_not_called()

        output_budget_runner = self.runner_for(complete)
        with self.assertRaises(self.feedback.ValidationError):
            self.generate(
                complete,
                budget=self.budget(max_output_tokens=1),
                runner=output_budget_runner,
            )
        output_budget_runner.assert_called_once()

    def test_validates_the_complete_response_before_rendering_any_proposal(
        self,
    ) -> None:
        invalid = copy.deepcopy(
            self.fixture["valid_responses"]["complete_taxonomy"]["response"]
        )
        invalid["proposals"][-1]["evidence_references"] = ["missing-evidence"]

        renderer = mock.Mock(wraps=self.feedback.render_review_proposal)
        with mock.patch.object(self.feedback, "render_review_proposal", renderer):
            with self.assertRaises(self.feedback.ValidationError):
                self.generate(invalid)

        renderer.assert_not_called()

    def test_enforces_every_generation_budget_at_below_and_above_the_limit(
        self,
    ) -> None:
        complete = self.fixture["valid_responses"]["complete_taxonomy"]["response"]
        baseline, _ = self.generate(complete)
        accounting = baseline["accounting"]
        dimensions = (
            ("max_input_tokens", accounting["request_input"]["estimated_tokens"]),
            ("max_output_tokens", accounting["request_output"]["estimated_tokens"]),
            ("max_proposals", accounting["proposal_count"]),
            ("max_elapsed_seconds", accounting["elapsed_seconds"]),
            ("max_estimated_cost_usd", accounting["estimated_cost_usd"]),
        )

        for setting, measured in dimensions:
            with self.subTest(setting=setting, position="below"):
                result, runner = self.generate(
                    complete, budget=self.budget(**{setting: measured + 1})
                )
                self.assertEqual(result["accounting"]["request_count"], 1)
                runner.assert_called_once()
            with self.subTest(setting=setting, position="at"):
                result, runner = self.generate(
                    complete, budget=self.budget(**{setting: measured})
                )
                self.assertEqual(result["accounting"]["request_count"], 1)
                runner.assert_called_once()
            with self.subTest(setting=setting, position="above"):
                runner = self.runner_for(complete)
                with self.assertRaises(self.feedback.ValidationError) as raised:
                    self.generate(
                        complete,
                        budget=self.budget(**{setting: max(0, measured - 1)}),
                        runner=runner,
                    )
                self.assertIn("limit", str(raised.exception).lower())
                self.assertLessEqual(
                    runner.call_count, 1, "budget failure must never retry"
                )

        timeout_runner = self.runner_for(complete, elapsed_seconds=31.0)
        with self.assertRaises(self.feedback.ValidationError):
            self.generate(complete, runner=timeout_runner)
        timeout_runner.assert_called_once()

    def test_rejects_untrusted_output_before_persistence_without_retry(self) -> None:
        unresolved = copy.deepcopy(
            self.fixture["valid_responses"]["complete_taxonomy"]["response"]
        )
        unresolved["proposals"][0]["evidence_references"] = ["missing-evidence"]
        invalid_cases = {
            name: self.invalid_response(name)
            for name in (
                "malformed",
                "oversized",
                "unknown_category",
                "fingerprint_mismatch",
                "unshortlisted_target",
                "extra_property",
            )
        }
        invalid_cases["unresolved_evidence"] = unresolved

        for name, response in invalid_cases.items():
            with self.subTest(case=name):
                home_before = sorted(
                    path.relative_to(self.home) for path in self.home.rglob("*")
                )
                runner = self.runner_for(response)
                with self.assertRaises(
                    (
                        self.feedback.ValidationError,
                        self.feedback.ContractValidationError,
                    )
                ) as raised:
                    self.generate(response, runner=runner)
                self.assertTrue(str(raised.exception).strip())
                runner.assert_called_once()
                self.assertEqual(
                    sorted(
                        path.relative_to(self.home) for path in self.home.rglob("*")
                    ),
                    home_before,
                    "invalid generator output must be rejected before persistence",
                )

    def test_generation_boundary_cannot_mutate_targets_or_lifecycle(self) -> None:
        target_root = Path(self.temp_dir.name) / "targets"
        target_root.mkdir()
        target = target_root / "SKILL.md"
        target.write_text("synthetic immutable target\n", encoding="utf-8")
        target_before = target.read_bytes()
        home_before = sorted(
            path.relative_to(self.home) for path in self.home.rglob("*")
        )
        zero = self.fixture["valid_responses"]["zero_proposals"]["response"]
        runner = self.runner_for(zero)

        with (
            mock.patch.object(
                Path, "write_text", side_effect=AssertionError("target write forbidden")
            ),
            mock.patch.object(
                Path,
                "write_bytes",
                side_effect=AssertionError("target write forbidden"),
            ),
            mock.patch.object(
                Path, "unlink", side_effect=AssertionError("delete forbidden")
            ),
            mock.patch("os.remove", side_effect=AssertionError("delete forbidden")),
            mock.patch("os.replace", side_effect=AssertionError("replace forbidden")),
            mock.patch(
                "subprocess.run",
                side_effect=AssertionError("uninjected process forbidden"),
            ),
            mock.patch(
                "subprocess.Popen",
                side_effect=AssertionError("uninjected process forbidden"),
            ),
            mock.patch(
                "socket.create_connection",
                side_effect=AssertionError("unapproved network forbidden"),
            ),
            mock.patch(
                "urllib.request.urlopen",
                side_effect=AssertionError("unapproved network forbidden"),
            ),
        ):
            result, _ = self.generate(zero, runner=runner)

        self.assertEqual(result["proposals"], [])
        runner.assert_called_once()
        self.assertEqual(target.read_bytes(), target_before)
        self.assertEqual(
            sorted(path.relative_to(self.home) for path in self.home.rglob("*")),
            home_before,
        )
        flattened_command = " ".join(runner.call_args.args[0]).lower()
        for forbidden in (
            "apply",
            "approve",
            "start",
            "delete",
            "replace",
            "publish",
            "replicate",
        ):
            self.assertNotIn(forbidden, flattened_command)


class LocalPersistenceAndDeduplicationTests(IsolatedSessionFeedbackTestCase):
    class FakeBdRunner:
        def __init__(
            self,
            records: list[dict[str, object]],
            *,
            fail_operation: str | None = None,
        ) -> None:
            self.records = copy.deepcopy(records)
            self.fail_operation = fail_operation
            self.commands: list[tuple[tuple[str, ...], Path]] = []
            self.created: list[dict[str, object]] = []
            self.appended: list[tuple[str, dict[str, object]]] = []

        def __call__(
            self, command: list[str] | tuple[str, ...], *, cwd: Path
        ) -> dict[str, object]:
            argv = tuple(str(part) for part in command)
            working_directory = Path(cwd)
            self.commands.append((argv, working_directory))
            lowered = {part.lower() for part in argv}
            prohibited = {"remote", "replicate", "sync", "push", "pull", "delete"}
            if lowered & prohibited:
                raise AssertionError(f"prohibited bd operation: {argv}")

            operation = self._operation(argv)
            if operation == self.fail_operation:
                return {
                    "returncode": 23,
                    "stdout": "",
                    "stderr": f"synthetic {operation} failure",
                }
            if operation == "init":
                (working_directory / ".beads").mkdir(parents=True, exist_ok=True)
                return {"returncode": 0, "stdout": "", "stderr": ""}
            if operation == "list":
                return {
                    "returncode": 0,
                    "stdout": json.dumps(self.records),
                    "stderr": "",
                }
            if operation == "create":
                bead_id = f"feedback-created-{len(self.created) + 1:03d}"
                self.created.append({"bead_id": bead_id, "command": argv})
                return {
                    "returncode": 0,
                    "stdout": json.dumps({"id": bead_id}),
                    "stderr": "",
                }
            if operation == "append":
                bead_id = next(part for part in argv if part.startswith("feedback-"))
                payload = json.loads(argv[-1])
                self.appended.append((bead_id, payload))
                return {"returncode": 0, "stdout": "", "stderr": ""}
            raise AssertionError(f"unexpected bd command: {argv}")

        @staticmethod
        def _operation(argv: tuple[str, ...]) -> str:
            if "init" in argv:
                return "init"
            if "list" in argv:
                return "list"
            if "create" in argv:
                return "create"
            if "comment" in argv or "comments" in argv:
                return "append"
            return "unknown"

    def setUp(self) -> None:
        super().setUp()
        self.fixture = json.loads(DEDUP_FIXTURE_PATH.read_text(encoding="utf-8"))
        self.feedback_root = self.home / ".jcode" / "feedback"

    def records(self, *fingerprint_names: str) -> list[dict[str, object]]:
        records_by_fingerprint = self.fixture["records_by_fingerprint"]
        fingerprints = self.fixture["fingerprints"]
        return [
            copy.deepcopy(record)
            for name in fingerprint_names
            for record in records_by_fingerprint[fingerprints[name]["value"]]
        ]

    def proposal(self, fingerprint_name: str) -> dict[str, object]:
        fingerprint = self.fixture["fingerprints"][fingerprint_name]
        material = fingerprint["material"]
        proposal = valid_proposal()
        proposal.update(
            {
                "target": {
                    "category": material["category"],
                    "scope": material["scope"],
                    "concrete_target": material["concrete_target"],
                },
                "problem": material["problem"],
                "expected_benefit": material["intended_outcome"],
                "fingerprint": fingerprint["value"],
            }
        )
        return proposal

    @staticmethod
    def occurrence(suffix: str = "new") -> dict[str, object]:
        return {
            "occurrence_id": f"occurrence-{suffix}",
            "session_id": f"session-{suffix}",
            "observed_at": "2026-08-14T07:00:00Z",
            "evidence_references": [f"outcome-{suffix}"],
            "evidence_digest": hashlib.sha256(suffix.encode()).hexdigest(),
        }

    def persist(
        self,
        fingerprint_name: str,
        runner: "LocalPersistenceAndDeduplicationTests.FakeBdRunner",
        *,
        occurrence: dict[str, object] | None = None,
        atomic_replace=None,
    ) -> dict[str, object]:
        kwargs: dict[str, object] = {
            "proposal": self.proposal(fingerprint_name),
            "evidence_occurrence": occurrence or self.occurrence(fingerprint_name),
            "feedback_root": self.feedback_root,
            "bd_runner": runner,
        }
        if atomic_replace is not None:
            kwargs["atomic_replace"] = atomic_replace
        return self.feedback.persist_review_proposal(**kwargs)

    def assert_only_local_bd_commands(self, runner: FakeBdRunner) -> None:
        self.assertTrue(runner.commands)
        for command, cwd in runner.commands:
            self.assertEqual(command[0], "bd")
            self.assertTrue(cwd.is_relative_to(self.feedback_root))
            self.assertFalse(
                {"remote", "replicate", "sync", "push", "pull"}
                & {part.lower() for part in command}
            )

    def test_bootstrap_is_idempotent_local_and_non_replicated(self) -> None:
        runner = self.FakeBdRunner([])

        first = self.feedback.bootstrap_feedback_store(
            feedback_root=self.feedback_root, bd_runner=runner
        )
        second = self.feedback.bootstrap_feedback_store(
            feedback_root=self.feedback_root, bd_runner=runner
        )

        self.assertEqual(first, second)
        for path in (
            self.feedback_root,
            self.feedback_root / "runs",
            self.feedback_root / "proposals",
            self.feedback_root / ".beads",
        ):
            self.assertTrue(path.is_dir(), path)
            if os.name != "nt":
                self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o700, path)
        init_commands = [command for command, _ in runner.commands if "init" in command]
        self.assertEqual(len(init_commands), 1)
        self.assertFalse((self.feedback_root / ".beads" / "remotes.json").exists())
        self.assert_only_local_bd_commands(runner)

    def test_finds_matches_across_allowed_lifecycle_states_only(self) -> None:
        for name, expected_bead_id in (
            ("open", "feedback-open-001"),
            ("in_progress", "feedback-progress-001"),
            ("relevant_closed", "feedback-closed-relevant-001"),
        ):
            with self.subTest(state=name):
                runner = self.FakeBdRunner(self.records(name))
                result = self.persist(name, runner)
                self.assertEqual(result["action"], "evidence_appended")
                self.assertEqual(result["bead_id"], expected_bead_id)
                self.assertEqual(len(runner.appended), 1)
                self.assertEqual(runner.created, [])

        runner = self.FakeBdRunner(self.records("unrelated_closed"))
        result = self.persist("unrelated_closed", runner)
        self.assertEqual(result["action"], "created")
        self.assertEqual(len(runner.created), 1)
        self.assertEqual(runner.appended, [])

    def test_appends_only_new_evidence_and_creates_one_needs_approval_record(
        self,
    ) -> None:
        duplicate = copy.deepcopy(
            self.fixture["cases"]["duplicate_evidence"]["incoming_occurrence"]
        )
        runner = self.FakeBdRunner(self.records("duplicate_evidence"))
        result = self.persist("duplicate_evidence", runner, occurrence=duplicate)
        self.assertEqual(result["action"], "duplicate_evidence_ignored")
        self.assertEqual(runner.appended, [])
        self.assertEqual(runner.created, [])

        fresh = self.occurrence("fresh-duplicate")
        result = self.persist("duplicate_evidence", runner, occurrence=fresh)
        self.assertEqual(result["action"], "evidence_appended")
        self.assertEqual(runner.appended, [("feedback-duplicate-evidence-001", fresh)])
        self.assertEqual(runner.created, [])

        unique_runner = self.FakeBdRunner([])
        result = self.persist("unique", unique_runner)
        self.assertEqual(result["action"], "created")
        self.assertEqual(len(unique_runner.created), 1)
        create_command = " ".join(unique_runner.created[0]["command"])
        self.assertIn("session-feedback", create_command)
        self.assertIn("needs-approval", create_command)
        self.assertEqual(unique_runner.appended, [])
        self.assert_only_local_bd_commands(unique_runner)

    def test_repeated_creation_artifacts_and_bead_metadata_remain_consistent(
        self,
    ) -> None:
        repository_beads = SKILL_DIR.parents[2] / ".beads"
        repository_beads_existed = repository_beads.exists()
        occurrence = self.occurrence("consistent")
        runner = self.FakeBdRunner([])

        created = self.persist("unique", runner, occurrence=occurrence)

        self.assertEqual(created["action"], "created")
        json_path = Path(created["proposal_json"])
        markdown_path = Path(created["proposal_markdown"])
        for path in (json_path, markdown_path):
            self.assertTrue(path.is_relative_to(self.feedback_root))
            self.assertTrue(path.is_file())

        document = json.loads(json_path.read_text(encoding="utf-8"))
        proposal = self.proposal("unique")
        fingerprint = proposal["fingerprint"]
        self.assertEqual(document["proposal"], proposal)
        self.assertEqual(document["proposal"]["fingerprint"], fingerprint)
        self.assertEqual(document["evidence_history"], [occurrence])
        self.assertTrue(document["review_only"])
        self.assertEqual(document["review_state"], "needs-approval")

        markdown = markdown_path.read_text(encoding="utf-8")
        self.assertIn(f"**Fingerprint:** {fingerprint}", markdown)
        self.assertIn("## Evidence history", markdown)
        self.assertIn(self.feedback.canonical_json(occurrence), markdown)

        self.assertEqual(len(runner.created), 1)
        create_command = runner.created[0]["command"]
        description = create_command[create_command.index("--description") + 1]
        labels = create_command[create_command.index("--labels") + 1].split(",")
        self.assertEqual(description, markdown.split("## Evidence history\n", 1)[0])
        self.assertEqual(labels, ["session-feedback", "needs-approval"])
        self.assertIn(proposal["target"]["concrete_target"], create_command[2])
        self.assertIn(f"**Fingerprint:** {fingerprint}", description)

        repeated_record = {
            "bead_id": created["bead_id"],
            "fingerprint": fingerprint,
            "status": "open",
            "labels": labels,
            "description": description,
            "evidence_history": document["evidence_history"],
        }
        repeated_runner = self.FakeBdRunner([repeated_record])
        repeated = self.persist("unique", repeated_runner, occurrence=occurrence)
        self.assertEqual(
            repeated,
            {
                "action": "duplicate_evidence_ignored",
                "bead_id": created["bead_id"],
            },
        )
        self.assertEqual(repeated_runner.created, [])
        self.assertEqual(repeated_runner.appended, [])
        self.assert_only_local_bd_commands(runner)
        self.assert_only_local_bd_commands(repeated_runner)
        self.assertEqual(repository_beads.exists(), repository_beads_existed)

    def test_ambiguous_malformed_and_bootstrap_failures_leave_no_partial_state(
        self,
    ) -> None:
        target = SKILL_DIR / "SKILL.md"
        target_before = target.read_bytes()
        failure_cases = (
            (
                "ambiguous matches",
                self.FakeBdRunner(self.records("ambiguous")),
                "ambiguous",
            ),
            (
                "malformed records",
                self.FakeBdRunner([{"status": "open", "labels": ["session-feedback"]}]),
                "malformed",
            ),
            ("bootstrap failure", self.FakeBdRunner([], fail_operation="init"), "init"),
        )
        for index, (label, runner, expected_error) in enumerate(failure_cases):
            self.feedback_root = self.home / ".jcode" / "feedback" / f"failure-{index}"
            with (
                self.subTest(case=label),
                self.assertRaisesRegex(self.feedback.ValidationError, expected_error),
            ):
                self.persist("ambiguous", runner)
            self.assertEqual(runner.created, [])
            self.assertEqual(runner.appended, [])
            self.assertEqual(list(self.feedback_root.glob("*.tmp")), [])
            self.assertEqual(list((self.feedback_root / "proposals").glob("*")), [])
            self.assertEqual(target.read_bytes(), target_before)

    def test_atomic_write_failure_is_not_reported_as_success(self) -> None:
        runner = self.FakeBdRunner([])

        def fail_replace(_source: Path, _destination: Path) -> None:
            raise OSError("synthetic unwritable storage")

        with self.assertRaisesRegex(
            self.feedback.ValidationError, "unwritable|atomic|persist"
        ):
            self.persist("unique", runner, atomic_replace=fail_replace)

        self.assertEqual(runner.created, [])
        self.assertEqual(runner.appended, [])
        self.assertEqual(list((self.feedback_root / "proposals").glob("*")), [])
        self.assertEqual(list(self.feedback_root.rglob("*.tmp")), [])


class ConfigurationResolutionTests(IsolatedSessionFeedbackTestCase):
    def setUp(self) -> None:
        super().setUp()
        self.fixture = json.loads(CONFIG_FIXTURE_PATH.read_text(encoding="utf-8"))
        self.config_path = self.home / ".jcode" / "feedback" / "config.json"

    def write_persisted(self, values: dict[str, object]) -> None:
        self.config_path.parent.mkdir(parents=True, exist_ok=True)
        self.config_path.write_text(json.dumps(values), encoding="utf-8")

    def resolve(
        self,
        *,
        invocation: dict[str, object] | None = None,
        environment: dict[str, str] | None = None,
        persisted: dict[str, object] | None = None,
    ) -> dict[str, object]:
        if persisted is not None:
            self.write_persisted(persisted)
        return self.feedback.resolve_feedback_config(
            invocation_overrides=invocation or {},
            environment=environment or {},
            config_path=self.config_path,
        )

    @staticmethod
    def distinct_values(
        field: str, metadata: dict[str, object]
    ) -> tuple[object, str, object]:
        default = metadata["default"]
        if field == "model":
            return default, str(default), default
        if field == "effort":
            return "low", "high", "minimal"
        if isinstance(default, int):
            return default - 3, str(default - 2), default - 1
        return float(default) - 0.3, str(float(default) - 0.2), float(default) - 0.1

    def test_every_field_resolves_invocation_environment_persisted_then_default(
        self,
    ) -> None:
        for field, metadata in self.fixture["fields"].items():
            invocation_value, environment_value, persisted_value = self.distinct_values(
                field, metadata
            )
            environment_name = metadata["environment"]
            layers = (
                (
                    "invocation",
                    {field: invocation_value},
                    {environment_name: environment_value},
                    {field: persisted_value},
                    invocation_value,
                ),
                (
                    "environment",
                    {field: ""},
                    {environment_name: environment_value},
                    {field: persisted_value},
                    environment_value,
                ),
                (
                    "persisted",
                    {field: ""},
                    {environment_name: ""},
                    {field: persisted_value},
                    persisted_value,
                ),
                (
                    "default",
                    {field: ""},
                    {environment_name: ""},
                    {field: ""},
                    metadata["default"],
                ),
            )
            for source, invocation, environment, persisted, expected in layers:
                with self.subTest(field=field, source=source):
                    resolved = self.resolve(
                        invocation=invocation,
                        environment=environment,
                        persisted=persisted,
                    )
                    expected_type = type(metadata["default"])
                    self.assertEqual(resolved["values"][field], expected_type(expected))
                    self.assertEqual(resolved["sources"][field], source)

    def test_fixture_precedence_cases_and_empty_values_are_deterministic(self) -> None:
        for case in self.fixture["precedence_cases"]:
            with self.subTest(case=case["id"]):
                resolved = self.resolve(
                    invocation=case["invocation"],
                    environment=case["environment"],
                    persisted=case["persisted"],
                )
                expected = case["expected"]
                for field, value in expected.get("values", {}).items():
                    self.assertEqual(resolved["values"][field], value)
                for field, source in expected.get("sources", {}).items():
                    self.assertEqual(resolved["sources"][field], source)
                if "source_for_all" in expected:
                    self.assertEqual(
                        set(resolved["sources"].values()), {expected["source_for_all"]}
                    )

    def test_invalid_present_values_fail_at_their_source_without_fallback(self) -> None:
        fields = self.fixture["fields"]
        for case in self.fixture["invalid_value_cases"]:
            field = case["field"]
            source = case["source"]
            invocation: dict[str, object] = {}
            environment: dict[str, str] = {}
            persisted = {field: fields[field]["default"]}
            if source == "invocation":
                invocation[field] = case["value"]
            elif source == "environment":
                environment[fields[field]["environment"]] = str(case["value"])
            else:
                persisted[field] = case["value"]

            with (
                self.subTest(case=case["id"]),
                self.assertRaisesRegex(
                    self.feedback.ValidationError, case["expected_error"]
                ),
            ):
                self.resolve(
                    invocation=invocation,
                    environment=environment,
                    persisted=persisted,
                )

    def test_invalid_invocation_fails_before_evidence_reads_or_generation(self) -> None:
        runner = mock.Mock(side_effect=AssertionError("generation must not start"))
        with (
            mock.patch.object(
                self.feedback,
                "prepare_feedback_invocation",
                side_effect=AssertionError("evidence must not be read"),
            ),
            self.assertRaisesRegex(self.feedback.ValidationError, "unsupported model"),
        ):
            self.feedback.run_feedback(
                requested_session_id=None,
                current_session_id="session-current-1",
                visible_session_ids=("session-current-1",),
                visible_items=(),
                librarian_summary_path=None,
                invocation_config={"model": "synthetic-unsupported-model"},
                feedback_config_path=self.config_path,
                runner=runner,
            )
        runner.assert_not_called()

    def test_defaults_use_native_openai_oauth_medium_and_one_request(self) -> None:
        resolved = self.resolve()
        self.assertEqual(resolved["route"]["provider"], "openai")
        self.assertEqual(resolved["route"]["authentication"], "native-oauth")
        self.assertEqual(resolved["values"]["model"], "gpt-5.6-sol")
        self.assertEqual(resolved["values"]["effort"], "medium")
        self.assertEqual(resolved["route"]["max_requests"], 1)

    def test_diagnostics_report_non_secret_values_and_sources(self) -> None:
        secret = "synthetic-secret-must-not-appear"
        resolved = self.resolve(
            invocation={"effort": "low"},
            environment={
                "JCODE_SESSION_FEEDBACK_MAX_PROPOSALS": "3",
                "OPENAI_API_KEY": secret,
            },
            persisted={"max_output_tokens": 4096},
        )
        diagnostics = resolved["diagnostics"]
        rendered = self.feedback.canonical_json(diagnostics)
        self.assertNotIn(secret, rendered)
        for field in self.fixture["fields"]:
            with self.subTest(field=field):
                self.assertEqual(
                    diagnostics[field]["effective"], resolved["values"][field]
                )
                self.assertEqual(
                    diagnostics[field]["source"], resolved["sources"][field]
                )
                self.assertIn("configured", diagnostics[field])

    def test_budget_boundaries_pass_below_and_at_limit_and_fail_above(self) -> None:
        for case in self.fixture["budget_boundary_cases"]:
            field = case["field"]
            limit = case["limit"]
            for position in ("below", "at"):
                with self.subTest(field=field, position=position):
                    self.feedback.validate_budget_limit(
                        field=field,
                        observed=case[position]["observed"],
                        limit=limit,
                    )
            with (
                self.subTest(field=field, position="above"),
                self.assertRaisesRegex(self.feedback.ValidationError, field),
            ):
                self.feedback.validate_budget_limit(
                    field=field,
                    observed=case["above"]["observed"],
                    limit=limit,
                )


class LocalEntrypointTests(IsolatedSessionFeedbackTestCase):
    def visible_items(self) -> list[dict[str, str]]:
        return [
            {
                "reference": "outcome-1",
                "category": "visible_outcome",
                "summary": "The requested synthetic change was completed.",
            }
        ]

    def install_fake_bd(self) -> None:
        fake_bd = self.bin_dir / "bd"
        fake_bd.write_text(
            f"#!{sys.executable}\n"
            "import sys\n"
            "from pathlib import Path\n"
            "if 'init' in sys.argv:\n"
            "    (Path.cwd() / '.beads').mkdir(parents=True, exist_ok=True)\n",
            encoding="utf-8",
        )
        fake_bd.chmod(fake_bd.stat().st_mode | stat.S_IXUSR)

    def test_reusable_orchestrator_runs_generation_and_reports_complete_accounting(
        self,
    ) -> None:
        runner = mock.Mock(
            return_value={
                "returncode": 0,
                "stdout": self.feedback.canonical_json(
                    {"contract_version": "generator-response-v1", "proposals": []}
                ),
                "stderr": "",
                "elapsed_seconds": 0.25,
                "estimated_cost_usd": 0.01,
                "observed_input_tokens": 123,
                "observed_output_tokens": 45,
            }
        )
        result = self.feedback.run_feedback(
            requested_session_id=None,
            current_session_id="session-current-1",
            visible_session_ids=("session-current-1",),
            visible_items=self.visible_items(),
            librarian_summary_path=None,
            target_root=self.temp_dir.name,
            runner=runner,
        )

        self.assertEqual(result["status"], "zero_proposals")
        self.assertEqual(result["session_id"], "session-current-1")
        self.assertEqual(result["evidence_source"], "fallback")
        self.assertEqual(result["proposal_count"], 0)
        self.assertEqual(result["proposal_locations"], [])
        self.assertEqual(result["effective_config"]["model"], "gpt-5.6-sol")
        self.assertEqual(result["effective_config"]["effort"], "medium")
        self.assertGreater(result["accounting"]["evidence_bytes"], 0)
        self.assertEqual(result["accounting"]["excerpt_bytes"], 0)
        self.assertGreater(result["accounting"]["request_input"]["bytes"], 0)
        self.assertGreater(result["accounting"]["request_output"]["bytes"], 0)
        self.assertEqual(result["accounting"]["proposal_count"], 0)
        self.assertEqual(result["accounting"]["elapsed_seconds"], 0.25)
        self.assertEqual(result["accounting"]["estimated_cost_usd"], 0.01)
        self.assertEqual(result["accounting"]["observed_input_tokens"], 123)
        self.assertEqual(result["accounting"]["observed_output_tokens"], 45)
        runner.assert_called_once()

    def test_entrypoint_accepts_optional_session_id_and_visible_evidence_json(
        self,
    ) -> None:
        self.install_fake_bd()
        input_document = {
            "current_session_id": "session-current-1",
            "visible_session_ids": ["session-current-1", "session-visible-2"],
            "visible_items": self.visible_items(),
        }

        fake_jcode = self.bin_dir / "jcode"
        fake_jcode.write_text(
            f"#!{sys.executable}\n"
            f"print({jcode_json_report(self.feedback.canonical_json({'contract_version': 'generator-response-v1', 'proposals': []}))!r})\n",
            encoding="utf-8",
        )
        fake_jcode.chmod(fake_jcode.stat().st_mode | stat.S_IXUSR)

        cases = (
            ([], "session-current-1"),
            (["session-visible-2"], "session-visible-2"),
        )
        for arguments, expected_session_id in cases:
            with self.subTest(arguments=arguments):
                completed = subprocess.run(
                    [sys.executable, str(ENTRYPOINT_PATH), *arguments],
                    input=json.dumps(input_document),
                    text=True,
                    capture_output=True,
                    check=False,
                    env=os.environ.copy(),
                )

                self.assertEqual(completed.returncode, 0, completed.stderr)
                self.assertEqual(completed.stderr, "")
                result = json.loads(completed.stdout)
                self.assertEqual(result["status"], "zero_proposals")
                self.assertEqual(result["session_id"], expected_session_id)
                self.assertEqual(result["evidence_source"], "fallback")
                self.assertEqual(result["proposal_count"], 0)
                self.assertEqual(result["proposal_locations"], [])
                self.assertGreater(result["accounting"]["evidence_bytes"], 0)
                self.assertEqual(result["accounting"]["request_count"], 1)

    def test_entrypoint_forwards_an_explicit_real_librarian_summary_path(self) -> None:
        self.install_fake_bd()
        fixture = json.loads(PRIVACY_FIXTURE_PATH.read_text(encoding="utf-8"))
        summary_path = Path(self.temp_dir.name) / "summary.json"
        summary_path.write_text(
            json.dumps(fixture["inputs"]["compatible_librarian"]),
            encoding="utf-8",
        )
        fake_jcode = self.bin_dir / "jcode"
        fake_jcode.write_text(
            f"#!{sys.executable}\n"
            f"print({jcode_json_report(self.feedback.canonical_json({'contract_version': 'generator-response-v1', 'proposals': []}))!r})\n",
            encoding="utf-8",
        )
        fake_jcode.chmod(fake_jcode.stat().st_mode | stat.S_IXUSR)
        input_document = {
            "current_session_id": "session-synthetic-001",
            "visible_session_ids": ["session-synthetic-001"],
            "visible_items": self.visible_items(),
            "librarian_summary_path": str(summary_path),
        }

        completed = subprocess.run(
            [sys.executable, str(ENTRYPOINT_PATH)],
            input=json.dumps(input_document),
            text=True,
            capture_output=True,
            check=False,
            env=os.environ.copy(),
        )

        self.assertEqual(completed.returncode, 0, completed.stderr)
        result = json.loads(completed.stdout)
        self.assertEqual(result["evidence_source"], "librarian")
        self.assertEqual(result["session_id"], "session-synthetic-001")
        self.assertEqual(result["accounting"]["request_count"], 1)

    def test_entrypoint_rejects_oversized_input_actionably(self) -> None:
        completed = subprocess.run(
            [sys.executable, str(ENTRYPOINT_PATH)],
            input=" " * (256 * 1024 + 1),
            text=True,
            capture_output=True,
            check=False,
            env=os.environ.copy(),
        )

        self.assertEqual(completed.returncode, 2)
        self.assertEqual(completed.stdout, "")
        self.assertIn("input", completed.stderr.lower())
        self.assertIn("limit", completed.stderr.lower())

    def test_entrypoint_returns_non_success_for_generation_schema_failure(self) -> None:
        self.install_fake_bd()
        fake_jcode = self.bin_dir / "jcode"
        fake_jcode.write_text(
            f"#!{sys.executable}\nprint({jcode_json_report('{}')!r})\n",
            encoding="utf-8",
        )
        fake_jcode.chmod(fake_jcode.stat().st_mode | stat.S_IXUSR)
        input_document = {
            "current_session_id": "session-current-1",
            "visible_session_ids": ["session-current-1"],
            "visible_items": self.visible_items(),
        }

        completed = subprocess.run(
            [sys.executable, str(ENTRYPOINT_PATH)],
            input=json.dumps(input_document),
            text=True,
            capture_output=True,
            check=False,
            env=os.environ.copy(),
        )

        self.assertEqual(completed.returncode, 2)
        self.assertEqual(completed.stdout, "")
        self.assertIn("generator-response-v1", completed.stderr)


class StandaloneCopiedSkillTests(IsolatedSessionFeedbackTestCase):
    def visible_items(self) -> list[dict[str, str]]:
        return [
            {
                "reference": "outcome-1",
                "category": "visible_outcome",
                "summary": "The copied synthetic workflow completed with one review candidate.",
                "relevant_path": ".jcode/skills/example/SKILL.md",
            }
        ]

    def generator_response(self) -> dict[str, object]:
        proposal = valid_proposal()
        proposal["fingerprint"] = self.feedback.proposal_fingerprint(
            {
                "category": proposal["target"]["category"],
                "scope": proposal["target"]["scope"],
                "concrete_target": proposal["target"]["concrete_target"],
                "problem": proposal["problem"],
                "intended_outcome": proposal["expected_benefit"],
            }
        )
        return {
            "contract_version": "generator-response-v1",
            "proposals": [proposal],
        }

    @staticmethod
    def write_executable(path: Path, body: str) -> None:
        path.write_text(f"#!{sys.executable}\n{body}", encoding="utf-8")
        path.chmod(path.stat().st_mode | stat.S_IXUSR)

    def install_fake_bd(self, bin_dir: Path) -> Path:
        log_path = bin_dir.parent / "bd-invocations.jsonl"
        self.write_executable(
            bin_dir / "bd",
            """import json
import os
import sys
from pathlib import Path

cwd = Path.cwd()
with Path(os.environ["SESSION_FEEDBACK_BD_LOG"]).open("a", encoding="utf-8") as log:
    log.write(json.dumps({"argv": sys.argv[1:], "cwd": str(cwd)}) + "\\n")
mode = os.environ.get("SESSION_FEEDBACK_BD_MODE", "success")
if "init" in sys.argv:
    if mode == "permission":
        print("synthetic permission denied", file=sys.stderr)
        raise SystemExit(13)
    if mode == "partial":
        (cwd / ".beads").mkdir(parents=True, exist_ok=True)
        (cwd / ".beads" / "partial-init").write_text("incomplete", encoding="utf-8")
        print("synthetic partial initialization", file=sys.stderr)
        raise SystemExit(23)
    (cwd / ".beads").mkdir(parents=True, exist_ok=True)
elif "list" in sys.argv:
    print("[]")
elif "create" in sys.argv:
    print('{"id":"feedback-test-001"}')
else:
    print("unexpected fake bd command", file=sys.stderr)
    raise SystemExit(91)
""",
        )
        return log_path

    def install_fake_jcode(
        self,
        bin_dir: Path,
        *,
        response: dict[str, object] | None = None,
        malformed: bool = False,
        sleep_seconds: float = 0.0,
    ) -> Path:
        log_path = bin_dir.parent / "jcode-invocations.log"
        output = (
            "{}"
            if malformed
            else self.feedback.canonical_json(response or self.generator_response())
        )
        report = jcode_json_report(output)
        self.write_executable(
            bin_dir / "jcode",
            "import os\n"
            "import time\n"
            "from pathlib import Path\n"
            "Path(os.environ['SESSION_FEEDBACK_JCODE_LOG']).write_text('called', encoding='utf-8')\n"
            f"time.sleep({sleep_seconds!r})\n"
            f"print({report!r})\n",
        )
        return log_path

    @staticmethod
    def file_manifest(root: Path) -> dict[str, str]:
        return {
            str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted(root.rglob("*"))
            if path.is_file()
        }

    def run_entrypoint(
        self,
        *,
        entrypoint: Path,
        home: Path,
        bin_dir: Path,
        working_directory: Path,
        session_arguments: list[str],
        extra_environment: dict[str, str] | None = None,
    ) -> subprocess.CompletedProcess[str]:
        legacy_auth = home / ".codex" / "auth.json"
        legacy_auth.parent.mkdir(parents=True, exist_ok=True)
        legacy_auth.write_text(
            json.dumps({"tokens": {"access_token": "synthetic-test-token"}}),
            encoding="utf-8",
        )
        legacy_auth.chmod(0o600)
        environment = os.environ.copy()
        environment.update(
            {
                "HOME": str(home),
                "PATH": str(bin_dir),
                "SESSION_FEEDBACK_BD_LOG": str(bin_dir.parent / "bd-invocations.jsonl"),
                "SESSION_FEEDBACK_JCODE_LOG": str(
                    bin_dir.parent / "jcode-invocations.log"
                ),
            }
        )
        environment.update(extra_environment or {})
        return subprocess.run(
            [sys.executable, str(entrypoint), *session_arguments],
            input=json.dumps(
                {
                    "current_session_id": "session-current-1",
                    "visible_session_ids": [
                        "session-current-1",
                        "session-visible-2",
                    ],
                    "visible_items": self.visible_items(),
                }
            ),
            text=True,
            capture_output=True,
            check=False,
            cwd=working_directory,
            env=environment,
        )

    def test_project_and_unchanged_global_copies_bootstrap_clean_homes(self) -> None:
        for location, session_arguments, expected_session_id in (
            ("project", [], "session-current-1"),
            ("project", ["session-visible-2"], "session-visible-2"),
            ("global", [], "session-current-1"),
            ("global", ["session-visible-2"], "session-visible-2"),
        ):
            with self.subTest(location=location, session_arguments=session_arguments):
                case_root = Path(self.temp_dir.name) / (
                    f"success-{location}-{'named' if session_arguments else 'current'}"
                )
                home = case_root / "home"
                bin_dir = case_root / "bin"
                working_directory = case_root / "work"
                home.mkdir(parents=True)
                bin_dir.mkdir()
                target = (
                    working_directory / ".jcode" / "skills" / "example" / "SKILL.md"
                )
                target.parent.mkdir(parents=True)
                target.write_text("# Synthetic target\n", encoding="utf-8")
                target_before = target.read_bytes()

                if location == "global":
                    skill_root = home / ".jcode" / "skills" / "session-feedback"
                    shutil.copytree(SKILL_DIR, skill_root)
                    self.assertEqual(
                        self.file_manifest(skill_root), self.file_manifest(SKILL_DIR)
                    )
                    entrypoint = skill_root / "__main__.py"
                else:
                    entrypoint = ENTRYPOINT_PATH

                bd_log = self.install_fake_bd(bin_dir)
                self.install_fake_jcode(bin_dir)
                completed = self.run_entrypoint(
                    entrypoint=entrypoint,
                    home=home,
                    bin_dir=bin_dir,
                    working_directory=working_directory,
                    session_arguments=session_arguments,
                )

                self.assertEqual(completed.returncode, 0, completed.stderr)
                self.assertEqual(completed.stderr, "")
                result = json.loads(completed.stdout)
                self.assertEqual(result["status"], "proposals_generated")
                self.assertEqual(result["session_id"], expected_session_id)
                self.assertEqual(result["proposal_count"], 1)
                feedback_root = home / ".jcode" / "feedback"
                for path in (
                    feedback_root,
                    feedback_root / "runs",
                    feedback_root / "proposals",
                    feedback_root / ".beads",
                ):
                    self.assertTrue(path.is_dir(), path)
                self.assertFalse((feedback_root / ".beads" / "remotes.json").exists())
                self.assertTrue(result["proposal_locations"])
                for location_path in result["proposal_locations"]:
                    self.assertTrue(Path(location_path).is_relative_to(feedback_root))
                for generated in feedback_root.rglob("*"):
                    self.assertTrue(generated.is_relative_to(feedback_root))
                commands = [
                    json.loads(line)
                    for line in bd_log.read_text(encoding="utf-8").splitlines()
                ]
                self.assertTrue(commands)
                for command in commands:
                    self.assertTrue(Path(command["cwd"]).is_relative_to(feedback_root))
                    self.assertFalse(
                        {"remote", "replicate", "sync", "push", "pull"}
                        & {part.lower() for part in command["argv"]}
                    )
                self.assertEqual(target.read_bytes(), target_before)

    def test_bootstrap_and_generation_failures_are_atomic_and_visible(self) -> None:
        cases = (
            ("permission", True, False, 0.0, {}, "permission"),
            ("partial", True, False, 0.0, {}, "partial"),
            ("missing-jcode", False, False, 0.0, {}, "could not start"),
            ("malformed", True, True, 0.0, {}, "generator-response-v1"),
            (
                "timeout",
                True,
                False,
                0.25,
                {"JCODE_SESSION_FEEDBACK_MAX_ELAPSED_SECONDS": "0.05"},
                "elapsed-time",
            ),
        )
        for (
            mode,
            install_jcode,
            malformed,
            sleep_seconds,
            extra_env,
            error_text,
        ) in cases:
            with self.subTest(mode=mode):
                case_root = Path(self.temp_dir.name) / f"failure-{mode}"
                home = case_root / "home"
                bin_dir = case_root / "bin"
                working_directory = case_root / "work"
                home.mkdir(parents=True)
                bin_dir.mkdir()
                working_directory.mkdir()
                target = working_directory / "target-sentinel.txt"
                target.write_text("must remain unchanged", encoding="utf-8")
                self.install_fake_bd(bin_dir)
                if install_jcode:
                    self.install_fake_jcode(
                        bin_dir,
                        malformed=malformed,
                        sleep_seconds=sleep_seconds,
                    )

                completed = self.run_entrypoint(
                    entrypoint=ENTRYPOINT_PATH,
                    home=home,
                    bin_dir=bin_dir,
                    working_directory=working_directory,
                    session_arguments=[],
                    extra_environment={
                        "SESSION_FEEDBACK_BD_MODE": mode,
                        **extra_env,
                    },
                )

                self.assertNotEqual(completed.returncode, 0)
                self.assertEqual(completed.stdout, "")
                self.assertIn(error_text, completed.stderr.lower())
                self.assertEqual(
                    target.read_text(encoding="utf-8"), "must remain unchanged"
                )
                feedback_root = home / ".jcode" / "feedback"
                if feedback_root.exists():
                    self.assertEqual(list(feedback_root.rglob("*.json")), [])
                    self.assertEqual(list(feedback_root.rglob("*.md")), [])
                    self.assertEqual(list(feedback_root.rglob("*.tmp")), [])
                self.assertFalse((working_directory / ".beads").exists())


class GenerationIsolationProcessTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp_dir.cleanup)
        self.root = Path(self.temp_dir.name)
        self.home = self.root / "source-home"
        self.bin_dir = self.root / "bin"
        self.observation = self.root / "observation.jsonl"
        self.home.mkdir()
        self.bin_dir.mkdir()
        legacy_auth = self.home / ".codex" / "auth.json"
        legacy_auth.parent.mkdir()
        legacy_auth.write_text(
            json.dumps(
                {
                    "tokens": {
                        "access_token": "test-access-secret",
                        "refresh_token": "test-refresh-secret",
                        "id_token": "test-id-secret",
                        "account_id": "test-account-secret",
                    },
                    "OPENAI_API_KEY": None,
                }
            ),
            encoding="utf-8",
        )
        legacy_auth.chmod(0o600)
        (self.home / "AGENTS.md").write_text("must not load", encoding="utf-8")
        skill = self.home / ".jcode" / "skills" / "must-not-load" / "SKILL.md"
        skill.parent.mkdir(parents=True)
        skill.write_text("must not load", encoding="utf-8")

        fake_jcode = self.bin_dir / "jcode"
        fake_jcode.write_text(
            "#!/usr/bin/env python3\n"
            "import json, os, sys\n"
            "from pathlib import Path\n"
            "observation = Path(os.environ['SESSION_FEEDBACK_OBSERVATION'])\n"
            "record = {\n"
            "  'argv': sys.argv[1:],\n"
            "  'cwd': os.getcwd(),\n"
            "  'home': os.environ.get('HOME'),\n"
            "  'jcode_home': os.environ.get('JCODE_HOME'),\n"
            "  'runtime': os.environ.get('JCODE_RUNTIME_DIR'),\n"
            "  'temp_server': os.environ.get('JCODE_TEMP_SERVER'),\n"
            "  'owner_pid': os.environ.get('JCODE_SERVER_OWNER_PID'),\n"
            "  'api_key_present': bool(os.environ.get('OPENAI_API_KEY')),\n"
            "  'legacy_auth_present': (Path(os.environ['JCODE_HOME']) / 'external/.codex/auth.json').is_file(),\n"
            "  'global_agents_present': (Path(os.environ['HOME']) / 'AGENTS.md').exists(),\n"
            "  'global_skill_present': (Path(os.environ['JCODE_HOME']) / 'skills/must-not-load/SKILL.md').exists(),\n"
            "}\n"
            "with observation.open('a', encoding='utf-8') as handle:\n"
            "  handle.write(json.dumps(record, sort_keys=True) + '\\n')\n"
            "if 'run' in sys.argv:\n"
            "  response = {'contract_version': 'generator-response-v1', 'proposals': []}\n"
            "  print(json.dumps({\n"
            "    'text': json.dumps(response, separators=(',', ':')),\n"
            "    'usage': {'input_tokens': 321, 'output_tokens': 17, 'cache_read_input_tokens': 0},\n"
            "  }))\n",
            encoding="utf-8",
        )
        fake_jcode.chmod(0o700)
        self.environment = mock.patch.dict(
            os.environ,
            {
                "HOME": str(self.home),
                "PATH": f"{self.bin_dir}:{os.environ.get('PATH', '')}",
                "SESSION_FEEDBACK_OBSERVATION": str(self.observation),
                "OPENAI_API_KEY": "must-not-reach-child",
            },
            clear=False,
        )
        self.environment.start()
        self.addCleanup(self.environment.stop)
        self.feedback = load_helper()

    def test_default_runner_isolates_startup_context_and_stops_owned_server(
        self,
    ) -> None:
        receipt = self.feedback._run_generation_command(
            [
                "jcode",
                "run",
                "--model",
                "openai-oauth:gpt-5.6-sol",
                "--json",
                "prompt",
            ],
            timeout_seconds=5.0,
            estimated_cost_usd=0.1,
        )

        self.assertEqual(
            json.loads(receipt["stdout"]),
            {"contract_version": "generator-response-v1", "proposals": []},
        )
        self.assertEqual(receipt["observed_input_tokens"], 321)
        self.assertEqual(receipt["observed_output_tokens"], 17)
        records = [
            json.loads(line)
            for line in self.observation.read_text(encoding="utf-8").splitlines()
        ]
        self.assertEqual(len(records), 2)
        run_record, stop_record = records
        self.assertIn("run", run_record["argv"])
        self.assertIn("server", stop_record["argv"])
        self.assertIn("stop", stop_record["argv"])
        self.assertEqual(run_record["temp_server"], "1")
        self.assertEqual(run_record["owner_pid"], str(os.getpid()))
        self.assertFalse(run_record["api_key_present"])
        self.assertTrue(run_record["legacy_auth_present"])
        self.assertFalse(run_record["global_agents_present"])
        self.assertFalse(run_record["global_skill_present"])
        self.assertNotEqual(run_record["home"], str(self.home))
        self.assertNotEqual(run_record["jcode_home"], str(self.home / ".jcode"))
        self.assertEqual(run_record["cwd"], str(Path(run_record["home"]) / "workspace"))
        self.assertEqual(
            stat.S_IMODE((self.home / ".codex/auth.json").stat().st_mode), 0o600
        )


if __name__ == "__main__":
    unittest.main()
