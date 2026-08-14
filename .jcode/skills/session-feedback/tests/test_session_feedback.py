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
            side_effect=AssertionError("session-feedback tests must not access the network"),
        )
        self.network_connection.start()
        self.addCleanup(self.network_connection.stop)
        self.urlopen = mock.patch(
            "urllib.request.urlopen",
            side_effect=AssertionError("session-feedback tests must not access the network"),
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

    def test_current_session_uses_supplied_visible_evidence_on_clean_first_run(self) -> None:
        self.assertFalse((self.home / ".jcode" / "feedback").exists())
        self.assertFalse((self.home / ".jcode" / "skills" / "session-librarian").exists())
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

    def test_missing_current_session_fails_actionably_without_creating_a_run(self) -> None:
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

    def test_invisible_named_session_fails_before_generation_or_persistence(self) -> None:
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

    def test_review_outcomes_keep_success_failure_and_persistence_distinct(self) -> None:
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
        path = self.write_librarian_summary(librarian) if librarian is not None else None
        return self.feedback.acquire_evidence(
            session_id=self.session_id,
            librarian_summary_path=path,
            visible_items=self.fallback_items if visible_items is None else visible_items,
        )

    def assert_sentinels_absent(self, value: object, surface: str) -> None:
        serialized = self.feedback.canonical_json(value)
        for sentinel in self.sentinels:
            with self.subTest(surface=surface, sentinel=sentinel):
                self.assertNotIn(sentinel, serialized)

    def test_librarian_and_fallback_normalize_to_the_same_v1_items(self) -> None:
        librarian = self.acquire(librarian=self.fixture["inputs"]["compatible_librarian"])
        fallback = self.acquire(librarian=None)

        self.assertEqual(librarian["contract_version"], "evidence-v1")
        self.assertEqual(fallback["contract_version"], "evidence-v1")
        self.assertEqual(librarian["source"], "librarian")
        self.assertEqual(fallback["source"], "fallback")
        self.assertEqual(librarian["items"], fallback["items"])
        self.assertEqual(librarian["accounting"], fallback["accounting"])
        self.feedback.validate_contract("evidence-v1", librarian)
        self.feedback.validate_contract("evidence-v1", fallback)

    def test_unsupported_librarian_falls_back_only_with_sufficient_visible_evidence(self) -> None:
        unsupported = copy.deepcopy(self.fixture["inputs"]["compatible_librarian"])
        unsupported["summary_version"] = "librarian-summary-v2"

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
        evidence = self.acquire(librarian=self.fixture["inputs"]["compatible_librarian"])
        shortlist = self.feedback.shortlist_targets(evidence)
        prompt = self.feedback.canonical_json(
            {"evidence": evidence, "shortlist": shortlist, "excerpts": []}
        )
        generator_recording = {"request_input": prompt, "request_count": 1}
        generator_output = {"contract_version": "generator-response-v1", "proposals": []}
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

        with mock.patch("builtins.open", side_effect=AssertionError("pre-shortlist target read")):
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
            "excerpt_bytes": self.feedback.excerpt_accounting(excerpts)["serialized_bytes"],
        }
        self.assertEqual(
            request_accounting["excerpt_bytes"],
            self.feedback.excerpt_accounting(excerpts)["serialized_bytes"],
        )

    def test_both_acquisition_paths_reconcile_privacy_reads_and_accounting(self) -> None:
        target_root = Path(self.temp_dir.name) / "targets"
        skill_path = target_root / ".jcode" / "skills" / "session-feedback" / "SKILL.md"
        skill_path.parent.mkdir(parents=True)
        skill_path.write_text("bounded session feedback excerpt with é", encoding="utf-8")

        acquired = {
            "librarian": self.acquire(
                librarian=self.fixture["inputs"]["compatible_librarian"]
            ),
            "fallback": self.acquire(librarian=None),
        }
        expected_items = acquired["fallback"]["items"]
        expected_accounting = self.feedback.evidence_accounting(expected_items)

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
                self.assertEqual(evidence["items"], expected_items)
                self.assertEqual(evidence["accounting"], expected_accounting)
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
                    "fingerprint": self.feedback.proposal_fingerprint(fingerprint_input),
                }
            )
            if baseline is None:
                baseline = result
            self.assertEqual(result, baseline)

    def test_boundary_failures_are_actionable_and_have_no_external_effects(self) -> None:
        home_before = sorted(path.relative_to(self.home) for path in self.home.rglob("*"))
        malformed = copy.deepcopy(self.fixture["inputs"]["compatible_librarian"])
        malformed["items"] = {"unexpected": "not-an-array"}

        with self.assertRaises(self.feedback.ValidationError) as acquisition_error:
            self.acquire(librarian=malformed, visible_items=[])
        self.assertIn("librarian", str(acquisition_error.exception).lower())
        self.assertIn("fallback", str(acquisition_error.exception).lower())

        opener = mock.Mock(side_effect=AssertionError("invalid shortlist must not read files"))
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


class LocalEntrypointTests(IsolatedSessionFeedbackTestCase):
    def visible_items(self) -> list[dict[str, str]]:
        return [
            {
                "reference": "outcome-1",
                "category": "visible_outcome",
                "summary": "The requested synthetic change was completed.",
            }
        ]

    def test_reusable_orchestrator_reports_zero_proposals_and_accounting(self) -> None:
        result = self.feedback.run_feedback(
            requested_session_id=None,
            current_session_id="session-current-1",
            visible_session_ids=("session-current-1",),
            visible_items=self.visible_items(),
            librarian_summary_path=None,
        )

        self.assertEqual(result["status"], "zero_proposals")
        self.assertEqual(result["session_id"], "session-current-1")
        self.assertEqual(result["evidence_source"], "fallback")
        self.assertEqual(result["proposal_count"], 0)
        self.assertEqual(result["proposal_locations"], [])
        self.assertGreater(result["accounting"]["serialized_bytes"], 0)
        self.assertGreater(result["accounting"]["estimated_tokens"], 0)

    def test_entrypoint_accepts_optional_session_id_and_visible_evidence_json(self) -> None:
        input_document = {
            "current_session_id": "session-current-1",
            "visible_session_ids": ["session-current-1", "session-visible-2"],
            "visible_items": self.visible_items(),
        }

        cases = (([], "session-current-1"), (["session-visible-2"], "session-visible-2"))
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
                self.assertGreater(result["accounting"]["serialized_bytes"], 0)

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


if __name__ == "__main__":
    unittest.main()
