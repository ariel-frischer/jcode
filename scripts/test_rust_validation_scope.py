#!/usr/bin/env python3
"""Contract tests for conservative Rust validation-scope resolution."""

from __future__ import annotations

import importlib.util
import io
import json
import os
from pathlib import Path
import unittest
from contextlib import redirect_stderr, redirect_stdout
from unittest import mock


SCRIPTS_DIR = Path(__file__).resolve().parent
RESOLVER_MODULE_PATH = SCRIPTS_DIR / "rust_validation_scope.py"


def load_resolver_module(*, require_discovery: bool = False):
    """Load the implementation while reporting a contract failure, not ImportError."""
    if not RESOLVER_MODULE_PATH.is_file():
        raise AssertionError(
            f"missing validation-scope implementation: {RESOLVER_MODULE_PATH}"
        )
    spec = importlib.util.spec_from_file_location(
        "rust_validation_scope", RESOLVER_MODULE_PATH
    )
    if spec is None or spec.loader is None:
        raise AssertionError(
            f"unable to create an import specification for {RESOLVER_MODULE_PATH}"
        )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    if not callable(getattr(module, "resolve_validation_scope", None)):
        raise AssertionError(
            "validation-scope implementation must define callable "
            "resolve_validation_scope()"
        )
    if require_discovery and not callable(
        getattr(module, "evaluate_test_discovery_snapshot", None)
    ):
        raise AssertionError(
            "validation-scope implementation must define callable "
            "evaluate_test_discovery_snapshot()"
        )
    return module


def cargo_metadata() -> dict[str, object]:
    """Return a small Cargo-metadata-shaped workspace fixture."""
    return {
        "workspace_root": "/repo",
        "workspace_members": [
            "alpha 0.1.0 (path+file:///repo/crates/alpha)",
            "beta 0.1.0 (path+file:///repo/crates/beta)",
        ],
        "packages": [
            {
                "id": "alpha 0.1.0 (path+file:///repo/crates/alpha)",
                "name": "alpha",
                "manifest_path": "/repo/crates/alpha/Cargo.toml",
                "features": {
                    "default": ["serde"],
                    "serde": [],
                    "telemetry": [],
                },
                "targets": [
                    {
                        "name": "alpha",
                        "kind": ["lib"],
                        "src_path": "/repo/crates/alpha/src/lib.rs",
                    },
                    {
                        "name": "alpha-cli",
                        "kind": ["bin"],
                        "src_path": "/repo/crates/alpha/src/bin/alpha-cli.rs",
                    },
                    {
                        "name": "api_contract",
                        "kind": ["test"],
                        "src_path": "/repo/crates/alpha/tests/api_contract.rs",
                    },
                ],
            },
            {
                "id": "beta 0.1.0 (path+file:///repo/crates/beta)",
                "name": "beta",
                "manifest_path": "/repo/crates/beta/Cargo.toml",
                "features": {"default": []},
                "targets": [
                    {
                        "name": "beta",
                        "kind": ["lib"],
                        "src_path": "/repo/crates/beta/src/lib.rs",
                    }
                ],
            },
        ],
    }


def path_rules() -> dict[str, object]:
    """Inject deterministic repository-owned safety and optional-feature rules."""
    return {
        "required_features": {
            "crates/alpha/src/telemetry/": {"alpha": ["telemetry"]},
        },
        "generated_prefixes": ["target/", "crates/alpha/src/generated/"],
        "build_script_names": ["build.rs"],
        "workspace_paths": ["Cargo.toml", "Cargo.lock", ".cargo/"],
        "guardrail_paths": ["scripts/check_guardrails.sh"],
        "release_sensitive_paths": [
            "scripts/install_release.sh",
            "scripts/check_release_version.sh",
        ],
    }


def resolve(
    module,
    paths: list[str],
    *,
    explicit: dict[str, object] | None = None,
    defaults: dict[str, object] | None = None,
):
    return module.resolve_validation_scope(
        cargo_metadata(),
        paths,
        explicit_inputs=explicit,
        defaults=defaults,
        path_rules=path_rules(),
    )


def discovery_snapshot(**overrides: object) -> dict[str, object]:
    """Return a complete, bounded discovery snapshot for one exact scope."""
    snapshot: dict[str, object] = {
        "schema_version": "1.0",
        "source_fingerprint": "source-state-a",
        "package": "alpha",
        "target": "test:api_contract",
        "features": ["serde", "telemetry"],
        "harness": "cargo-test",
        "discovery_id": "cargo-test-list-v1",
        "complete": True,
        "test_names": [
            "api::creates_record",
            "api::creates_record_when_optional",
            "api::deletes_record",
            "ignored::migration_fixture",
        ],
    }
    snapshot.update(overrides)
    return snapshot


def discovery_request(**overrides: object) -> dict[str, object]:
    """Return the exact request identity paired with discovery_snapshot()."""
    request: dict[str, object] = {
        "source_fingerprint": "source-state-a",
        "package": "alpha",
        "target": "test:api_contract",
        "features": ["telemetry", "serde"],
        "harness": "cargo-test",
        "discovery_id": "cargo-test-list-v1",
        "filter": "creates_record",
        "exact": False,
        "ignored": False,
    }
    request.update(overrides)
    return request


class ValidationScopeContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.resolver = load_resolver_module()

    def assert_broad_fallback(self, result, reason_fragment: str) -> None:
        self.assertEqual(result["effective_scope"]["mode"], "broad")
        self.assertEqual(result["resolution_source"], "conservative_fallback")
        self.assertIn(reason_fragment, result["fallback_reason"])

    def test_single_package_library_path_resolves_focused_package_and_target(self):
        result = resolve(self.resolver, ["crates/alpha/src/lib.rs"])

        self.assertEqual(result["resolution_source"], "inferred")
        self.assertEqual(result["effective_scope"]["mode"], "focused")
        self.assertEqual(result["effective_scope"]["packages"], ["alpha"])
        self.assertEqual(result["effective_scope"]["targets"], ["lib:alpha"])
        self.assertEqual(result["affected_paths"], ["crates/alpha/src/lib.rs"])

    def test_binary_and_integration_test_paths_preserve_target_ownership(self):
        bin_result = resolve(
            self.resolver, ["crates/alpha/src/bin/alpha-cli.rs"]
        )
        test_result = resolve(
            self.resolver, ["crates/alpha/tests/api_contract.rs"]
        )

        self.assertEqual(bin_result["effective_scope"]["targets"], ["bin:alpha-cli"])
        self.assertEqual(
            test_result["effective_scope"]["targets"], ["test:api_contract"]
        )

    def test_cross_package_paths_fall_back_to_broad_scope(self):
        result = resolve(
            self.resolver,
            ["crates/alpha/src/lib.rs", "crates/beta/src/lib.rs"],
        )

        self.assert_broad_fallback(result, "cross-package")

    def test_cross_package_fallback_preserves_explicit_feature_precedence(self):
        result = resolve(
            self.resolver,
            ["crates/alpha/src/lib.rs", "crates/beta/src/lib.rs"],
            explicit={
                "features": ["telemetry"],
                "no_default_features": True,
                "all_features": False,
            },
            defaults={"features": ["serde"]},
        )

        self.assert_broad_fallback(result, "cross-package")
        self.assertEqual(result["effective_scope"]["features"], ["telemetry"])
        self.assertTrue(result["effective_scope"]["no_default_features"])
        self.assertFalse(result["effective_scope"]["all_features"])

    def test_generated_build_workspace_unknown_guardrail_and_release_paths_fall_back(self):
        cases = {
            "target/generated.rs": "generated",
            "crates/alpha/build.rs": "build script",
            "Cargo.lock": "workspace",
            "docs/unowned.md": "unknown",
            "scripts/check_guardrails.sh": "guardrail",
            "scripts/install_release.sh": "release-sensitive",
        }
        for path, reason in cases.items():
            with self.subTest(path=path):
                self.assert_broad_fallback(resolve(self.resolver, [path]), reason)

    def test_explicit_package_target_and_features_take_precedence(self):
        explicit = {
            "packages": ["alpha"],
            "targets": ["bin:alpha-cli"],
            "features": ["serde"],
            "no_default_features": True,
            "all_features": False,
        }

        result = resolve(
            self.resolver,
            ["crates/alpha/src/lib.rs"],
            explicit=explicit,
            defaults={"features": ["telemetry"]},
        )

        self.assertEqual(result["resolution_source"], "explicit")
        self.assertEqual(result["explicit_inputs"], explicit)
        self.assertEqual(result["effective_scope"]["packages"], ["alpha"])
        self.assertEqual(result["effective_scope"]["targets"], ["bin:alpha-cli"])
        self.assertEqual(result["effective_scope"]["features"], ["serde"])
        self.assertTrue(result["effective_scope"]["no_default_features"])
        self.assertFalse(result["effective_scope"]["all_features"])

    def test_explicit_all_features_is_preserved_without_inferred_feature_changes(self):
        result = resolve(
            self.resolver,
            ["crates/alpha/src/telemetry/export.rs"],
            explicit={"all_features": True},
            defaults={"features": ["serde"]},
        )

        self.assertEqual(result["resolution_source"], "explicit")
        self.assertTrue(result["effective_scope"]["all_features"])
        self.assertEqual(result["effective_scope"]["features"], [])

    def test_empty_and_unset_explicit_values_do_not_override_inference(self):
        unset_result = resolve(self.resolver, ["crates/alpha/src/lib.rs"])
        empty_result = resolve(
            self.resolver,
            ["crates/alpha/src/lib.rs"],
            explicit={
                "packages": [],
                "targets": [],
                "features": [],
                "no_default_features": None,
                "all_features": None,
            },
        )

        self.assertEqual(unset_result["effective_scope"], empty_result["effective_scope"])
        self.assertEqual(empty_result["resolution_source"], "inferred")

    def test_defaults_apply_only_when_no_explicit_or_inferred_selection_exists(self):
        defaults = {
            "mode": "broad",
            "features": ["serde"],
            "no_default_features": False,
            "all_features": False,
        }

        result = resolve(self.resolver, [], defaults=defaults)

        self.assertEqual(result["resolution_source"], "default")
        self.assertEqual(result["effective_scope"]["mode"], "broad")
        self.assertEqual(result["effective_scope"]["features"], ["serde"])

    def test_unknown_explicit_package_and_target_fail_actionably(self):
        cases = [
            ({"packages": ["missing"]}, "package"),
            (
                {"packages": ["alpha"], "targets": ["bin:missing"]},
                "target",
            ),
        ]
        for explicit, message in cases:
            with self.subTest(explicit=explicit):
                with self.assertRaisesRegex(ValueError, message):
                    resolve(
                        self.resolver,
                        ["crates/alpha/src/lib.rs"],
                        explicit=explicit,
                    )

    def test_conflicting_package_and_target_fail_actionably(self):
        with self.assertRaisesRegex(ValueError, "package|target|conflict"):
            resolve(
                self.resolver,
                ["crates/beta/src/lib.rs"],
                explicit={
                    "packages": ["beta"],
                    "targets": ["bin:alpha-cli"],
                },
            )

    def test_unsupported_explicit_value_types_fail_actionably(self):
        with self.assertRaisesRegex(ValueError, "features"):
            resolve(
                self.resolver,
                ["crates/alpha/src/lib.rs"],
                explicit={"features": "telemetry"},
            )

    def test_optional_feature_path_infers_the_required_feature(self):
        result = resolve(
            self.resolver, ["crates/alpha/src/telemetry/export.rs"]
        )

        self.assertEqual(result["effective_scope"]["mode"], "focused")
        self.assertEqual(result["effective_scope"]["packages"], ["alpha"])
        self.assertIn("telemetry", result["effective_scope"]["features"])

    def test_explicit_insufficient_features_cannot_report_focused_success(self):
        with self.assertRaisesRegex(ValueError, "telemetry|required feature"):
            resolve(
                self.resolver,
                ["crates/alpha/src/telemetry/export.rs"],
                explicit={
                    "packages": ["alpha"],
                    "features": ["serde"],
                    "no_default_features": True,
                },
            )

    def test_optional_feature_path_can_use_correctness_preserving_broad_scope(self):
        rules = path_rules()
        rules["required_features"] = {}

        result = self.resolver.resolve_validation_scope(
            cargo_metadata(),
            ["crates/alpha/src/telemetry/export.rs"],
            explicit_inputs=None,
            defaults=None,
            path_rules=rules,
        )

        self.assert_broad_fallback(result, "optional feature")


class TestDiscoverySnapshotContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.resolver = load_resolver_module(require_discovery=True)

    def evaluate(
        self,
        *,
        snapshot: dict[str, object] | None = None,
        request: dict[str, object] | None = None,
    ) -> dict[str, object]:
        return self.resolver.evaluate_test_discovery_snapshot(
            discovery_snapshot() if snapshot is None else snapshot,
            discovery_request() if request is None else request,
        )

    def assert_unknown(self, result: dict[str, object], reason: str) -> None:
        self.assertEqual(result["schema_version"], "1.0")
        self.assertEqual(result["decision"], "unknown")
        self.assertIn(reason, result["reason"])

    def test_complete_exact_identity_snapshot_can_prove_matches(self):
        result = self.evaluate()

        self.assertEqual(result["decision"], "matches")
        self.assertEqual(
            result["proof_provenance"],
            "current complete test-discovery snapshot",
        )
        self.assertEqual(result["matched_test_names"], [
            "api::creates_record",
            "api::creates_record_when_optional",
        ])
        self.assertEqual(result["effective_scope"]["features"], ["serde", "telemetry"])

    def test_complete_exact_identity_snapshot_can_prove_zero_substring_matches(self):
        result = self.evaluate(
            request=discovery_request(filter="does_not_exist", exact=False)
        )

        self.assertEqual(result["decision"], "empty")
        self.assertEqual(result["matched_test_names"], [])
        self.assertEqual(
            result["proof_provenance"],
            "current complete test-discovery snapshot",
        )

    def test_exact_name_filtering_is_distinct_from_cargo_substring_filtering(self):
        substring = self.evaluate(
            request=discovery_request(filter="api::creates_record", exact=False)
        )
        exact = self.evaluate(
            request=discovery_request(filter="api::creates_record", exact=True)
        )
        exact_zero = self.evaluate(
            request=discovery_request(filter="creates_record", exact=True)
        )

        self.assertEqual(len(substring["matched_test_names"]), 2)
        self.assertEqual(exact["matched_test_names"], ["api::creates_record"])
        self.assertEqual(exact_zero["decision"], "empty")

    def test_ignored_selection_uses_only_discovered_ignored_tests(self):
        snapshot = discovery_snapshot(
            test_names=[
                {"name": "api::creates_record", "ignored": False},
                {"name": "ignored::migration_fixture", "ignored": True},
            ]
        )

        ignored = self.evaluate(
            snapshot=snapshot,
            request=discovery_request(filter="migration", ignored=True),
        )
        non_ignored = self.evaluate(
            snapshot=snapshot,
            request=discovery_request(filter="migration", ignored=False),
        )

        self.assertEqual(
            ignored["matched_test_names"], ["ignored::migration_fixture"]
        )
        self.assertEqual(non_ignored["decision"], "empty")

    def test_stale_source_fingerprint_cannot_prove_emptiness(self):
        result = self.evaluate(
            snapshot=discovery_snapshot(source_fingerprint="source-state-old"),
            request=discovery_request(filter="does_not_exist"),
        )

        self.assert_unknown(result, "source fingerprint")

    def test_scope_identity_mismatches_cannot_prove_emptiness(self):
        cases = {
            "package": {"package": "beta"},
            "target": {"target": "lib:alpha"},
            "features": {"features": ["serde"]},
            "harness": {"harness": "nextest"},
            "discovery": {"discovery_id": "other-listing"},
        }
        for reason, overrides in cases.items():
            with self.subTest(reason=reason):
                result = self.evaluate(
                    snapshot=discovery_snapshot(**overrides),
                    request=discovery_request(filter="does_not_exist"),
                )
                self.assert_unknown(result, reason)

    def test_unsupported_schema_incomplete_and_partial_snapshots_are_unknown(self):
        cases = [
            (discovery_snapshot(schema_version="2.0"), "schema"),
            (discovery_snapshot(complete=False), "incomplete"),
            ({"schema_version": "1.0", "complete": True}, "missing"),
        ]
        for snapshot, reason in cases:
            with self.subTest(reason=reason):
                result = self.evaluate(
                    snapshot=snapshot,
                    request=discovery_request(filter="does_not_exist"),
                )
                self.assert_unknown(result, reason)

    def test_malformed_or_ambiguous_snapshots_are_unknown(self):
        cases = [
            (discovery_snapshot(test_names="api::creates_record"), "test_names"),
            (discovery_snapshot(test_names=[""]), "test name"),
            (discovery_snapshot(package=["alpha", "beta"]), "package"),
            (discovery_snapshot(target=None), "target"),
            (discovery_snapshot(features=["serde", 7]), "features"),
        ]
        for snapshot, reason in cases:
            with self.subTest(reason=reason):
                result = self.evaluate(
                    snapshot=snapshot,
                    request=discovery_request(filter="does_not_exist"),
                )
                self.assert_unknown(result, reason)

    def test_snapshot_test_name_count_is_bounded(self):
        limit = getattr(self.resolver, "MAX_DISCOVERY_TEST_NAMES", None)
        self.assertIsInstance(limit, int)
        self.assertGreater(limit, 0)
        self.assertLessEqual(limit, 100_000)

        result = self.evaluate(
            snapshot=discovery_snapshot(
                test_names=[f"case_{index}" for index in range(limit + 1)]
            ),
            request=discovery_request(filter="does_not_exist"),
        )

        self.assert_unknown(result, "bounded")

    def test_canonical_cli_evaluates_environment_preflight(self):
        request = discovery_request(filter="does_not_exist")
        request["source_fingerprint"] = "stale-caller-value"
        environment = {
            "JCODE_TEST_PREFLIGHT_COMMAND": "cargo test -p alpha does_not_exist",
            "JCODE_TEST_PREFLIGHT_SOURCE_FINGERPRINT": "source-state-a",
            "JCODE_TEST_DISCOVERY_SNAPSHOT_JSON": json.dumps(discovery_snapshot()),
            "JCODE_TEST_DISCOVERY_REQUEST_JSON": json.dumps(request),
        }
        stdout = io.StringIO()
        with mock.patch.dict(os.environ, environment, clear=False), redirect_stdout(stdout):
            exit_code = self.resolver.main([])

        self.assertEqual(exit_code, 0)
        result = json.loads(stdout.getvalue())
        self.assertEqual(result["decision"], "empty")
        self.assertEqual(
            result["effective_scope"]["source_fingerprint"], "source-state-a"
        )

    def test_canonical_cli_rejects_incomplete_environment_preflight(self):
        environment = {
            "JCODE_TEST_PREFLIGHT_COMMAND": "cargo test -p alpha does_not_exist",
            "JCODE_TEST_PREFLIGHT_SOURCE_FINGERPRINT": "source-state-a",
        }
        stderr = io.StringIO()
        with mock.patch.dict(os.environ, environment, clear=True), redirect_stderr(stderr):
            self.assertEqual(self.resolver.main([]), 2)
        self.assertIn("requires JCODE_TEST_DISCOVERY_SNAPSHOT_JSON", stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
