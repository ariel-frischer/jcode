#!/usr/bin/env python3
"""Dependency-free deterministic primitives for the session-feedback skill."""

from __future__ import annotations

import hashlib
import json
import math
import os
import re
import shutil
import subprocess
import tempfile
import time
import unicodedata
from pathlib import Path
from typing import Any, Mapping, Sequence


SKILL_DIR = Path(__file__).resolve().parent
SCHEMA_DIR = SKILL_DIR / "schemas"
SUPPORTED_SCHEMAS = frozenset({"evidence-v1", "proposal-v1", "generator-response-v1"})
SUPPORTED_SCOPES = frozenset({"personal-global", "project-jcode"})
SUPPORTED_CATEGORIES = frozenset(
    {
        "global-instructions",
        "skills",
        "hooks-config",
        "model-profile-choices",
        "context-skill-routing",
        "harness-setup-contracts",
        "sdk-public-surfaces",
        "jcode",
    }
)
MAX_ERROR_LENGTH = 768
MAX_SESSION_ID_LENGTH = 128
MAX_PROPOSAL_LOCATIONS = 16
MAX_RENDERED_PROPOSAL_BYTES = 65_536
MAX_LIBRARIAN_SUMMARY_BYTES = 262_144
MAX_SHORTLIST_TARGETS = 16
DEFAULT_PER_EXCERPT_BYTES = 8_192
DEFAULT_TOTAL_EXCERPT_BYTES = 32_768
DEFAULT_GENERATION_CONFIG = {
    "model": "gpt-5.6-sol",
    "effort": "medium",
    "max_evidence_bytes": 262_144,
    "max_excerpt_bytes": 32_768,
    "max_input_tokens": 32_768,
    "max_output_tokens": 8_192,
    "max_proposals": 8,
    "max_elapsed_seconds": 120.0,
    "max_estimated_cost_usd": 1.0,
}
CONFIG_ENVIRONMENT_NAMES = {
    "model": "JCODE_SESSION_FEEDBACK_MODEL",
    "effort": "JCODE_SESSION_FEEDBACK_EFFORT",
    "max_evidence_bytes": "JCODE_SESSION_FEEDBACK_MAX_EVIDENCE_BYTES",
    "max_excerpt_bytes": "JCODE_SESSION_FEEDBACK_MAX_EXCERPT_BYTES",
    "max_input_tokens": "JCODE_SESSION_FEEDBACK_MAX_INPUT_TOKENS",
    "max_output_tokens": "JCODE_SESSION_FEEDBACK_MAX_OUTPUT_TOKENS",
    "max_proposals": "JCODE_SESSION_FEEDBACK_MAX_PROPOSALS",
    "max_elapsed_seconds": "JCODE_SESSION_FEEDBACK_MAX_ELAPSED_SECONDS",
    "max_estimated_cost_usd": "JCODE_SESSION_FEEDBACK_MAX_ESTIMATED_COST_USD",
}
INTEGER_CONFIG_FIELDS = frozenset(
    {
        "max_evidence_bytes",
        "max_excerpt_bytes",
        "max_input_tokens",
        "max_output_tokens",
        "max_proposals",
    }
)
FLOAT_CONFIG_FIELDS = frozenset({"max_elapsed_seconds", "max_estimated_cost_usd"})
SUPPORTED_EFFORTS = frozenset(
    {"none", "minimal", "low", "medium", "high", "xhigh", "max"}
)
SUPPORTED_MODELS = frozenset({"gpt-5.6-sol"})
MAX_FEEDBACK_CONFIG_BYTES = 65_536
MAX_OAUTH_CREDENTIAL_BYTES = 1_048_576
GENERATION_ENV_ALLOWLIST = (
    "PATH",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TZ",
    "TERM",
    "COLORTERM",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "REQUESTS_CA_BUNDLE",
    "CURL_CA_BUNDLE",
    "NO_PROXY",
    "no_proxy",
    "JCODE_NO_TELEMETRY",
)
LIBRARIAN_SUMMARY_VERSION = "session-summary.v1"
LIBRARIAN_SECTION_FIELDS = (
    "goal",
    "outcomes",
    "decisions",
    "unresolved_work",
    "risks",
    "next_steps",
)
EVIDENCE_ITEM_FIELDS = {
    "visible_outcome": frozenset(
        {"reference", "category", "summary", "relevant_path", "content_hash"}
    ),
    "todo_assessment": frozenset({"reference", "category", "summary", "status"}),
    "tool_invocation_receipt": frozenset(
        {
            "reference",
            "category",
            "name",
            "outcome",
            "summary",
            "relevant_path",
            "content_hash",
        }
    ),
    "skill_invocation_receipt": frozenset(
        {
            "reference",
            "category",
            "name",
            "outcome",
            "summary",
            "relevant_path",
            "content_hash",
        }
    ),
    "failure_excerpt": frozenset(
        {"reference", "category", "name", "excerpt", "relevant_path", "content_hash"}
    ),
    "validation_receipt": frozenset(
        {
            "reference",
            "category",
            "name",
            "outcome",
            "summary",
            "relevant_path",
            "content_hash",
        }
    ),
    "relevant_path": frozenset(
        {"reference", "category", "path", "content_hash", "summary"}
    ),
}


class ValidationError(ValueError):
    """Raised when deterministic input normalization or accounting fails."""


class ContractValidationError(ValidationError):
    """Raised when a document does not satisfy its versioned JSON contract."""


def _bounded_error(message: str) -> str:
    if len(message) <= MAX_ERROR_LENGTH:
        return message
    return f"{message[: MAX_ERROR_LENGTH - 3]}..."


def canonical_json(value: Any) -> str:
    """Return stable compact JSON without escaping non-ASCII text."""
    try:
        return json.dumps(
            value,
            ensure_ascii=False,
            sort_keys=True,
            separators=(",", ":"),
            allow_nan=False,
        )
    except (TypeError, ValueError) as error:
        raise ValidationError(
            _bounded_error(f"value is not canonical JSON: {error}")
        ) from error


def canonical_bytes(value: Any) -> bytes:
    """Return the exact UTF-8 bytes used for persistence and accounting."""
    return canonical_json(value).encode("utf-8")


def load_schema(name: str) -> dict[str, Any]:
    """Load one allowlisted schema relative to this copy of the skill."""
    if name not in SUPPORTED_SCHEMAS:
        supported = ", ".join(sorted(SUPPORTED_SCHEMAS))
        raise ValidationError(
            _bounded_error(
                f"unsupported schema {name!r}; supported schemas: {supported}"
            )
        )
    path = SCHEMA_DIR / f"{name}.schema.json"
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
    except OSError as error:
        raise ValidationError(
            _bounded_error(f"unable to load schema {name!r} from {path.name}: {error}")
        ) from error
    except json.JSONDecodeError as error:
        raise ValidationError(
            _bounded_error(
                f"schema {name!r} is invalid JSON at line {error.lineno}, column {error.colno}"
            )
        ) from error
    if not isinstance(document, dict):
        raise ValidationError(f"schema {name!r} must contain a JSON object")
    return document


def _pointer(path: Sequence[str | int]) -> str:
    if not path:
        return "/"
    return "/" + "/".join(
        str(part).replace("~", "~0").replace("/", "~1") for part in path
    )


def _fail(schema_name: str, path: Sequence[str | int], detail: str) -> None:
    raise ContractValidationError(
        _bounded_error(f"{schema_name} contract error at {_pointer(path)}: {detail}")
    )


def _resolve_ref(
    schema_name: str, root: Mapping[str, Any], reference: str
) -> Mapping[str, Any]:
    if not reference.startswith("#/"):
        _fail(schema_name, (), f"unsupported schema reference {reference!r}")
    current: Any = root
    for raw_part in reference[2:].split("/"):
        part = raw_part.replace("~1", "/").replace("~0", "~")
        if not isinstance(current, Mapping) or part not in current:
            _fail(schema_name, (), f"schema reference {reference!r} does not exist")
        current = current[part]
    if not isinstance(current, Mapping):
        _fail(schema_name, (), f"schema reference {reference!r} is not an object")
    return current


def _matches_type(value: Any, expected: str) -> bool:
    if expected == "object":
        return isinstance(value, dict)
    if expected == "array":
        return isinstance(value, list)
    if expected == "string":
        return isinstance(value, str)
    if expected == "integer":
        return isinstance(value, int) and not isinstance(value, bool)
    if expected == "number":
        return isinstance(value, (int, float)) and not isinstance(value, bool)
    if expected == "boolean":
        return isinstance(value, bool)
    if expected == "null":
        return value is None
    return False


def _validate_node(
    schema_name: str,
    root: Mapping[str, Any],
    schema: Mapping[str, Any],
    value: Any,
    path: tuple[str | int, ...],
) -> None:
    if "$ref" in schema:
        target = _resolve_ref(schema_name, root, schema["$ref"])
        _validate_node(schema_name, root, target, value, path)
        return

    if "oneOf" in schema:
        errors: list[str] = []
        matches = 0
        for candidate in schema["oneOf"]:
            try:
                _validate_node(schema_name, root, candidate, value, path)
                matches += 1
            except ContractValidationError as error:
                errors.append(str(error))
        if matches != 1:
            detail = "value must match exactly one allowed shape"
            if errors:
                detail += f"; first mismatch: {errors[0].split(': ', 1)[-1]}"
            _fail(schema_name, path, detail)
        return

    if "const" in schema and value != schema["const"]:
        expected = schema["const"]
        detail = f"unsupported value {value!r}; supported value is {expected!r}"
        _fail(schema_name, path, detail)

    if "enum" in schema and value not in schema["enum"]:
        _fail(
            schema_name,
            path,
            f"unsupported value {value!r}; supported values: {schema['enum']}",
        )

    expected_type = schema.get("type")
    if expected_type is not None:
        allowed_types = (
            [expected_type] if isinstance(expected_type, str) else expected_type
        )
        if not any(_matches_type(value, item) for item in allowed_types):
            _fail(
                schema_name,
                path,
                f"expected {expected_type}, got {type(value).__name__}",
            )

    if isinstance(value, dict):
        properties = schema.get("properties", {})
        for required in schema.get("required", []):
            if required not in value:
                _fail(schema_name, path + (required,), "required field is missing")
        if schema.get("additionalProperties") is False:
            for key in value:
                if key not in properties:
                    _fail(schema_name, path + (key,), "unknown field is not allowed")
        if "maxProperties" in schema and len(value) > schema["maxProperties"]:
            _fail(
                schema_name,
                path,
                f"contains {len(value)} fields; maximum is {schema['maxProperties']}",
            )
        for key, child in value.items():
            child_schema = properties.get(key)
            if child_schema is not None:
                _validate_node(schema_name, root, child_schema, child, path + (key,))

    if isinstance(value, list):
        if "minItems" in schema and len(value) < schema["minItems"]:
            _fail(
                schema_name,
                path,
                f"contains {len(value)} items; minimum is {schema['minItems']}",
            )
        if "maxItems" in schema and len(value) > schema["maxItems"]:
            _fail(
                schema_name,
                path,
                f"contains {len(value)} items; maximum is {schema['maxItems']}",
            )
        if schema.get("uniqueItems"):
            seen: set[str] = set()
            for index, item in enumerate(value):
                marker = canonical_json(item)
                if marker in seen:
                    _fail(schema_name, path + (index,), "duplicate item is not allowed")
                seen.add(marker)
        item_schema = schema.get("items")
        if item_schema is not None:
            for index, item in enumerate(value):
                _validate_node(schema_name, root, item_schema, item, path + (index,))

    if isinstance(value, str):
        if "minLength" in schema and len(value) < schema["minLength"]:
            _fail(
                schema_name,
                path,
                f"length {len(value)} is below minimum {schema['minLength']}",
            )
        if "maxLength" in schema and len(value) > schema["maxLength"]:
            _fail(
                schema_name,
                path,
                f"length {len(value)} exceeds maximum {schema['maxLength']}",
            )
        pattern = schema.get("pattern")
        if pattern is not None and re.search(pattern, value) is None:
            _fail(schema_name, path, "value does not match the required format")

    if isinstance(value, (int, float)) and not isinstance(value, bool):
        if "minimum" in schema and value < schema["minimum"]:
            _fail(
                schema_name, path, f"value {value} is below minimum {schema['minimum']}"
            )
        if "maximum" in schema and value > schema["maximum"]:
            _fail(
                schema_name, path, f"value {value} exceeds maximum {schema['maximum']}"
            )


def validate_contract(schema_name: str, document: Any) -> Any:
    """Validate a decoded document against an allowlisted versioned contract."""
    schema = load_schema(schema_name)
    _validate_node(schema_name, schema, schema, document, ())
    if schema_name == "evidence-v1":
        expected = evidence_accounting(document["items"])
        if document["accounting"] != expected:
            _fail(
                schema_name,
                ("accounting",),
                "measurements do not reconcile with the canonical evidence items; "
                f"expected {canonical_json(expected)}",
            )
    return document


def parse_contract(schema_name: str, payload: str | bytes) -> Any:
    """Decode and validate one bounded contract payload."""
    if not isinstance(payload, (str, bytes)):
        raise ContractValidationError(
            f"{schema_name} contract error at /: payload must be str or bytes"
        )
    try:
        document = json.loads(payload)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        detail = getattr(error, "msg", str(error))
        raise ContractValidationError(
            _bounded_error(f"{schema_name} contract error at /: invalid JSON: {detail}")
        ) from error
    return validate_contract(schema_name, document)


def _normalize_identity(field: str, value: str, allowed: frozenset[str]) -> str:
    if not isinstance(value, str):
        raise ValidationError(f"{field} must be a string, got {type(value).__name__}")
    normalized = unicodedata.normalize("NFKC", value).strip().lower()
    normalized = re.sub(r"[^a-z0-9]+", "-", normalized).strip("-")
    if normalized not in allowed:
        supported = ", ".join(sorted(allowed))
        raise ValidationError(
            _bounded_error(
                f"invalid {field} value {value!r}; supported values: {supported}"
            )
        )
    return normalized


def normalize_scope(value: str) -> str:
    return _normalize_identity("scope", value, SUPPORTED_SCOPES)


def normalize_category(value: str) -> str:
    return _normalize_identity("category", value, SUPPORTED_CATEGORIES)


def normalize_concrete_target(value: str) -> str:
    if not isinstance(value, str):
        raise ValidationError(
            f"concrete_target must be a string, got {type(value).__name__}"
        )
    normalized = unicodedata.normalize("NFKC", value).strip().replace("\\", "/")
    normalized = re.sub(r"/{2,}", "/", normalized)
    if not normalized:
        raise ValidationError("concrete_target must not be empty")
    if len(normalized) > 512:
        raise ValidationError("concrete_target exceeds the 512 character limit")
    if any(ord(character) < 32 or ord(character) == 127 for character in normalized):
        raise ValidationError("concrete_target contains a control character")
    return normalized


def normalize_text(field: str, value: str) -> str:
    if not isinstance(value, str):
        raise ValidationError(f"{field} must be a string, got {type(value).__name__}")
    normalized = " ".join(unicodedata.normalize("NFKC", value).split()).lower()
    if not normalized:
        raise ValidationError(f"{field} must not be empty")
    return normalized


def normalize_evidence_reference(value: str) -> str:
    if not isinstance(value, str):
        raise ValidationError(
            f"evidence reference must be a string, got {type(value).__name__}"
        )
    normalized = unicodedata.normalize("NFKC", value).strip().lower()
    normalized = re.sub(r"[^a-z0-9]+", "-", normalized).strip("-")
    if not normalized or len(normalized) > 96:
        raise ValidationError(f"invalid evidence reference {value!r}")
    return normalized


def normalize_evidence_references(values: Sequence[str]) -> list[str]:
    if isinstance(values, (str, bytes)):
        raise ValidationError("evidence_references must be a sequence of strings")
    return sorted({normalize_evidence_reference(value) for value in values})


def fingerprint_material(material: Mapping[str, Any]) -> dict[str, str]:
    """Return the canonical target/problem/outcome identity used for deduplication."""
    required = ("scope", "category", "concrete_target", "problem", "intended_outcome")
    missing = [field for field in required if field not in material]
    if missing:
        raise ValidationError(
            f"fingerprint material is missing required fields: {', '.join(missing)}"
        )
    return {
        "category": normalize_category(material["category"]),
        "concrete_target": normalize_concrete_target(material["concrete_target"]),
        "intended_outcome": normalize_text(
            "intended_outcome", material["intended_outcome"]
        ),
        "problem": normalize_text("problem", material["problem"]),
        "scope": normalize_scope(material["scope"]),
    }


def proposal_fingerprint(material: Mapping[str, Any]) -> str:
    return hashlib.sha256(canonical_bytes(fingerprint_material(material))).hexdigest()


def measure_text(label: str, text: str) -> dict[str, int | str]:
    """Measure exact UTF-8 bytes and a deterministic, explicitly estimated token count."""
    if not isinstance(label, str) or not label.strip():
        raise ValidationError("measurement label must be a non-empty string")
    if not isinstance(text, str):
        raise ValidationError(
            f"measurement text must be a string, got {type(text).__name__}"
        )
    byte_count = len(text.encode("utf-8"))
    return {
        "label": label,
        "bytes": byte_count,
        "estimated_tokens": math.ceil(byte_count / 4),
    }


def measure_json(label: str, value: Any) -> dict[str, int | str]:
    """Measure the exact canonical JSON representation of a value."""
    return measure_text(label, canonical_json(value))


def evidence_accounting(items: Sequence[Mapping[str, Any]]) -> dict[str, Any]:
    """Build reconciled per-category and aggregate measurements for evidence items."""
    if isinstance(items, (str, bytes)):
        raise ValidationError("evidence items must be a sequence of objects")
    category_bytes: dict[str, int] = {}
    category_counts: dict[str, int] = {}
    for index, item in enumerate(items):
        if not isinstance(item, Mapping):
            raise ValidationError(f"evidence item {index} must be an object")
        category = item.get("category")
        if not isinstance(category, str) or not category:
            raise ValidationError(f"evidence item {index} must have a category")
        category_counts[category] = category_counts.get(category, 0) + 1
        category_bytes[category] = category_bytes.get(category, 0) + len(
            canonical_bytes(item)
        )

    by_category: dict[str, dict[str, int]] = {}
    for category in sorted(category_counts):
        byte_count = category_bytes[category]
        by_category[category] = {
            "item_count": category_counts[category],
            "serialized_bytes": byte_count,
            "estimated_tokens": math.ceil(byte_count / 4),
        }
    serialized_bytes = sum(entry["serialized_bytes"] for entry in by_category.values())
    estimated_tokens = sum(entry["estimated_tokens"] for entry in by_category.values())
    return {
        "serialized_bytes": serialized_bytes,
        "estimated_tokens": estimated_tokens,
        "by_category": by_category,
    }


def with_evidence_accounting(document: Mapping[str, Any]) -> dict[str, Any]:
    """Return a copy of an evidence document with canonical accounting populated."""
    if not isinstance(document, Mapping):
        raise ValidationError("evidence document must be an object")
    items = document.get("items")
    if not isinstance(items, list):
        raise ValidationError("evidence document items must be an array")
    result = dict(document)
    result["accounting"] = evidence_accounting(items)
    return result


def _target_identity(path: str) -> tuple[str, str, str]:
    target = normalize_concrete_target(path)
    lowered = target.lower()
    parts = tuple(part for part in lowered.split("/") if part)
    name = parts[-1] if parts else ""
    scope = (
        "personal-global" if lowered.startswith(("~/", "$home/")) else "project-jcode"
    )

    if name == "skill.md" or "skills" in parts:
        category = "skills"
    elif name in {"agents.md", "claude.md"} or "instructions" in name:
        category = "global-instructions"
    elif "hook" in lowered or "config" in name:
        category = "hooks-config"
    elif "model" in lowered or "profile" in lowered:
        category = "model-profile-choices"
    elif "context" in lowered or "routing" in lowered:
        category = "context-skill-routing"
    elif "harness" in lowered or "fixture" in lowered or "schema" in lowered:
        category = "harness-setup-contracts"
    elif "sdk" in parts:
        category = "sdk-public-surfaces"
    else:
        category = "jcode"
    return normalize_scope(scope), normalize_category(category), target


def shortlist_targets(
    evidence: Mapping[str, Any], *, limit: int = MAX_SHORTLIST_TARGETS
) -> list[dict[str, Any]]:
    """Build a deterministic target list using evidence metadata only."""
    if not isinstance(limit, int) or isinstance(limit, bool) or limit <= 0:
        raise ValidationError("shortlist limit must be a positive integer")
    if not isinstance(evidence, Mapping):
        raise ValidationError("evidence must be an object")
    items = evidence.get("items")
    if not isinstance(items, list):
        raise ValidationError("evidence items must be an array")

    candidates: dict[tuple[str, str, str], dict[str, set[str]]] = {}
    for index, item in enumerate(items):
        if not isinstance(item, Mapping):
            raise ValidationError(f"evidence item {index} must be an object")
        path = item.get("relevant_path", item.get("path"))
        if path is None:
            continue
        if not isinstance(path, str):
            raise ValidationError(f"evidence item {index} target path must be a string")
        identity = _target_identity(path)
        metadata = candidates.setdefault(
            identity,
            {
                "selection_evidence": set(),
                "metadata_hashes": set(),
                "invocation_outcomes": set(),
                "cited_hiccups": set(),
            },
        )
        reference = item.get("reference")
        if isinstance(reference, str):
            normalized_reference = normalize_evidence_reference(reference)
            metadata["selection_evidence"].add(normalized_reference)
        else:
            normalized_reference = None
        content_hash = item.get("content_hash")
        if isinstance(content_hash, str):
            metadata["metadata_hashes"].add(content_hash.lower())
        outcome = item.get("outcome")
        if isinstance(outcome, str):
            metadata["invocation_outcomes"].add(normalize_text("outcome", outcome))
        status = item.get("status")
        is_hiccup = (
            item.get("category") == "failure_excerpt"
            or (
                isinstance(outcome, str)
                and normalize_text("outcome", outcome)
                not in {"passed", "succeeded", "success"}
            )
            or (
                isinstance(status, str)
                and normalize_text("status", status) != "completed"
            )
        )
        if is_hiccup and normalized_reference is not None:
            metadata["cited_hiccups"].add(normalized_reference)

    result = [
        {
            "category": category,
            "scope": scope,
            "concrete_target": concrete_target,
            "selection_evidence": sorted(metadata["selection_evidence"]),
            "metadata_hashes": sorted(metadata["metadata_hashes"]),
            "invocation_outcomes": sorted(metadata["invocation_outcomes"]),
            "cited_hiccups": sorted(metadata["cited_hiccups"]),
        }
        for (scope, category, concrete_target), metadata in candidates.items()
    ]
    result.sort(
        key=lambda candidate: (
            candidate["concrete_target"],
            candidate["scope"],
            candidate["category"],
            candidate["metadata_hashes"],
            candidate["invocation_outcomes"],
            candidate["cited_hiccups"],
            candidate["selection_evidence"],
        )
    )
    return result[:limit]


def _positive_byte_limit(field: str, value: Any) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
        raise ValidationError(f"{field} must be a positive integer")
    return value


def _decode_bounded_utf8(content: bytes) -> str | None:
    try:
        return content.decode("utf-8")
    except UnicodeDecodeError as error:
        if error.end == len(content):
            return content[: error.start].decode("utf-8")
        return None


def excerpt_accounting(excerpts: Sequence[Mapping[str, Any]]) -> dict[str, int]:
    """Reconcile exact excerpt bytes and deterministic token estimates."""
    if isinstance(excerpts, (str, bytes)):
        raise ValidationError("excerpts must be a sequence of objects")
    byte_count = 0
    token_count = 0
    for index, excerpt in enumerate(excerpts):
        if not isinstance(excerpt, Mapping):
            raise ValidationError(f"excerpt {index} must be an object")
        measurement = excerpt.get("accounting")
        if not isinstance(measurement, Mapping):
            raise ValidationError(f"excerpt {index} is missing accounting")
        expected = measure_text("excerpt", excerpt.get("excerpt"))
        if dict(measurement) != expected:
            raise ValidationError(
                f"excerpt {index} accounting does not match its exact content"
            )
        byte_count += expected["bytes"]
        token_count += expected["estimated_tokens"]
    return {"serialized_bytes": byte_count, "estimated_tokens": token_count}


def load_shortlisted_excerpts(
    *,
    shortlist: Sequence[Mapping[str, Any]],
    target_root: str | Path,
    per_excerpt_byte_limit: int,
    total_excerpt_byte_limit: int,
    opener: Any = None,
) -> list[dict[str, Any]]:
    """Read only exact shortlisted skill or instruction targets within byte budgets."""
    if isinstance(shortlist, (str, bytes)):
        raise ValidationError("shortlist must be a sequence of objects")
    per_limit = _positive_byte_limit("per_excerpt_byte_limit", per_excerpt_byte_limit)
    total_limit = _positive_byte_limit(
        "total_excerpt_byte_limit", total_excerpt_byte_limit
    )
    root = Path(target_root).resolve()
    open_file = open if opener is None else opener
    excerpts: list[dict[str, Any]] = []
    used_bytes = 0

    for index, candidate in enumerate(shortlist):
        if not isinstance(candidate, Mapping):
            raise ValidationError(f"shortlist entry {index} must be an object")
        category = candidate.get("category")
        if category not in {"skills", "global-instructions"}:
            continue
        target = candidate.get("concrete_target")
        if not isinstance(target, str):
            raise ValidationError(
                f"shortlist entry {index} concrete_target must be a string"
            )
        normalized_target = normalize_concrete_target(target)
        if normalized_target.startswith(
            "~/"
        ) or normalized_target.casefold().startswith("$home/"):
            continue
        target_path = Path(normalized_target)
        if ".." in target_path.parts:
            continue
        resolved = (
            target_path.resolve()
            if target_path.is_absolute()
            else (root / target_path).resolve()
        )
        if resolved == root or root not in resolved.parents:
            continue
        remaining = total_limit - used_bytes
        if remaining <= 0:
            break
        read_limit = min(per_limit, remaining)
        try:
            with open_file(resolved, "rb") as handle:
                raw = handle.read(read_limit + 1)[:read_limit]
        except (OSError, PermissionError):
            continue
        excerpt_text = _decode_bounded_utf8(raw)
        if excerpt_text is None:
            continue
        accounting = measure_text("excerpt", excerpt_text)
        used_bytes += accounting["bytes"]
        excerpts.append(
            {
                "category": category,
                "scope": candidate.get("scope"),
                "concrete_target": normalized_target,
                "selection_evidence": list(candidate.get("selection_evidence", ())),
                "excerpt": excerpt_text,
                "accounting": accounting,
            }
        )
    return excerpts


def _validate_session_id(field: str, value: Any) -> str:
    if not isinstance(value, str):
        raise ValidationError(f"{field} must be a string, got {type(value).__name__}")
    if not value or value != value.strip():
        raise ValidationError(
            f"{field} must be a non-empty session id without surrounding whitespace"
        )
    if len(value) > MAX_SESSION_ID_LENGTH:
        raise ValidationError(
            f"{field} exceeds the {MAX_SESSION_ID_LENGTH} character limit"
        )
    if any(ord(character) < 32 or ord(character) == 127 for character in value):
        raise ValidationError(f"{field} contains a control character")
    return value


def _select_visible_session(
    requested_session_id: str | None,
    current_session_id: str | None,
    visible_session_ids: Sequence[str],
) -> str:
    if not isinstance(visible_session_ids, Sequence) or isinstance(
        visible_session_ids, (str, bytes)
    ):
        raise ValidationError("visible_session_ids must be a sequence of session ids")
    visible = {
        _validate_session_id(f"visible_session_ids[{index}]", session_id)
        for index, session_id in enumerate(visible_session_ids)
    }

    if requested_session_id is None:
        if current_session_id is None:
            raise ValidationError(
                "current session is unavailable; supply a visible session id explicitly"
            )
        selected = _validate_session_id("current_session_id", current_session_id)
    else:
        selected = _validate_session_id("requested_session_id", requested_session_id)

    if selected not in visible:
        raise ValidationError(
            _bounded_error(
                f"session id {selected!r} is not in the supplied visible session ids"
            )
        )
    return selected


def _allowlist_evidence_items(
    items: Sequence[Mapping[str, Any]], *, source_label: str
) -> list[dict[str, Any]]:
    if not isinstance(items, Sequence) or isinstance(items, (str, bytes)):
        raise ValidationError(
            f"{source_label} must be a sequence of allowlisted evidence objects"
        )
    if not items:
        raise ValidationError(f"{source_label} did not contain any permitted evidence")

    allowlisted: list[dict[str, Any]] = []
    for index, item in enumerate(items):
        if not isinstance(item, Mapping):
            raise ValidationError(f"{source_label}[{index}] must be an evidence object")
        category = item.get("category")
        allowed_fields = EVIDENCE_ITEM_FIELDS.get(category)
        if allowed_fields is None:
            raise ContractValidationError(
                _bounded_error(
                    f"{source_label}[{index}] has unsupported non-allowlisted "
                    f"evidence category {category!r}"
                )
            )
        allowlisted.append({key: item[key] for key in allowed_fields if key in item})
    return allowlisted


def _normalized_evidence(
    *, source: str, items: Sequence[Mapping[str, Any]], source_label: str
) -> dict[str, Any]:
    allowlisted_items = _allowlist_evidence_items(items, source_label=source_label)
    document = with_evidence_accounting(
        {
            "contract_version": "evidence-v1",
            "source": source,
            "items": allowlisted_items,
        }
    )
    try:
        return validate_contract("evidence-v1", document)
    except ContractValidationError as error:
        raise ContractValidationError(
            _bounded_error(
                f"{source_label} must contain only valid allowlisted evidence: {error}"
            )
        ) from error


def _fallback_evidence(visible_items: Sequence[Mapping[str, Any]]) -> dict[str, Any]:
    return _normalized_evidence(
        source="fallback",
        items=visible_items,
        source_label="fallback visible_items",
    )


def _read_librarian_summary(path_value: str | Path) -> Mapping[str, Any]:
    if not isinstance(path_value, (str, Path)):
        raise ValidationError(
            "librarian_summary_path must be an explicitly supplied path"
        )
    path = Path(path_value).expanduser()
    try:
        with path.open("rb") as summary_file:
            payload = summary_file.read(MAX_LIBRARIAN_SUMMARY_BYTES + 1)
    except OSError as error:
        raise ValidationError(
            _bounded_error(f"unable to read supplied librarian summary {path}: {error}")
        ) from error
    if len(payload) > MAX_LIBRARIAN_SUMMARY_BYTES:
        raise ValidationError(
            f"supplied librarian summary exceeds the {MAX_LIBRARIAN_SUMMARY_BYTES} byte limit"
        )
    try:
        document = json.loads(payload)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        detail = getattr(error, "msg", str(error))
        raise ValidationError(
            _bounded_error(f"supplied librarian summary is invalid JSON: {detail}")
        ) from error
    if not isinstance(document, Mapping):
        raise ValidationError("supplied librarian summary must contain a JSON object")
    return document


def _librarian_evidence(
    *, session_id: str, librarian_summary_path: str | Path
) -> dict[str, Any]:
    summary = _read_librarian_summary(librarian_summary_path)
    version = summary.get("format_version")
    if version != LIBRARIAN_SUMMARY_VERSION:
        raise ValidationError(
            _bounded_error(
                f"unsupported librarian summary version {version!r}; "
                f"supported value is {LIBRARIAN_SUMMARY_VERSION!r}"
            )
        )
    summary_session_id = _validate_session_id(
        "librarian summary session_id", summary.get("session_id")
    )
    if summary_session_id != session_id:
        raise ValidationError(
            _bounded_error(
                f"librarian summary session id {summary_session_id!r} does not match "
                f"selected session {session_id!r}"
            )
        )
    sections = summary.get("summary")
    if not isinstance(sections, Mapping) or set(sections) != set(
        LIBRARIAN_SECTION_FIELDS
    ):
        raise ValidationError(
            "librarian summary must contain exactly the supported structured sections"
        )

    items: list[dict[str, Any]] = []

    def append_text(
        reference: str,
        category: str,
        value: Any,
        *,
        status: str | None = None,
    ) -> None:
        if not isinstance(value, str):
            raise ValidationError(f"librarian summary {reference} must be a string")
        normalized = " ".join(unicodedata.normalize("NFKC", value).split())
        if not normalized:
            raise ValidationError(f"librarian summary {reference} must not be empty")
        item = {
            "reference": reference,
            "category": category,
            "summary": normalized,
        }
        if status is not None:
            item["status"] = status
        items.append(item)

    def append_list(
        field: str,
        reference_prefix: str,
        category: str,
        *,
        status: str | None = None,
    ) -> None:
        values = sections[field]
        if not isinstance(values, Sequence) or isinstance(values, (str, bytes)):
            raise ValidationError(f"librarian summary {field} must be an array")
        for index, value in enumerate(values, 1):
            append_text(f"{reference_prefix}-{index}", category, value, status=status)

    append_text("librarian-goal", "visible_outcome", sections["goal"])
    append_list("outcomes", "librarian-outcome", "visible_outcome")
    append_list("decisions", "librarian-decision", "visible_outcome")
    append_list(
        "unresolved_work",
        "librarian-unresolved",
        "todo_assessment",
        status="pending",
    )
    append_list("risks", "librarian-risk", "visible_outcome")
    append_list(
        "next_steps",
        "librarian-next-step",
        "todo_assessment",
        status="pending",
    )

    relevant_files = summary.get("relevant_files")
    if not isinstance(relevant_files, Sequence) or isinstance(
        relevant_files, (str, bytes)
    ):
        raise ValidationError("librarian summary relevant_files must be an array")
    for index, path in enumerate(relevant_files, 1):
        items.append(
            {
                "reference": f"librarian-path-{index}",
                "category": "relevant_path",
                "path": path,
            }
        )

    return _normalized_evidence(
        source="librarian",
        items=items,
        source_label="librarian structured summary",
    )


def acquire_evidence(
    *,
    session_id: str,
    librarian_summary_path: str | Path | None,
    visible_items: Sequence[Mapping[str, Any]],
) -> dict[str, Any]:
    """Adapt an explicitly supplied librarian summary or use visible fallback evidence."""
    selected_session_id = _validate_session_id("session_id", session_id)
    librarian_error: ValidationError | None = None
    if librarian_summary_path is not None:
        try:
            return _librarian_evidence(
                session_id=selected_session_id,
                librarian_summary_path=librarian_summary_path,
            )
        except ValidationError as error:
            librarian_error = error

    try:
        return _fallback_evidence(visible_items)
    except ValidationError as fallback_error:
        if librarian_error is None:
            raise
        raise ValidationError(
            _bounded_error(
                "librarian evidence was unusable and fallback evidence was unavailable: "
                f"librarian: {librarian_error}; fallback: {fallback_error}"
            )
        ) from fallback_error


def prepare_feedback_invocation(
    *,
    requested_session_id: str | None,
    current_session_id: str | None,
    visible_session_ids: Sequence[str],
    visible_items: Sequence[Mapping[str, Any]],
    librarian_summary_path: str | Path | None,
) -> dict[str, Any]:
    """Select a visible session and build its deterministic first-run evidence bundle.

    This preparation step reads only an explicitly supplied librarian summary path. It
    never scans for summaries, inspects the selected session, invokes a provider, or
    creates feedback state.
    """
    session_id = _select_visible_session(
        requested_session_id,
        current_session_id,
        visible_session_ids,
    )
    return {
        "session_id": session_id,
        "evidence": acquire_evidence(
            session_id=session_id,
            librarian_summary_path=librarian_summary_path,
            visible_items=visible_items,
        ),
    }


def _configured_value(value: Any) -> bool:
    return not (isinstance(value, str) and not value.strip())


def _parse_config_value(field: str, value: Any, source: str) -> Any:
    if field == "model":
        if not isinstance(value, str) or value.strip() not in SUPPORTED_MODELS:
            raise ValidationError(f"unsupported model from {source}")
        return value.strip()
    if field == "effort":
        if not isinstance(value, str) or value.strip() not in SUPPORTED_EFFORTS:
            raise ValidationError(f"unsupported effort from {source}")
        return value.strip()
    if field in INTEGER_CONFIG_FIELDS:
        parsed = value
        if isinstance(value, str):
            try:
                parsed = int(value.strip())
            except ValueError as error:
                raise ValidationError(
                    f"{field} from {source} must be a positive integer"
                ) from error
        if not isinstance(parsed, int) or isinstance(parsed, bool) or parsed <= 0:
            raise ValidationError(f"{field} from {source} must be a positive integer")
        return parsed
    if field in FLOAT_CONFIG_FIELDS:
        parsed = value
        if isinstance(value, str):
            try:
                parsed = float(value.strip())
            except ValueError as error:
                raise ValidationError(
                    f"{field} from {source} must be a positive finite number"
                ) from error
        if (
            not isinstance(parsed, (int, float))
            or isinstance(parsed, bool)
            or not math.isfinite(parsed)
            or parsed <= 0
        ):
            raise ValidationError(
                f"{field} from {source} must be a positive finite number"
            )
        return float(parsed)
    raise ValidationError(f"unsupported session-feedback configuration field: {field}")


def _load_persisted_feedback_config(config_path: Path) -> dict[str, Any]:
    try:
        if not config_path.exists():
            return {}
        if not config_path.is_file():
            raise ValidationError("persisted feedback configuration is not a file")
        if config_path.stat().st_size > MAX_FEEDBACK_CONFIG_BYTES:
            raise ValidationError("persisted feedback configuration is oversized")
        parsed = json.loads(config_path.read_text(encoding="utf-8"))
    except OSError as error:
        raise ValidationError(
            f"persisted feedback configuration could not be read: {_bounded_error(str(error))}"
        ) from error
    except json.JSONDecodeError as error:
        raise ValidationError(
            "persisted feedback configuration is malformed JSON"
        ) from error
    if not isinstance(parsed, dict):
        raise ValidationError("persisted feedback configuration must be an object")
    unknown = sorted(set(parsed) - set(DEFAULT_GENERATION_CONFIG))
    if unknown:
        raise ValidationError(
            "persisted feedback configuration has unsupported fields: "
            + ", ".join(unknown)
        )
    return parsed


def resolve_feedback_config(
    *,
    invocation_overrides: Mapping[str, Any] | None = None,
    environment: Mapping[str, str] | None = None,
    config_path: str | Path | None = None,
) -> dict[str, Any]:
    """Resolve the typed non-secret feedback configuration through one path."""
    invocation = invocation_overrides or {}
    if not isinstance(invocation, Mapping):
        raise ValidationError("invocation configuration must be an object")
    unknown = sorted(set(invocation) - set(DEFAULT_GENERATION_CONFIG))
    if unknown:
        raise ValidationError(
            "invocation configuration has unsupported fields: " + ", ".join(unknown)
        )
    environment_values = os.environ if environment is None else environment
    if not isinstance(environment_values, Mapping):
        raise ValidationError("configuration environment must be an object")
    persisted_path = (
        Path(config_path).expanduser()
        if config_path is not None
        else Path.home() / ".jcode" / "feedback" / "config.json"
    )
    persisted = _load_persisted_feedback_config(persisted_path)

    values: dict[str, Any] = {}
    sources: dict[str, str] = {}
    diagnostics: dict[str, dict[str, Any]] = {}
    for field, default in DEFAULT_GENERATION_CONFIG.items():
        candidates = (
            ("invocation", invocation.get(field)),
            ("environment", environment_values.get(CONFIG_ENVIRONMENT_NAMES[field])),
            ("persisted", persisted.get(field)),
            ("default", default),
        )
        configured = {
            source: candidate
            for source, candidate in candidates[:-1]
            if candidate is not None and _configured_value(candidate)
        }
        for source, candidate in candidates:
            if candidate is None or not _configured_value(candidate):
                continue
            values[field] = _parse_config_value(field, candidate, source)
            sources[field] = source
            break
        diagnostics[field] = {
            "configured": configured,
            "effective": values[field],
            "source": sources[field],
        }

    return {
        "values": values,
        "sources": sources,
        "diagnostics": diagnostics,
        "route": {
            "provider": "openai",
            "authentication": "native-oauth",
            "max_requests": 1,
        },
        "persisted_config_path": str(persisted_path),
    }


def validate_budget_limit(
    *, field: str, observed: int | float, limit: int | float
) -> None:
    """Fail when a measured non-negative budget contribution exceeds its limit."""
    if field not in INTEGER_CONFIG_FIELDS | FLOAT_CONFIG_FIELDS:
        raise ValidationError(f"unsupported budget field: {field}")
    parsed_limit = _parse_config_value(field, limit, "effective configuration")
    if (
        not isinstance(observed, (int, float))
        or isinstance(observed, bool)
        or not math.isfinite(observed)
        or observed < 0
    ):
        raise ValidationError(f"observed {field} must be a non-negative finite number")
    if observed > parsed_limit:
        raise ValidationError(
            f"{field} exceeded: observed {observed}, limit {parsed_limit}"
        )


def _generation_limit(
    config: Mapping[str, Any], name: str, *, integer: bool = False
) -> int | float:
    value = config.get(name)
    valid = (
        isinstance(value, int) and not isinstance(value, bool) and value > 0
        if integer
        else isinstance(value, (int, float))
        and not isinstance(value, bool)
        and math.isfinite(value)
        and value > 0
    )
    if not valid:
        kind = "positive integer" if integer else "positive finite number"
        raise ValidationError(f"{name} limit must be a {kind}")
    return int(value) if integer else float(value)


def _copy_oauth_credential(source: Path, destination: Path) -> None:
    try:
        metadata = source.lstat()
        if source.is_symlink() or not source.is_file():
            raise ValidationError("OpenAI OAuth credential must be a regular file")
        if metadata.st_size <= 0 or metadata.st_size > MAX_OAUTH_CREDENTIAL_BYTES:
            raise ValidationError("OpenAI OAuth credential has an invalid size")
        destination.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        shutil.copyfile(source, destination)
        destination.chmod(0o600)
    except ValidationError:
        raise
    except OSError as error:
        raise ValidationError(
            f"OpenAI OAuth credential could not be isolated: {_bounded_error(str(error))}"
        ) from error


def _isolated_generation_environment(root: Path) -> tuple[dict[str, str], Path, Path]:
    source_home = Path(os.environ.get("HOME", str(Path.home()))).expanduser()
    source_jcode_home = Path(
        os.environ.get("JCODE_HOME", str(source_home / ".jcode"))
    ).expanduser()
    isolated_home = root / "home"
    isolated_jcode_home = root / "jcode-home"
    runtime = root / "runtime"
    workspace = isolated_home / "workspace"
    for directory in (
        isolated_home,
        isolated_jcode_home,
        runtime,
        workspace,
        root / "xdg-config",
        root / "xdg-cache",
        root / "xdg-data",
        root / "xdg-state",
    ):
        directory.mkdir(parents=True, exist_ok=True, mode=0o700)

    native_auth = source_jcode_home / "openai-auth.json"
    legacy_auth = source_home / ".codex" / "auth.json"
    allow_legacy = False
    if native_auth.is_file() and not native_auth.is_symlink():
        _copy_oauth_credential(native_auth, isolated_jcode_home / "openai-auth.json")
    elif legacy_auth.is_file() and not legacy_auth.is_symlink():
        _copy_oauth_credential(
            legacy_auth,
            isolated_jcode_home / "external" / ".codex" / "auth.json",
        )
        allow_legacy = True
    else:
        raise ValidationError("OpenAI OAuth credential was not found")

    environment = {
        key: value
        for key in GENERATION_ENV_ALLOWLIST
        if isinstance((value := os.environ.get(key)), str) and value
    }
    environment.update(
        {
            "HOME": str(isolated_home),
            "JCODE_HOME": str(isolated_jcode_home),
            "JCODE_RUNTIME_DIR": str(runtime),
            "XDG_CONFIG_HOME": str(root / "xdg-config"),
            "XDG_CACHE_HOME": str(root / "xdg-cache"),
            "XDG_DATA_HOME": str(root / "xdg-data"),
            "XDG_STATE_HOME": str(root / "xdg-state"),
            "JCODE_TEMP_SERVER": "1",
            "JCODE_SERVER_OWNER_PID": str(os.getpid()),
            "JCODE_TEMP_SERVER_IDLE_SECS": "5",
            "PATH": os.pathsep.join(
                part
                for part in ("/usr/bin", "/bin", environment.get("PATH", ""))
                if part
            ),
        }
    )
    if allow_legacy:
        environment["JCODE_ALLOW_CODEX_LEGACY_AUTH"] = "1"
    else:
        environment.pop("JCODE_ALLOW_CODEX_LEGACY_AUTH", None)
    return environment, workspace, runtime / "session-feedback.sock"


def _parse_jcode_json_report(payload: str) -> tuple[str, int, int]:
    try:
        report = json.loads(payload)
    except json.JSONDecodeError as error:
        raise ValidationError(
            "generation runner returned malformed Jcode JSON"
        ) from error
    if not isinstance(report, Mapping) or not isinstance(report.get("text"), str):
        raise ValidationError("generation runner returned an invalid Jcode JSON report")
    usage = report.get("usage")
    if not isinstance(usage, Mapping):
        raise ValidationError("generation runner omitted Jcode usage accounting")
    observed: list[int] = []
    for field in ("input_tokens", "output_tokens"):
        value = usage.get(field)
        if not isinstance(value, int) or isinstance(value, bool) or value < 0:
            raise ValidationError(f"generation runner returned invalid {field}")
        observed.append(value)
    return report["text"], observed[0], observed[1]


def _run_generation_command(
    command: Sequence[str], *, timeout_seconds: float, estimated_cost_usd: float
) -> dict[str, Any]:
    if len(command) < 2 or command[1] != "run":
        raise ValidationError("generation command must invoke jcode run")
    executable = shutil.which(command[0], path=os.environ.get("PATH"))
    if executable is None:
        raise ValidationError(
            "generation request could not start: executable not found"
        )
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="jcode-session-feedback-") as directory:
        root = Path(directory)
        environment, workspace, socket_path = _isolated_generation_environment(root)
        run_command = [
            executable,
            "run",
            "--no-update",
            "--socket",
            str(socket_path),
            *command[2:],
        ]
        try:
            completed = subprocess.run(
                run_command,
                text=True,
                capture_output=True,
                check=False,
                timeout=timeout_seconds,
                cwd=workspace,
                env=environment,
            )
        except OSError as error:
            raise ValidationError(
                f"generation request could not start: {_bounded_error(str(error))}"
            ) from error
        except subprocess.TimeoutExpired as error:
            elapsed = time.monotonic() - started
            raise ValidationError(
                f"generation elapsed-time limit exceeded after {elapsed:.3f} seconds"
            ) from error
        finally:
            subprocess.run(
                [
                    executable,
                    "--socket",
                    str(socket_path),
                    "server",
                    "stop",
                    "--force",
                    "--json",
                ],
                text=True,
                capture_output=True,
                check=False,
                timeout=5.0,
                cwd=workspace,
                env=environment,
            )
        stdout = completed.stdout
        observed_input_tokens = None
        observed_output_tokens = None
        if completed.returncode == 0:
            stdout, observed_input_tokens, observed_output_tokens = (
                _parse_jcode_json_report(completed.stdout)
            )
        return {
            "returncode": completed.returncode,
            "stdout": stdout,
            "stderr": completed.stderr,
            "elapsed_seconds": time.monotonic() - started,
            "estimated_cost_usd": estimated_cost_usd,
            "observed_input_tokens": observed_input_tokens,
            "observed_output_tokens": observed_output_tokens,
        }


def _validated_runner_receipt(
    receipt: Any,
) -> tuple[str, float, float, int | None, int | None]:
    if not isinstance(receipt, Mapping):
        raise ValidationError("generation runner returned an invalid receipt")
    returncode = receipt.get("returncode")
    stdout = receipt.get("stdout")
    stderr = receipt.get("stderr", "")
    elapsed = receipt.get("elapsed_seconds")
    cost = receipt.get("estimated_cost_usd")
    if not isinstance(returncode, int) or isinstance(returncode, bool):
        raise ValidationError("generation runner receipt has an invalid returncode")
    if not isinstance(stdout, str) or not isinstance(stderr, str):
        raise ValidationError("generation runner receipt must contain text output")
    for name, value in (("elapsed_seconds", elapsed), ("estimated_cost_usd", cost)):
        if (
            not isinstance(value, (int, float))
            or isinstance(value, bool)
            or not math.isfinite(value)
            or value < 0
        ):
            raise ValidationError(f"generation runner receipt has invalid {name}")
    if returncode != 0:
        detail = _bounded_error(stderr.strip() or "no bounded error detail")
        raise ValidationError(
            f"generation request failed with exit {returncode}: {detail}"
        )
    observed_tokens: list[int | None] = []
    for field in ("observed_input_tokens", "observed_output_tokens"):
        value = receipt.get(field)
        if value is not None and (
            not isinstance(value, int) or isinstance(value, bool) or value < 0
        ):
            raise ValidationError(f"generation runner receipt has invalid {field}")
        observed_tokens.append(value)
    return stdout, float(elapsed), float(cost), observed_tokens[0], observed_tokens[1]


def _validate_generated_proposals(
    response: Mapping[str, Any],
    *,
    evidence: Mapping[str, Any],
    shortlisted_targets: Sequence[Mapping[str, Any]],
) -> list[dict[str, Any]]:
    evidence_references = {item["reference"] for item in evidence["items"]}
    targets = {
        (
            normalize_category(target["category"]),
            normalize_scope(target["scope"]),
            normalize_concrete_target(target["concrete_target"]),
        )
        for target in shortlisted_targets
    }
    proposals: list[dict[str, Any]] = []
    for index, proposal in enumerate(response["proposals"]):
        target = proposal["target"]
        target_identity = (
            normalize_category(target["category"]),
            normalize_scope(target["scope"]),
            normalize_concrete_target(target["concrete_target"]),
        )
        if target_identity not in targets:
            raise ValidationError(
                f"generated proposal {index} targets an item outside the shortlist"
            )
        unresolved = sorted(set(proposal["evidence_references"]) - evidence_references)
        if unresolved:
            raise ValidationError(
                f"generated proposal {index} has unresolved evidence references: "
                + ", ".join(unresolved)
            )
        expected_fingerprint = proposal_fingerprint(
            {
                "category": target["category"],
                "scope": target["scope"],
                "concrete_target": target["concrete_target"],
                "problem": proposal["problem"],
                "intended_outcome": proposal["expected_benefit"],
            }
        )
        canonical_proposal = json.loads(canonical_json(proposal))
        canonical_proposal["target"] = {
            "category": target_identity[0],
            "scope": target_identity[1],
            "concrete_target": target_identity[2],
        }
        canonical_proposal["evidence_references"] = normalize_evidence_references(
            proposal["evidence_references"]
        )
        canonical_proposal["fingerprint"] = expected_fingerprint
        validate_contract("proposal-v1", canonical_proposal)
        proposals.append(canonical_proposal)
    return proposals


def _markdown_block(value: str) -> str:
    return "\n".join(f"> {line}" if line else ">" for line in value.splitlines())


def _markdown_list(values: Sequence[str]) -> str:
    return "\n".join(f"{index}. {value}" for index, value in enumerate(values, 1))


def render_review_proposal(proposal: Mapping[str, Any]) -> dict[str, Any]:
    """Render one validated proposal without mutating or persisting its target."""
    if not isinstance(proposal, Mapping):
        raise ValidationError("proposal must be an object")
    validate_contract("proposal-v1", proposal)
    canonical_proposal = json.loads(canonical_json(proposal))
    target = canonical_proposal["target"]
    expected_fingerprint = proposal_fingerprint(
        {
            "category": target["category"],
            "scope": target["scope"],
            "concrete_target": target["concrete_target"],
            "problem": canonical_proposal["problem"],
            "intended_outcome": canonical_proposal["expected_benefit"],
        }
    )
    if canonical_proposal["fingerprint"] != expected_fingerprint:
        raise ValidationError("proposal fingerprint does not match normalized material")

    document = {
        "proposal": canonical_proposal,
        "review_only": True,
        "review_state": "needs-approval",
    }
    json_rendering = canonical_json(document)
    markdown_rendering = "\n".join(
        [
            "# Session Feedback Proposal",
            "",
            "**State:** review-only",
            "**Review state:** needs-approval",
            f"**Category:** {target['category']}",
            f"**Scope:** {target['scope']}",
            f"**Concrete target:** {target['concrete_target']}",
            f"**Evidence references:** {', '.join(canonical_proposal['evidence_references'])}",
            f"**Risk:** {canonical_proposal['risk']}",
            f"**Confidence:** {canonical_proposal['confidence']}",
            f"**Fingerprint:** {canonical_proposal['fingerprint']}",
            "",
            "## Problem",
            _markdown_block(canonical_proposal["problem"]),
            "",
            "## Hypothesis",
            _markdown_block(canonical_proposal["hypothesis"]),
            "",
            "## Suggested behavior or patch outline",
            _markdown_block(canonical_proposal["suggested_behavior"]),
            "",
            "## Expected benefit",
            _markdown_block(canonical_proposal["expected_benefit"]),
            "",
            "## Token or context impact",
            _markdown_block(canonical_proposal["token_context_impact"]),
            "",
            "## Blast radius",
            _markdown_block(canonical_proposal["blast_radius"]),
            "",
            "## Validation plan",
            _markdown_list(canonical_proposal["validation_plan"]),
            "",
            "## Non-goals",
            _markdown_list(canonical_proposal["non_goals"]),
            "",
            "## Canonical review JSON",
            "",
            "    " + json_rendering,
            "",
        ]
    )
    accounting = {
        "json": measure_text("proposal_json", json_rendering),
        "markdown": measure_text("proposal_markdown", markdown_rendering),
    }
    for format_name, measurement in accounting.items():
        if measurement["bytes"] > MAX_RENDERED_PROPOSAL_BYTES:
            raise ValidationError(
                f"rendered proposal {format_name} exceeds the "
                f"{MAX_RENDERED_PROPOSAL_BYTES} byte limit"
            )
    return {
        "json": json_rendering,
        "markdown": markdown_rendering,
        "accounting": accounting,
    }


def _default_bd_runner(command: Sequence[str], *, cwd: Path) -> dict[str, Any]:
    completed = subprocess.run(
        list(command),
        cwd=cwd,
        capture_output=True,
        text=True,
        check=False,
    )
    return {
        "returncode": completed.returncode,
        "stdout": completed.stdout,
        "stderr": completed.stderr,
    }


def _run_bd(
    runner: Any,
    command: Sequence[str],
    *,
    cwd: Path,
    operation: str,
) -> str:
    if not command or command[0] != "bd":
        raise ValidationError("feedback persistence permits only local bd commands")
    forbidden = {"remote", "replicate", "sync", "push", "pull", "delete"}
    if forbidden & {part.lower() for part in command}:
        raise ValidationError(
            "feedback persistence forbids remote or destructive bd commands"
        )
    try:
        receipt = runner(tuple(command), cwd=cwd)
    except (OSError, subprocess.SubprocessError) as error:
        raise ValidationError(
            _bounded_error(f"bd {operation} failed: {error}")
        ) from error
    if not isinstance(receipt, Mapping):
        raise ValidationError(f"bd {operation} returned a malformed receipt")
    returncode = receipt.get("returncode")
    stdout = receipt.get("stdout", "")
    stderr = receipt.get("stderr", "")
    if (
        not isinstance(returncode, int)
        or not isinstance(stdout, str)
        or not isinstance(stderr, str)
    ):
        raise ValidationError(f"bd {operation} returned a malformed receipt")
    if returncode != 0:
        detail = stderr.strip() or stdout.strip() or f"exit status {returncode}"
        raise ValidationError(_bounded_error(f"bd {operation} failed: {detail}"))
    return stdout


def _feedback_root(value: str | Path) -> Path:
    root = Path(value).expanduser()
    if not root.is_absolute():
        raise ValidationError("feedback_root must be an absolute path")
    return root.resolve(strict=False)


def bootstrap_feedback_store(
    *,
    feedback_root: str | Path = Path.home() / ".jcode" / "feedback",
    bd_runner: Any | None = None,
) -> dict[str, str]:
    """Create the contained local feedback layout and initialize Beads once."""
    root = _feedback_root(feedback_root)
    runs = root / "runs"
    proposals = root / "proposals"
    beads = root / ".beads"
    try:
        root.mkdir(parents=True, exist_ok=True)
        runs.mkdir(exist_ok=True)
        proposals.mkdir(exist_ok=True)
        if os.name != "nt":
            for directory in (root, runs, proposals):
                directory.chmod(0o700)
    except OSError as error:
        raise ValidationError(
            _bounded_error(f"unable to bootstrap feedback storage: {error}")
        ) from error

    runner = _default_bd_runner if bd_runner is None else bd_runner
    if not beads.is_dir():
        _run_bd(runner, ["bd", "init", "--quiet"], cwd=root, operation="init")
    if not beads.is_dir():
        raise ValidationError("bd init did not create the local .beads directory")
    if os.name != "nt":
        try:
            beads.chmod(0o700)
        except OSError as error:
            raise ValidationError(
                _bounded_error(f"unable to secure feedback storage: {error}")
            ) from error
    return {
        "feedback_root": str(root),
        "runs": str(runs),
        "proposals": str(proposals),
        "beads": str(beads),
    }


def _validated_evidence_occurrence(value: Mapping[str, Any]) -> dict[str, Any]:
    if not isinstance(value, Mapping):
        raise ValidationError("evidence_occurrence must be an object")
    required = {
        "occurrence_id",
        "session_id",
        "observed_at",
        "evidence_references",
        "evidence_digest",
    }
    if set(value) != required:
        raise ValidationError("evidence_occurrence has missing or unknown fields")
    occurrence = json.loads(canonical_json(value))
    for field in ("occurrence_id", "session_id", "observed_at"):
        item = occurrence[field]
        if not isinstance(item, str) or not item.strip() or len(item) > 128:
            raise ValidationError(
                f"evidence_occurrence.{field} is invalid or oversized"
            )
    references = occurrence["evidence_references"]
    if not isinstance(references, list) or not 1 <= len(references) <= 32:
        raise ValidationError(
            "evidence_occurrence.evidence_references is invalid or oversized"
        )
    if any(
        not isinstance(item, str) or not item.strip() or len(item) > 256
        for item in references
    ):
        raise ValidationError(
            "evidence_occurrence.evidence_references contains an invalid value"
        )
    if len(set(references)) != len(references):
        raise ValidationError(
            "evidence_occurrence.evidence_references contains duplicates"
        )
    digest = occurrence["evidence_digest"]
    if not isinstance(digest, str) or re.fullmatch(r"[0-9a-f]{64}", digest) is None:
        raise ValidationError(
            "evidence_occurrence.evidence_digest must be lowercase SHA-256"
        )
    if len(canonical_bytes(occurrence)) > 8_192:
        raise ValidationError("evidence_occurrence exceeds the byte limit")
    return occurrence


def _record_labels(record: Mapping[str, Any]) -> set[str]:
    labels = record.get("labels", [])
    if isinstance(labels, str):
        return {item.strip() for item in labels.split(",") if item.strip()}
    if isinstance(labels, Sequence) and not isinstance(labels, (str, bytes)):
        if all(isinstance(item, str) for item in labels):
            return {item for item in labels if item}
    raise ValidationError("malformed session-feedback record labels")


def _record_fingerprint(record: Mapping[str, Any]) -> str:
    fingerprint = record.get("fingerprint")
    if fingerprint is None and isinstance(record.get("metadata"), Mapping):
        fingerprint = record["metadata"].get("fingerprint")
    if fingerprint is None and isinstance(record.get("description"), str):
        match = re.search(
            r"(?im)^\*\*Fingerprint:\*\*\s*([0-9a-f]{64})\s*$", record["description"]
        )
        fingerprint = match.group(1) if match else None
    if (
        not isinstance(fingerprint, str)
        or re.fullmatch(r"[0-9a-f]{64}", fingerprint) is None
    ):
        raise ValidationError("malformed session-feedback record fingerprint")
    return fingerprint


def _searchable_record(record: Mapping[str, Any]) -> bool:
    labels = _record_labels(record)
    if "session-feedback" not in labels:
        return False
    status = record.get("status")
    if status in {"open", "in_progress", "in-progress"}:
        return True
    return status == "closed" and "feedback-relevant" in labels


def _record_identity(record: Mapping[str, Any]) -> str:
    bead_id = record.get("bead_id", record.get("id"))
    if not isinstance(bead_id, str) or not bead_id.strip() or len(bead_id) > 256:
        raise ValidationError("malformed session-feedback record identity")
    return bead_id


def _record_evidence_history(record: Mapping[str, Any]) -> list[dict[str, Any]]:
    history = record.get("evidence_history", [])
    if not isinstance(history, Sequence) or isinstance(history, (str, bytes)):
        raise ValidationError("malformed session-feedback evidence history")
    return [_validated_evidence_occurrence(item) for item in history]


def _write_proposal_artifacts(
    *,
    proposals_dir: Path,
    proposal: Mapping[str, Any],
    occurrence: Mapping[str, Any],
    atomic_replace: Any,
) -> tuple[Path, Path]:
    rendered = render_review_proposal(proposal)
    fingerprint = proposal["fingerprint"]
    json_path = proposals_dir / f"{fingerprint}.json"
    markdown_path = proposals_dir / f"{fingerprint}.md"
    if json_path.exists() or markdown_path.exists():
        raise ValidationError(
            "proposal artifact already exists without a matching record"
        )
    document = json.loads(rendered["json"])
    document["evidence_history"] = [occurrence]
    json_text = canonical_json(document) + "\n"
    markdown_text = (
        rendered["markdown"]
        + "## Evidence history\n\n    "
        + canonical_json(occurrence)
        + "\n"
    )
    staged: list[Path] = []
    committed: list[Path] = []
    try:
        for destination, content in (
            (json_path, json_text),
            (markdown_path, markdown_text),
        ):
            descriptor, temporary_name = tempfile.mkstemp(
                prefix=f".{fingerprint}.", suffix=".tmp", dir=proposals_dir
            )
            temporary = Path(temporary_name)
            staged.append(temporary)
            with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
                handle.write(content)
                handle.flush()
                os.fsync(handle.fileno())
            atomic_replace(temporary, destination)
            staged.remove(temporary)
            committed.append(destination)
    except OSError as error:
        for path in staged + committed:
            try:
                path.unlink(missing_ok=True)
            except OSError:
                pass
        raise ValidationError(
            _bounded_error(f"atomic proposal persistence failed: {error}")
        ) from error
    return json_path, markdown_path


def persist_review_proposal(
    *,
    proposal: Mapping[str, Any],
    evidence_occurrence: Mapping[str, Any],
    feedback_root: str | Path = Path.home() / ".jcode" / "feedback",
    bd_runner: Any | None = None,
    atomic_replace: Any = os.replace,
) -> dict[str, Any]:
    """Create one review record or append one bounded, nonduplicate occurrence."""
    rendered = render_review_proposal(proposal)
    canonical_proposal = json.loads(rendered["json"])["proposal"]
    occurrence = _validated_evidence_occurrence(evidence_occurrence)
    root = _feedback_root(feedback_root)
    runner = _default_bd_runner if bd_runner is None else bd_runner
    layout = bootstrap_feedback_store(feedback_root=root, bd_runner=runner)
    proposals_dir = Path(layout["proposals"])

    stdout = _run_bd(
        runner,
        ["bd", "list", "--json", "--label", "session-feedback", "--limit", "0"],
        cwd=root,
        operation="list",
    )
    try:
        records = json.loads(stdout or "[]")
    except json.JSONDecodeError as error:
        raise ValidationError(
            _bounded_error(f"bd list returned malformed JSON: {error}")
        ) from error
    if not isinstance(records, list) or any(
        not isinstance(item, Mapping) for item in records
    ):
        raise ValidationError("bd list returned malformed session-feedback records")

    matches: list[Mapping[str, Any]] = []
    for record in records:
        if not _searchable_record(record):
            continue
        if _record_fingerprint(record) == canonical_proposal["fingerprint"]:
            matches.append(record)
    if len(matches) > 1:
        raise ValidationError("ambiguous session-feedback fingerprint matches")
    if matches:
        match = matches[0]
        bead_id = _record_identity(match)
        history = _record_evidence_history(match)
        if occurrence in history:
            return {"action": "duplicate_evidence_ignored", "bead_id": bead_id}
        _run_bd(
            runner,
            ["bd", "comments", "add", bead_id, canonical_json(occurrence)],
            cwd=root,
            operation="append evidence",
        )
        return {"action": "evidence_appended", "bead_id": bead_id}

    json_path, markdown_path = _write_proposal_artifacts(
        proposals_dir=proposals_dir,
        proposal=canonical_proposal,
        occurrence=occurrence,
        atomic_replace=atomic_replace,
    )
    title = f"Session feedback: {canonical_proposal['target']['concrete_target']}"
    try:
        create_stdout = _run_bd(
            runner,
            [
                "bd",
                "create",
                title,
                "--description",
                rendered["markdown"],
                "--labels",
                "session-feedback,needs-approval",
                "--json",
            ],
            cwd=root,
            operation="create",
        )
        created = json.loads(create_stdout)
        bead_id = created.get("id") if isinstance(created, Mapping) else None
        if not isinstance(bead_id, str) or not bead_id:
            raise ValidationError("bd create returned a malformed record identity")
    except (ValidationError, json.JSONDecodeError):
        json_path.unlink(missing_ok=True)
        markdown_path.unlink(missing_ok=True)
        raise
    return {
        "action": "created",
        "bead_id": bead_id,
        "proposal_json": str(json_path),
        "proposal_markdown": str(markdown_path),
    }


def generate_review_proposals(
    *,
    request_input: Mapping[str, Any],
    evidence: Mapping[str, Any],
    shortlisted_targets: Sequence[Mapping[str, Any]],
    effective_config: Mapping[str, Any],
    runner: Any | None = None,
) -> dict[str, Any]:
    """Issue at most one no-tools structured generation request and validate it locally."""
    if not isinstance(request_input, Mapping) or not isinstance(
        effective_config, Mapping
    ):
        raise ValidationError("generation input and effective_config must be objects")
    validate_contract("evidence-v1", evidence)
    if not isinstance(shortlisted_targets, Sequence) or isinstance(
        shortlisted_targets, (str, bytes)
    ):
        raise ValidationError("shortlisted_targets must be a sequence")

    model = effective_config.get("model")
    effort = effective_config.get("effort")
    if not isinstance(model, str) or not model.strip() or len(model) > 128:
        raise ValidationError("model must be a bounded non-empty string")
    if effort not in SUPPORTED_EFFORTS:
        raise ValidationError("effort must be a supported reasoning effort")
    max_input_tokens = _generation_limit(
        effective_config, "max_input_tokens", integer=True
    )
    max_output_tokens = _generation_limit(
        effective_config, "max_output_tokens", integer=True
    )
    max_proposals = _generation_limit(effective_config, "max_proposals", integer=True)
    max_elapsed = _generation_limit(effective_config, "max_elapsed_seconds")
    max_cost = _generation_limit(effective_config, "max_estimated_cost_usd")
    model_route = f"openai-oauth:{model.strip()}"

    schema_path = SCHEMA_DIR / "generator-response-v1.schema.json"
    schema_document = json.loads(schema_path.read_text(encoding="utf-8"))
    request_document = canonical_json(request_input)
    prompt = (
        "Return exactly one JSON object matching the generator-response-v1 schema. "
        "Do not wrap it in markdown or explanatory text. Use no tools and perform no "
        "side effects. Schema: "
        + canonical_json(schema_document)
        + " Input: "
        + request_document
    )
    request_accounting = measure_text("request_input", prompt)
    validate_budget_limit(
        field="max_input_tokens",
        observed=request_accounting["estimated_tokens"],
        limit=max_input_tokens,
    )
    command = [
        "jcode",
        "run",
        "--model",
        model_route,
        "--reasoning-effort",
        effort,
        "--tool-profile",
        "none",
        "--max-turns",
        "1",
        "--token-budget",
        str(max_input_tokens + max_output_tokens),
        "--json",
        prompt,
    ]

    invoke = runner
    if invoke is None:
        invoke = lambda value: _run_generation_command(  # noqa: E731
            value,
            timeout_seconds=max_elapsed,
            estimated_cost_usd=max_cost,
        )
    (
        stdout,
        elapsed_seconds,
        estimated_cost_usd,
        observed_input_tokens,
        observed_output_tokens,
    ) = _validated_runner_receipt(invoke(command))
    validate_budget_limit(
        field="max_elapsed_seconds", observed=elapsed_seconds, limit=max_elapsed
    )
    validate_budget_limit(
        field="max_estimated_cost_usd", observed=estimated_cost_usd, limit=max_cost
    )
    if observed_input_tokens is not None:
        validate_budget_limit(
            field="max_input_tokens",
            observed=observed_input_tokens,
            limit=max_input_tokens,
        )
    if observed_output_tokens is not None:
        validate_budget_limit(
            field="max_output_tokens",
            observed=observed_output_tokens,
            limit=max_output_tokens,
        )

    output_accounting = measure_text("request_output", stdout)
    validate_budget_limit(
        field="max_output_tokens",
        observed=output_accounting["estimated_tokens"],
        limit=max_output_tokens,
    )
    response = parse_contract("generator-response-v1", stdout)
    proposals = _validate_generated_proposals(
        response,
        evidence=evidence,
        shortlisted_targets=shortlisted_targets,
    )
    validate_budget_limit(
        field="max_proposals", observed=len(proposals), limit=max_proposals
    )
    rendered_proposals = [render_review_proposal(proposal) for proposal in proposals]

    return {
        "contract_version": "generator-response-v1",
        "proposals": proposals,
        "rendered_proposals": rendered_proposals,
        "accounting": {
            "request_count": 1,
            "request_input": request_accounting,
            "request_output": output_accounting,
            "proposal_count": len(proposals),
            "elapsed_seconds": elapsed_seconds,
            "estimated_cost_usd": estimated_cost_usd,
            "command": {
                "provider": "openai",
                "model": model_route,
                "effort": effort,
                "tool_profile": "none",
                "request_count_limit": 1,
                "max_turns": 1,
                "output_mode": "json",
                "schema": schema_path.name,
            },
            "observed_input_tokens": observed_input_tokens,
            "observed_output_tokens": observed_output_tokens,
        },
    }


def build_review_outcome(
    *,
    session_id: str,
    proposal_locations: Sequence[str] = (),
    failure: str | None = None,
) -> dict[str, Any]:
    """Build a bounded result whose success, persistence, and failure states are distinct."""
    selected = _validate_session_id("session_id", session_id)
    if not isinstance(proposal_locations, Sequence) or isinstance(
        proposal_locations, (str, bytes)
    ):
        raise ValidationError("proposal_locations must be a sequence of paths")
    if len(proposal_locations) > MAX_PROPOSAL_LOCATIONS:
        raise ValidationError(
            f"proposal_locations exceeds the {MAX_PROPOSAL_LOCATIONS} item limit"
        )

    locations: list[str] = []
    for index, location in enumerate(proposal_locations):
        if not isinstance(location, str) or not location.strip():
            raise ValidationError(
                f"proposal_locations[{index}] must be a non-empty path"
            )
        if len(location) > 512 or any(
            ord(character) < 32 or ord(character) == 127 for character in location
        ):
            raise ValidationError(
                f"proposal_locations[{index}] is invalid or oversized"
            )
        locations.append(location)

    if failure is not None:
        if locations:
            raise ValidationError(
                "a failed review outcome cannot include persisted proposals"
            )
        if not isinstance(failure, str) or not failure.strip():
            raise ValidationError("failure must be a non-empty string")
        return {
            "status": "failed",
            "session_id": selected,
            "proposal_count": 0,
            "proposal_locations": [],
            "failure": _bounded_error(failure.strip()),
        }

    return {
        "status": "proposals_persisted" if locations else "zero_proposals",
        "session_id": selected,
        "proposal_count": len(locations),
        "proposal_locations": locations,
        "failure": None,
    }


def run_feedback(
    *,
    requested_session_id: str | None,
    current_session_id: str | None,
    visible_session_ids: Sequence[str],
    visible_items: Sequence[Mapping[str, Any]],
    librarian_summary_path: str | Path | None,
    target_root: str | Path = ".",
    effective_config: Mapping[str, Any] | None = None,
    invocation_config: Mapping[str, Any] | None = None,
    environment: Mapping[str, str] | None = None,
    feedback_config_path: str | Path | None = None,
    runner: Any | None = None,
    per_excerpt_byte_limit: int = DEFAULT_PER_EXCERPT_BYTES,
    total_excerpt_byte_limit: int = DEFAULT_TOTAL_EXCERPT_BYTES,
) -> dict[str, Any]:
    """Run one bounded acquisition, shortlist, excerpt, and generation workflow."""
    if effective_config is not None and invocation_config is not None:
        raise ValidationError(
            "effective_config and invocation_config cannot be supplied together"
        )
    if effective_config is None:
        resolved_config = resolve_feedback_config(
            invocation_overrides=invocation_config,
            environment=environment,
            config_path=feedback_config_path,
        )
        config = resolved_config["values"]
    else:
        config = dict(DEFAULT_GENERATION_CONFIG)
        config.update(effective_config)
        for field, value in config.items():
            config[field] = _parse_config_value(field, value, "effective configuration")

    invocation = prepare_feedback_invocation(
        requested_session_id=requested_session_id,
        current_session_id=current_session_id,
        visible_session_ids=visible_session_ids,
        visible_items=visible_items,
        librarian_summary_path=librarian_summary_path,
    )
    evidence = invocation["evidence"]
    validate_budget_limit(
        field="max_evidence_bytes",
        observed=evidence["accounting"]["serialized_bytes"],
        limit=config["max_evidence_bytes"],
    )
    shortlist = shortlist_targets(evidence)
    excerpt_limit = int(config["max_excerpt_bytes"])
    excerpts = load_shortlisted_excerpts(
        shortlist=shortlist,
        target_root=target_root,
        per_excerpt_byte_limit=min(per_excerpt_byte_limit, excerpt_limit),
        total_excerpt_byte_limit=min(total_excerpt_byte_limit, excerpt_limit),
    )
    excerpt_totals = excerpt_accounting(excerpts)
    validate_budget_limit(
        field="max_excerpt_bytes",
        observed=excerpt_totals["serialized_bytes"],
        limit=excerpt_limit,
    )
    request_input = {
        "session_id": invocation["session_id"],
        "evidence": evidence,
        "shortlisted_targets": shortlist,
        "excerpts": excerpts,
    }
    generation = generate_review_proposals(
        request_input=request_input,
        evidence=evidence,
        shortlisted_targets=shortlist,
        effective_config=config,
        runner=runner,
    )
    generation_accounting = generation["accounting"]
    proposal_count = generation_accounting["proposal_count"]
    return {
        "status": "proposals_generated" if proposal_count else "zero_proposals",
        "session_id": invocation["session_id"],
        "proposal_count": proposal_count,
        "proposal_locations": [],
        "failure": None,
        "evidence_source": evidence["source"],
        "effective_config": config,
        "shortlisted_target_count": len(shortlist),
        "proposals": generation["proposals"],
        "rendered_proposals": generation["rendered_proposals"],
        "accounting": {
            "evidence": evidence["accounting"],
            "evidence_bytes": evidence["accounting"]["serialized_bytes"],
            "excerpts": excerpt_totals,
            "excerpt_bytes": excerpt_totals["serialized_bytes"],
            "request_count": generation_accounting["request_count"],
            "request_input": generation_accounting["request_input"],
            "request_output": generation_accounting["request_output"],
            "observed_input_tokens": generation_accounting["observed_input_tokens"],
            "observed_output_tokens": generation_accounting["observed_output_tokens"],
            "proposal_count": proposal_count,
            "elapsed_seconds": generation_accounting["elapsed_seconds"],
            "estimated_cost_usd": generation_accounting["estimated_cost_usd"],
            "command": generation_accounting["command"],
        },
    }


__all__ = [
    "ContractValidationError",
    "ValidationError",
    "canonical_bytes",
    "canonical_json",
    "bootstrap_feedback_store",
    "build_review_outcome",
    "DEFAULT_GENERATION_CONFIG",
    "evidence_accounting",
    "fingerprint_material",
    "generate_review_proposals",
    "load_schema",
    "measure_json",
    "measure_text",
    "normalize_category",
    "normalize_concrete_target",
    "normalize_evidence_reference",
    "normalize_evidence_references",
    "normalize_scope",
    "normalize_text",
    "parse_contract",
    "persist_review_proposal",
    "prepare_feedback_invocation",
    "proposal_fingerprint",
    "resolve_feedback_config",
    "render_review_proposal",
    "run_feedback",
    "validate_budget_limit",
    "validate_contract",
    "with_evidence_accounting",
]
