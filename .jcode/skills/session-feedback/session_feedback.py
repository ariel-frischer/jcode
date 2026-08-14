#!/usr/bin/env python3
"""Dependency-free deterministic primitives for the session-feedback skill."""

from __future__ import annotations

import hashlib
import json
import math
import re
import subprocess
import time
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
MAX_RENDERED_PROPOSAL_BYTES = 65_536
MAX_LIBRARIAN_SUMMARY_BYTES = 262_144
MAX_SHORTLIST_TARGETS = 16
LIBRARIAN_SUMMARY_VERSION = "librarian-summary-v1"
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
        target_path = Path(
            normalized_target[2:]
            if normalized_target.startswith("~/")
            else normalized_target
        )
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


def _allowlist_evidence_items(
    items: Sequence[Mapping[str, Any]], *, source_label: str
) -> list[dict[str, Any]]:
    if not isinstance(items, Sequence) or isinstance(items, (str, bytes)):
        raise ValidationError(f"{source_label} must be a sequence of allowlisted evidence objects")
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
            _bounded_error(f"{source_label} must contain only valid allowlisted evidence: {error}")
        ) from error


def _fallback_evidence(visible_items: Sequence[Mapping[str, Any]]) -> dict[str, Any]:
    return _normalized_evidence(
        source="fallback",
        items=visible_items,
        source_label="fallback visible_items",
    )


def _read_librarian_summary(path_value: str | Path) -> Mapping[str, Any]:
    if not isinstance(path_value, (str, Path)):
        raise ValidationError("librarian_summary_path must be an explicitly supplied path")
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
    version = summary.get("summary_version")
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
    return _normalized_evidence(
        source="librarian",
        items=summary.get("items"),
        source_label="librarian summary items",
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


def _run_generation_command(
    command: Sequence[str], *, timeout_seconds: float, estimated_cost_usd: float
) -> dict[str, Any]:
    started = time.monotonic()
    try:
        completed = subprocess.run(
            list(command),
            text=True,
            capture_output=True,
            check=False,
            timeout=timeout_seconds,
        )
    except subprocess.TimeoutExpired as error:
        elapsed = time.monotonic() - started
        raise ValidationError(
            f"generation elapsed-time limit exceeded after {elapsed:.3f} seconds"
        ) from error
    return {
        "returncode": completed.returncode,
        "stdout": completed.stdout,
        "stderr": completed.stderr,
        "elapsed_seconds": time.monotonic() - started,
        # Schema mode does not expose provider pricing, so the default runner
        # records the configured cost reservation as its conservative estimate.
        "estimated_cost_usd": estimated_cost_usd,
    }


def _validated_runner_receipt(receipt: Any) -> tuple[str, float, float]:
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
    return stdout, float(elapsed), float(cost)


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
        if proposal["fingerprint"] != expected_fingerprint:
            raise ValidationError(
                f"generated proposal {index} fingerprint does not match normalized material"
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
    if effort not in {"none", "minimal", "low", "medium", "high", "xhigh", "max"}:
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

    schema_path = SCHEMA_DIR / "generator-response-v1.schema.json"
    request_document = canonical_json(request_input)
    prompt = (
        "Return only a generator-response-v1 review result for this bounded session "
        "feedback input. Use no tools and perform no side effects. Input: "
        + request_document
    )
    request_accounting = measure_text("request_input", prompt)
    if request_accounting["estimated_tokens"] > max_input_tokens:
        raise ValidationError("generation request input exceeds the token limit")
    command = [
        "jcode",
        "run",
        "--provider",
        "openai",
        "--model",
        model.strip(),
        "--reasoning-effort",
        effort,
        "--tool-profile",
        "none",
        "--max-turns",
        "1",
        "--schema",
        str(schema_path),
        prompt,
    ]

    invoke = runner
    if invoke is None:
        invoke = lambda value: _run_generation_command(  # noqa: E731
            value,
            timeout_seconds=max_elapsed,
            estimated_cost_usd=max_cost,
        )
    stdout, elapsed_seconds, estimated_cost_usd = _validated_runner_receipt(
        invoke(command)
    )
    if elapsed_seconds > max_elapsed:
        raise ValidationError("generation elapsed-time limit exceeded")
    if estimated_cost_usd > max_cost:
        raise ValidationError("generation estimated-cost limit exceeded")

    output_accounting = measure_text("request_output", stdout)
    if output_accounting["estimated_tokens"] > max_output_tokens:
        raise ValidationError("generation request output exceeds the token limit")
    response = parse_contract("generator-response-v1", stdout)
    proposals = _validate_generated_proposals(
        response,
        evidence=evidence,
        shortlisted_targets=shortlisted_targets,
    )
    if len(proposals) > max_proposals:
        raise ValidationError("generated proposal count exceeds the configured limit")
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
                "model": model.strip(),
                "effort": effort,
                "tool_profile": "none",
                "max_turns": 1,
                "schema": schema_path.name,
            },
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


def run_feedback(
    *,
    requested_session_id: str | None,
    current_session_id: str | None,
    visible_session_ids: Sequence[str],
    visible_items: Sequence[Mapping[str, Any]],
    librarian_summary_path: str | Path | None,
) -> dict[str, Any]:
    """Run the reusable deterministic portion of one session-feedback review.

    Later tasks extend this orchestrator with acquisition, generation, and persistence.
    Keeping the entry point here lets the slash skill and a future opt-in pre-close caller
    share one path without registering or enabling that caller.
    """
    invocation = prepare_feedback_invocation(
        requested_session_id=requested_session_id,
        current_session_id=current_session_id,
        visible_session_ids=visible_session_ids,
        visible_items=visible_items,
        librarian_summary_path=librarian_summary_path,
    )
    outcome = build_review_outcome(session_id=invocation["session_id"])
    evidence = invocation["evidence"]
    return {
        **outcome,
        "evidence_source": evidence["source"],
        "accounting": evidence["accounting"],
    }


__all__ = [
    "ContractValidationError",
    "ValidationError",
    "canonical_bytes",
    "canonical_json",
    "build_review_outcome",
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
    "prepare_feedback_invocation",
    "proposal_fingerprint",
    "render_review_proposal",
    "run_feedback",
    "validate_contract",
    "with_evidence_accounting",
]
