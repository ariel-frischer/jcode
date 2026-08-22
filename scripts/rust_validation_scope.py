#!/usr/bin/env python3
"""Resolve conservative Cargo validation scopes from metadata and changed paths.

The resolver is intentionally deterministic and side-effect free when imported.  The
CLI obtains Cargo metadata only when a metadata document is not supplied explicitly.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path, PurePosixPath
import subprocess
import sys
from typing import Mapping, Sequence


SCHEMA_VERSION = "1.0"
DISCOVERY_SCHEMA_VERSION = "1.0"
MAX_DISCOVERY_TEST_NAMES = 100_000
DISCOVERY_PROVENANCE = "current complete test-discovery snapshot"
DEFAULT_PATH_RULES: dict[str, object] = {
    "required_features": {},
    "generated_prefixes": ["target/"],
    "build_script_names": ["build.rs"],
    "workspace_paths": ["Cargo.toml", "Cargo.lock", ".cargo/"],
    "guardrail_paths": [
        "scripts/check_guardrails.sh",
        "scripts/check_dependency_boundaries.py",
    ],
    "release_sensitive_paths": [
        "scripts/install.sh",
        "scripts/install.ps1",
        "scripts/install_release.sh",
        "scripts/check_release_version.sh",
    ],
}


def _string_list(value: object, field: str) -> list[str]:
    if value is None:
        return []
    if not isinstance(value, list) or any(
        not isinstance(item, str) or not item for item in value
    ):
        raise ValueError(f"{field} must be a list of non-empty strings")
    return list(dict.fromkeys(value))


def _optional_bool(value: object, field: str) -> bool | None:
    if value is None:
        return None
    if not isinstance(value, bool):
        raise ValueError(f"{field} must be a boolean or null")
    return value


def _normalise_path(path: str) -> str:
    if not isinstance(path, str) or not path:
        raise ValueError("affected paths must be non-empty strings")
    normalised = PurePosixPath(path.replace("\\", "/")).as_posix()
    while normalised.startswith("./"):
        normalised = normalised[2:]
    if normalised == ".." or normalised.startswith("../"):
        raise ValueError(f"affected path escapes the workspace: {path}")
    return normalised


def _matches_rule(path: str, rule: str) -> bool:
    normalised_rule = _normalise_path(rule)
    if rule.endswith("/"):
        return path.startswith(normalised_rule.rstrip("/") + "/")
    return path == normalised_rule


def _relative_to_workspace(path: str, workspace_root: str) -> str | None:
    candidate = Path(path)
    root = Path(workspace_root)
    try:
        return candidate.relative_to(root).as_posix()
    except ValueError:
        return None


def _package_index(
    metadata: Mapping[str, object],
) -> tuple[str, dict[str, dict[str, object]]]:
    workspace_root = metadata.get("workspace_root")
    packages = metadata.get("packages")
    members = metadata.get("workspace_members")
    if not isinstance(workspace_root, str) or not workspace_root:
        raise ValueError("Cargo metadata must contain a non-empty workspace_root")
    if not isinstance(packages, list) or not isinstance(members, list):
        raise ValueError(
            "Cargo metadata must contain packages and workspace_members lists"
        )

    member_ids = {member for member in members if isinstance(member, str)}
    by_name: dict[str, dict[str, object]] = {}
    for raw_package in packages:
        if not isinstance(raw_package, dict) or raw_package.get("id") not in member_ids:
            continue
        name = raw_package.get("name")
        manifest_path = raw_package.get("manifest_path")
        targets = raw_package.get("targets")
        features = raw_package.get("features")
        if (
            not isinstance(name, str)
            or not name
            or not isinstance(manifest_path, str)
            or not isinstance(targets, list)
            or not isinstance(features, dict)
        ):
            raise ValueError("workspace package metadata is incomplete")
        if name in by_name:
            raise ValueError(f"Cargo metadata contains duplicate package name: {name}")
        package_root = _relative_to_workspace(
            str(Path(manifest_path).parent), workspace_root
        )
        if package_root is None:
            raise ValueError(f"package {name} is outside the workspace root")
        by_name[name] = {
            "name": name,
            "root": package_root,
            "features": features,
            "targets": targets,
        }
    if not by_name:
        raise ValueError("Cargo metadata contains no workspace packages")
    return workspace_root, by_name


def _path_package(
    path: str, packages: Mapping[str, Mapping[str, object]]
) -> dict[str, object] | None:
    matches: list[dict[str, object]] = []
    for package in packages.values():
        root = str(package["root"])
        if not root or path == root or path.startswith(root.rstrip("/") + "/"):
            matches.append(dict(package))
    if not matches:
        return None
    matches.sort(key=lambda package: len(str(package["root"])), reverse=True)
    if len(matches) > 1 and len(str(matches[0]["root"])) == len(
        str(matches[1]["root"])
    ):
        return None
    return matches[0]


def _target_label(target: Mapping[str, object]) -> str | None:
    name = target.get("name")
    kinds = target.get("kind")
    if not isinstance(name, str) or not isinstance(kinds, list):
        return None
    supported = ("lib", "bin", "test", "example", "bench")
    kind = next((candidate for candidate in supported if candidate in kinds), None)
    return f"{kind}:{name}" if kind else None


def _package_targets(
    package: Mapping[str, object], workspace_root: str
) -> dict[str, str]:
    labels: dict[str, str] = {}
    targets = package["targets"]
    assert isinstance(targets, list)
    for raw_target in targets:
        if not isinstance(raw_target, dict):
            continue
        label = _target_label(raw_target)
        src_path = raw_target.get("src_path")
        if label is None or not isinstance(src_path, str):
            continue
        relative = _relative_to_workspace(src_path, workspace_root)
        if relative is not None:
            labels[label] = relative
    return labels


def _infer_target(
    path: str, package: Mapping[str, object], workspace_root: str
) -> str | None:
    targets = _package_targets(package, workspace_root)
    exact = [label for label, src_path in targets.items() if path == src_path]
    if len(exact) == 1:
        return exact[0]
    if len(exact) > 1:
        return None

    package_root = str(package["root"])
    src_prefix = f"{package_root}/src/" if package_root else "src/"
    if path.startswith(src_prefix):
        lib_targets = sorted(label for label in targets if label.startswith("lib:"))
        if len(lib_targets) == 1:
            return lib_targets[0]
    return None


def _classify_forced_fallback(path: str, rules: Mapping[str, object]) -> str | None:
    classifications = (
        ("generated_prefixes", "generated path"),
        ("workspace_paths", "workspace path"),
        ("guardrail_paths", "guardrail path"),
        ("release_sensitive_paths", "release-sensitive path"),
    )
    for field, reason in classifications:
        for rule in _string_list(rules.get(field), field):
            if _matches_rule(path, rule):
                return reason
    build_names = _string_list(rules.get("build_script_names"), "build_script_names")
    if PurePosixPath(path).name in build_names:
        return "build script path"
    return None


def _required_features(
    path: str, package_name: str, rules: Mapping[str, object]
) -> set[str]:
    raw_rules = rules.get("required_features", {})
    if not isinstance(raw_rules, dict):
        raise ValueError("required_features must be an object")
    required: set[str] = set()
    for raw_prefix, raw_packages in raw_rules.items():
        if not isinstance(raw_prefix, str) or not isinstance(raw_packages, dict):
            raise ValueError(
                "required_features entries must map paths to package objects"
            )
        if not _matches_rule(path, raw_prefix):
            continue
        package_features = raw_packages.get(package_name, [])
        required.update(
            _string_list(
                package_features, f"required_features[{raw_prefix}][{package_name}]"
            )
        )
    return required


def _looks_optional_but_unproven(
    path: str, package: Mapping[str, object], required: set[str]
) -> bool:
    if required:
        return False
    features = package["features"]
    assert isinstance(features, dict)
    components = set(PurePosixPath(path).parts)
    return any(feature != "default" and feature in components for feature in features)


def _explicit_values(explicit_inputs: Mapping[str, object]) -> dict[str, object]:
    unknown = set(explicit_inputs) - {
        "packages",
        "targets",
        "features",
        "no_default_features",
        "all_features",
    }
    if unknown:
        raise ValueError(
            f"unsupported explicit input fields: {', '.join(sorted(unknown))}"
        )
    return {
        "packages": _string_list(explicit_inputs.get("packages"), "packages"),
        "targets": _string_list(explicit_inputs.get("targets"), "targets"),
        "features": _string_list(explicit_inputs.get("features"), "features"),
        "no_default_features": _optional_bool(
            explicit_inputs.get("no_default_features"), "no_default_features"
        ),
        "all_features": _optional_bool(
            explicit_inputs.get("all_features"), "all_features"
        ),
    }


def _default_scope(defaults: Mapping[str, object]) -> dict[str, object]:
    mode = defaults.get("mode", "broad")
    if mode not in {"broad", "focused"}:
        raise ValueError("default mode must be broad or focused")
    packages = _string_list(defaults.get("packages"), "default packages")
    targets = _string_list(defaults.get("targets"), "default targets")
    features = _string_list(defaults.get("features"), "default features")
    no_default = _optional_bool(
        defaults.get("no_default_features"), "default no_default_features"
    )
    all_features = _optional_bool(defaults.get("all_features"), "default all_features")
    if all_features and features:
        raise ValueError("default all_features conflicts with default features")
    return {
        "mode": mode,
        "packages": packages,
        "targets": targets,
        "features": [] if all_features else features,
        "no_default_features": bool(no_default),
        "all_features": bool(all_features),
    }


def _broad_scope(
    defaults: Mapping[str, object], explicit: Mapping[str, object] | None = None
) -> dict[str, object]:
    scope = _default_scope(defaults)
    scope["mode"] = "broad"
    scope["packages"] = []
    scope["targets"] = []
    if explicit:
        explicit_features = explicit.get("features", [])
        explicit_all_features = explicit.get("all_features")
        explicit_no_default = explicit.get("no_default_features")
        if explicit_all_features is not None:
            scope["all_features"] = bool(explicit_all_features)
        if explicit_no_default is not None:
            scope["no_default_features"] = bool(explicit_no_default)
        if explicit_features:
            scope["features"] = _string_list(explicit_features, "features")
        if scope["all_features"]:
            scope["features"] = []
    return scope


def _result(
    *,
    explicit_inputs: Mapping[str, object],
    defaults: Mapping[str, object],
    affected_paths: list[str],
    effective_scope: Mapping[str, object],
    resolution_source: str,
    fallback_reason: str | None,
) -> dict[str, object]:
    return {
        "schema_version": SCHEMA_VERSION,
        "configured_inputs": {
            "explicit": dict(explicit_inputs),
            "defaults": dict(defaults),
        },
        "explicit_inputs": dict(explicit_inputs),
        "affected_paths": affected_paths,
        "effective_scope": dict(effective_scope),
        "resolution_source": resolution_source,
        "fallback_reason": fallback_reason,
    }


def _discovery_unknown(reason: str) -> dict[str, object]:
    return {
        "schema_version": DISCOVERY_SCHEMA_VERSION,
        "decision": "unknown",
        "reason": reason,
        "proof_provenance": None,
        "matched_test_names": [],
        "effective_scope": None,
    }


def _discovery_identity(
    value: Mapping[str, object], label: str
) -> tuple[dict[str, object] | None, str | None]:
    identity: dict[str, object] = {}
    for field in (
        "source_fingerprint",
        "package",
        "target",
        "harness",
        "discovery_id",
    ):
        item = value.get(field)
        if not isinstance(item, str) or not item:
            return None, f"{label} {field} must be a non-empty string"
        identity[field] = item

    features = value.get("features")
    if not isinstance(features, list) or any(
        not isinstance(feature, str) or not feature for feature in features
    ):
        return None, f"{label} features must be a list of non-empty strings"
    if len(features) != len(set(features)):
        return None, f"{label} features are ambiguous because they contain duplicates"
    identity["features"] = sorted(features)
    return identity, None


def _discovered_tests(
    value: object,
) -> tuple[list[tuple[str, bool]] | None, str | None]:
    if not isinstance(value, list):
        return None, "snapshot test_names must be a list"
    if len(value) > MAX_DISCOVERY_TEST_NAMES:
        return None, "snapshot test_names exceeds the bounded discovery limit"

    tests: list[tuple[str, bool]] = []
    seen: dict[str, bool] = {}
    for item in value:
        if isinstance(item, str):
            name = item
            ignored = False
        elif isinstance(item, Mapping):
            name = item.get("name")
            ignored = item.get("ignored", False)
            if set(item) - {"name", "ignored"}:
                return None, "snapshot test name entry contains unsupported fields"
        else:
            return None, "snapshot test name entries must be strings or objects"
        if not isinstance(name, str) or not name:
            return None, "snapshot test name must be a non-empty string"
        if not isinstance(ignored, bool):
            return None, "snapshot test name ignored flag must be a boolean"
        if name in seen:
            return None, f"snapshot test name is ambiguous because it is duplicated: {name}"
        seen[name] = ignored
        tests.append((name, ignored))
    return tests, None


def evaluate_test_discovery_snapshot(
    discovery_snapshot: Mapping[str, object],
    request: Mapping[str, object],
) -> dict[str, object]:
    """Evaluate a bounded discovery snapshot without rejecting uncertain requests."""
    if not isinstance(discovery_snapshot, Mapping):
        return _discovery_unknown("snapshot must be an object")
    if not isinstance(request, Mapping):
        return _discovery_unknown("request must be an object")
    if discovery_snapshot.get("schema_version") != DISCOVERY_SCHEMA_VERSION:
        return _discovery_unknown("snapshot schema version is unsupported")
    if discovery_snapshot.get("complete") is not True:
        return _discovery_unknown("snapshot is incomplete")

    snapshot_identity, error = _discovery_identity(discovery_snapshot, "snapshot")
    if error:
        return _discovery_unknown(f"snapshot identity is missing or malformed: {error}")
    request_identity, error = _discovery_identity(request, "request")
    if error:
        return _discovery_unknown(f"request identity is missing or malformed: {error}")
    assert snapshot_identity is not None and request_identity is not None

    identity_labels = {
        "source_fingerprint": "source fingerprint",
        "package": "package",
        "target": "target",
        "features": "features",
        "harness": "harness",
        "discovery_id": "discovery identity",
    }
    for field, reason in identity_labels.items():
        if snapshot_identity[field] != request_identity[field]:
            return _discovery_unknown(f"{reason} does not match the snapshot")

    tests, error = _discovered_tests(discovery_snapshot.get("test_names"))
    if error:
        return _discovery_unknown(error)
    assert tests is not None

    test_filter = request.get("filter", "")
    exact = request.get("exact", False)
    ignored = request.get("ignored", False)
    if not isinstance(test_filter, str):
        return _discovery_unknown("request filter must be a string")
    if not isinstance(exact, bool):
        return _discovery_unknown("request exact flag must be a boolean")
    if not isinstance(ignored, bool):
        return _discovery_unknown("request ignored flag must be a boolean")

    matched = [
        name
        for name, discovered_ignored in tests
        if discovered_ignored == ignored
        and (name == test_filter if exact else test_filter in name)
    ]
    effective_scope = {
        "source_fingerprint": request_identity["source_fingerprint"],
        "package": request_identity["package"],
        "target": request_identity["target"],
        "features": request_identity["features"],
        "harness": request_identity["harness"],
        "discovery_id": request_identity["discovery_id"],
    }
    return {
        "schema_version": DISCOVERY_SCHEMA_VERSION,
        "decision": "matches" if matched else "empty",
        "reason": "snapshot proves matching tests" if matched else "snapshot proves zero matching tests",
        "proof_provenance": DISCOVERY_PROVENANCE,
        "matched_test_names": matched,
        "effective_scope": effective_scope,
    }


def resolve_validation_scope(
    cargo_metadata: Mapping[str, object],
    affected_paths: Sequence[str],
    *,
    explicit_inputs: Mapping[str, object] | None = None,
    defaults: Mapping[str, object] | None = None,
    path_rules: Mapping[str, object] | None = None,
) -> dict[str, object]:
    """Return the narrowest provably correct Cargo scope or a broad fallback."""
    if isinstance(affected_paths, (str, bytes)) or not isinstance(
        affected_paths, Sequence
    ):
        raise ValueError(
            "affected_paths must be a sequence of repository-relative paths"
        )
    if explicit_inputs is not None and not isinstance(explicit_inputs, Mapping):
        raise ValueError("explicit_inputs must be an object or null")
    if defaults is not None and not isinstance(defaults, Mapping):
        raise ValueError("defaults must be an object or null")
    if path_rules is not None and not isinstance(path_rules, Mapping):
        raise ValueError("path_rules must be an object or null")

    explicit_original = dict(explicit_inputs or {})
    defaults_original = dict(defaults or {})
    rules = dict(DEFAULT_PATH_RULES if path_rules is None else path_rules)
    paths = list(dict.fromkeys(_normalise_path(path) for path in affected_paths))
    explicit = _explicit_values(explicit_original)
    workspace_root, packages = _package_index(cargo_metadata)

    explicit_active = any(
        explicit[field] for field in ("packages", "targets", "features")
    ) or any(
        explicit[field] is not None for field in ("no_default_features", "all_features")
    )

    if explicit["all_features"] and explicit["features"]:
        raise ValueError("all_features conflicts with explicit features")

    if not paths and not explicit_active:
        return _result(
            explicit_inputs=explicit_original,
            defaults=defaults_original,
            affected_paths=paths,
            effective_scope=_default_scope(defaults_original),
            resolution_source="default",
            fallback_reason=None,
        )

    if not paths and not explicit["packages"] and not explicit["targets"]:
        return _result(
            explicit_inputs=explicit_original,
            defaults=defaults_original,
            affected_paths=paths,
            effective_scope=_broad_scope(defaults_original, explicit),
            resolution_source="explicit",
            fallback_reason=None,
        )

    for path in paths:
        fallback = _classify_forced_fallback(path, rules)
        if fallback:
            return _result(
                explicit_inputs=explicit_original,
                defaults=defaults_original,
                affected_paths=paths,
                effective_scope=_broad_scope(defaults_original, explicit),
                resolution_source="conservative_fallback",
                fallback_reason=fallback,
            )

    path_packages: list[dict[str, object]] = []
    targets: set[str] = set()
    required_features: set[str] = set()
    for path in paths:
        package = _path_package(path, packages)
        if package is None:
            return _result(
                explicit_inputs=explicit_original,
                defaults=defaults_original,
                affected_paths=paths,
                effective_scope=_broad_scope(defaults_original, explicit),
                resolution_source="conservative_fallback",
                fallback_reason=f"unknown path ownership: {path}",
            )
        path_packages.append(package)
        required = _required_features(path, str(package["name"]), rules)
        if _looks_optional_but_unproven(path, package, required):
            return _result(
                explicit_inputs=explicit_original,
                defaults=defaults_original,
                affected_paths=paths,
                effective_scope=_broad_scope(defaults_original, explicit),
                resolution_source="conservative_fallback",
                fallback_reason=f"optional feature requirement is unprovable for {path}",
            )
        required_features.update(required)
        target = _infer_target(path, package, workspace_root)
        if target is None:
            return _result(
                explicit_inputs=explicit_original,
                defaults=defaults_original,
                affected_paths=paths,
                effective_scope=_broad_scope(defaults_original, explicit),
                resolution_source="conservative_fallback",
                fallback_reason=f"ambiguous target ownership: {path}",
            )
        targets.add(target)

    inferred_packages = sorted({str(package["name"]) for package in path_packages})
    if len(inferred_packages) > 1:
        return _result(
            explicit_inputs=explicit_original,
            defaults=defaults_original,
            affected_paths=paths,
            effective_scope=_broad_scope(defaults_original),
            resolution_source="conservative_fallback",
            fallback_reason="cross-package affected paths require broad validation",
        )

    selected_packages = list(explicit["packages"] or inferred_packages)
    for package_name in selected_packages:
        if package_name not in packages:
            raise ValueError(f"unknown explicit package: {package_name}")
    if (
        explicit["packages"]
        and inferred_packages
        and selected_packages != inferred_packages
    ):
        raise ValueError(
            "explicit package conflicts with the package owning the affected paths"
        )

    all_targets: dict[str, set[str]] = {}
    for package_name, package in packages.items():
        for label in _package_targets(package, workspace_root):
            all_targets.setdefault(label, set()).add(package_name)
    selected_targets = list(explicit["targets"] or sorted(targets))
    for target in selected_targets:
        owners = all_targets.get(target)
        if not owners:
            raise ValueError(f"unknown explicit target: {target}")
        compatible_owners = owners.intersection(selected_packages)
        if selected_packages and not compatible_owners:
            raise ValueError(
                f"target {target} conflicts with selected package {selected_packages}"
            )
        if len(compatible_owners or owners) > 1:
            raise ValueError(f"target {target} is ambiguous across packages")
    if explicit["targets"] and inferred_packages:
        target_owners = {
            next(iter(all_targets[target].intersection(selected_packages)))
            for target in selected_targets
        }
        if target_owners != set(inferred_packages):
            raise ValueError("explicit target conflicts with affected path ownership")

    selected_features = list(explicit["features"] or sorted(required_features))
    all_features = bool(explicit["all_features"])
    no_default_features = bool(explicit["no_default_features"])
    known_features: set[str] = set()
    default_features: set[str] = set()
    for package_name in selected_packages:
        package_features = packages[package_name]["features"]
        assert isinstance(package_features, dict)
        known_features.update(
            str(feature) for feature in package_features if feature != "default"
        )
        raw_defaults = package_features.get("default", [])
        if isinstance(raw_defaults, list):
            default_features.update(
                feature for feature in raw_defaults if isinstance(feature, str)
            )
    unknown_features = sorted(set(selected_features) - known_features)
    if unknown_features:
        raise ValueError(f"unknown explicit features: {', '.join(unknown_features)}")
    satisfied_features = set(selected_features)
    if not no_default_features:
        satisfied_features.update(default_features)
    missing_required = sorted(required_features - satisfied_features)
    if missing_required and not all_features:
        raise ValueError(
            "required feature selection is insufficient; missing: "
            + ", ".join(missing_required)
        )

    effective_scope = {
        "mode": "focused",
        "packages": selected_packages,
        "targets": selected_targets,
        "features": [] if all_features else selected_features,
        "no_default_features": no_default_features,
        "all_features": all_features,
    }
    return _result(
        explicit_inputs=explicit_original,
        defaults=defaults_original,
        affected_paths=paths,
        effective_scope=effective_scope,
        resolution_source="explicit" if explicit_active else "inferred",
        fallback_reason=None,
    )


def _load_json(path: str, label: str) -> object:
    try:
        return json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"unable to load {label} JSON from {path}: {error}") from error


def _cargo_metadata(workspace_root: str) -> dict[str, object]:
    completed = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        cwd=workspace_root,
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or f"exit status {completed.returncode}"
        raise ValueError(f"cargo metadata failed: {detail}")
    try:
        metadata = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise ValueError(f"cargo metadata returned invalid JSON: {error}") from error
    if not isinstance(metadata, dict):
        raise ValueError("cargo metadata must return a JSON object")
    return metadata


def _mapping_json(value: str | None, label: str) -> dict[str, object]:
    if value is None:
        return {}
    try:
        parsed = json.loads(value)
    except json.JSONDecodeError as error:
        raise ValueError(f"invalid {label} JSON: {error}") from error
    if not isinstance(parsed, dict):
        raise ValueError(f"{label} JSON must be an object")
    return parsed


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="*", help="repository-relative affected paths")
    parser.add_argument("--workspace-root", default=".")
    parser.add_argument("--metadata", help="Cargo metadata JSON file")
    parser.add_argument("--path-rules", help="repository path-rule JSON file")
    parser.add_argument("--explicit-json", help="explicit Cargo selections as JSON")
    parser.add_argument("--defaults-json", help="default Cargo selections as JSON")
    args = parser.parse_args(argv)

    try:
        raw_metadata = (
            _load_json(args.metadata, "Cargo metadata")
            if args.metadata
            else _cargo_metadata(args.workspace_root)
        )
        if not isinstance(raw_metadata, dict):
            raise ValueError("Cargo metadata JSON must be an object")
        raw_rules = (
            _load_json(args.path_rules, "path rules")
            if args.path_rules
            else DEFAULT_PATH_RULES
        )
        if not isinstance(raw_rules, dict):
            raise ValueError("path-rule JSON must be an object")
        result = resolve_validation_scope(
            raw_metadata,
            args.paths,
            explicit_inputs=_mapping_json(args.explicit_json, "explicit inputs"),
            defaults=_mapping_json(args.defaults_json, "defaults"),
            path_rules=raw_rules,
        )
    except ValueError as error:
        print(f"rust-validation-scope: {error}", file=sys.stderr)
        return 2

    json.dump(result, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
