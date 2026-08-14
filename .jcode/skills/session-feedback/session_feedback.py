#!/usr/bin/env python3
"""Dependency-free deterministic primitives for the session-feedback skill."""

from __future__ import annotations

import hashlib
import json
import math
import re
import unicodedata
from pathlib import Path
from typing import Any, Mapping, Sequence


SKILL_DIR = Path(__file__).resolve().parent
SCHEMA_DIR = SKILL_DIR / "schemas"
SUPPORTED_SCHEMAS = frozenset(
    {"evidence-v1", "proposal-v1", "generator-response-v1"}
)
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
        raise ValidationError(_bounded_error(f"value is not canonical JSON: {error}")) from error


def canonical_bytes(value: Any) -> bytes:
    """Return the exact UTF-8 bytes used for persistence and accounting."""
    return canonical_json(value).encode("utf-8")


def load_schema(name: str) -> dict[str, Any]:
    """Load one allowlisted schema relative to this copy of the skill."""
    if name not in SUPPORTED_SCHEMAS:
        supported = ", ".join(sorted(SUPPORTED_SCHEMAS))
        raise ValidationError(
            _bounded_error(f"unsupported schema {name!r}; supported schemas: {supported}")
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


def _resolve_ref(schema_name: str, root: Mapping[str, Any], reference: str) -> Mapping[str, Any]:
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
        _fail(schema_name, path, f"unsupported value {value!r}; supported values: {schema['enum']}")

    expected_type = schema.get("type")
    if expected_type is not None:
        allowed_types = [expected_type] if isinstance(expected_type, str) else expected_type
        if not any(_matches_type(value, item) for item in allowed_types):
            _fail(schema_name, path, f"expected {expected_type}, got {type(value).__name__}")

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
            _fail(schema_name, path, f"contains {len(value)} fields; maximum is {schema['maxProperties']}")
        for key, child in value.items():
            child_schema = properties.get(key)
            if child_schema is not None:
                _validate_node(schema_name, root, child_schema, child, path + (key,))

    if isinstance(value, list):
        if "minItems" in schema and len(value) < schema["minItems"]:
            _fail(schema_name, path, f"contains {len(value)} items; minimum is {schema['minItems']}")
        if "maxItems" in schema and len(value) > schema["maxItems"]:
            _fail(schema_name, path, f"contains {len(value)} items; maximum is {schema['maxItems']}")
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
            _fail(schema_name, path, f"length {len(value)} is below minimum {schema['minLength']}")
        if "maxLength" in schema and len(value) > schema["maxLength"]:
            _fail(schema_name, path, f"length {len(value)} exceeds maximum {schema['maxLength']}")
        pattern = schema.get("pattern")
        if pattern is not None and re.search(pattern, value) is None:
            _fail(schema_name, path, "value does not match the required format")

    if isinstance(value, (int, float)) and not isinstance(value, bool):
        if "minimum" in schema and value < schema["minimum"]:
            _fail(schema_name, path, f"value {value} is below minimum {schema['minimum']}")
        if "maximum" in schema and value > schema["maximum"]:
            _fail(schema_name, path, f"value {value} exceeds maximum {schema['maximum']}")


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
            _bounded_error(f"invalid {field} value {value!r}; supported values: {supported}")
        )
    return normalized


def normalize_scope(value: str) -> str:
    return _normalize_identity("scope", value, SUPPORTED_SCOPES)


def normalize_category(value: str) -> str:
    return _normalize_identity("category", value, SUPPORTED_CATEGORIES)


def normalize_concrete_target(value: str) -> str:
    if not isinstance(value, str):
        raise ValidationError(f"concrete_target must be a string, got {type(value).__name__}")
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
        raise ValidationError(f"evidence reference must be a string, got {type(value).__name__}")
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
        raise ValidationError(f"fingerprint material is missing required fields: {', '.join(missing)}")
    return {
        "category": normalize_category(material["category"]),
        "concrete_target": normalize_concrete_target(material["concrete_target"]),
        "intended_outcome": normalize_text("intended_outcome", material["intended_outcome"]),
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
        raise ValidationError(f"measurement text must be a string, got {type(text).__name__}")
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
        category_bytes[category] = category_bytes.get(category, 0) + len(canonical_bytes(item))

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


def _validate_session_id(field: str, value: Any) -> str:
    if not isinstance(value, str):
        raise ValidationError(f"{field} must be a string, got {type(value).__name__}")
    if not value or value != value.strip():
        raise ValidationError(
            f"{field} must be a non-empty session id without surrounding whitespace"
        )
    if len(value) > MAX_SESSION_ID_LENGTH:
        raise ValidationError(f"{field} exceeds the {MAX_SESSION_ID_LENGTH} character limit")
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
            _bounded_error(f"session id {selected!r} is not in the supplied visible session ids")
        )
    return selected


def _fallback_evidence(visible_items: Sequence[Mapping[str, Any]]) -> dict[str, Any]:
    if not isinstance(visible_items, Sequence) or isinstance(visible_items, (str, bytes)):
        raise ValidationError("visible_items must be a sequence of allowlisted evidence objects")
    items: list[dict[str, Any]] = []
    for index, item in enumerate(visible_items):
        if not isinstance(item, Mapping):
            raise ValidationError(f"visible_items[{index}] must be an evidence object")
        items.append(dict(item))

    document = with_evidence_accounting(
        {
            "contract_version": "evidence-v1",
            "source": "fallback",
            "items": items,
        }
    )
    try:
        return validate_contract("evidence-v1", document)
    except ContractValidationError as error:
        raise ContractValidationError(
            _bounded_error(f"fallback evidence must contain only allowlisted visible items: {error}")
        ) from error


def prepare_feedback_invocation(
    *,
    requested_session_id: str | None,
    current_session_id: str | None,
    visible_session_ids: Sequence[str],
    visible_items: Sequence[Mapping[str, Any]],
    librarian_summary_path: str | Path | None,
) -> dict[str, Any]:
    """Select a visible session and build its deterministic first-run evidence bundle.

    This preparation step is pure apart from loading the copy-local evidence schema. It
    does not inspect the selected session, scan the filesystem, invoke a provider, or
    create feedback state. Librarian acquisition is added by its dedicated later task.
    """
    session_id = _select_visible_session(
        requested_session_id,
        current_session_id,
        visible_session_ids,
    )
    if librarian_summary_path is not None:
        raise ValidationError(
            "librarian_summary_path is unsupported during fallback acquisition; "
            "supply already-visible items instead"
        )
    return {
        "session_id": session_id,
        "evidence": _fallback_evidence(visible_items),
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
            raise ValidationError(f"proposal_locations[{index}] must be a non-empty path")
        if len(location) > 512 or any(
            ord(character) < 32 or ord(character) == 127 for character in location
        ):
            raise ValidationError(f"proposal_locations[{index}] is invalid or oversized")
        locations.append(location)

    if failure is not None:
        if locations:
            raise ValidationError("a failed review outcome cannot include persisted proposals")
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


__all__ = [
    "ContractValidationError",
    "ValidationError",
    "canonical_bytes",
    "canonical_json",
    "build_review_outcome",
    "evidence_accounting",
    "fingerprint_material",
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
    "prepare_feedback_invocation",
    "proposal_fingerprint",
    "validate_contract",
    "with_evidence_accounting",
]
