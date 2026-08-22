#!/usr/bin/env python3
"""Contract tests for conservative Rust validation-scope resolution."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import unittest


SCRIPTS_DIR = Path(__file__).resolve().parent
RESOLVER_MODULE_PATH = SCRIPTS_DIR / "rust_validation_scope.py"


def load_resolver_module():
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


if __name__ == "__main__":
    unittest.main()
