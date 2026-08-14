#!/usr/bin/env python3
"""Thin noninteractive entry point for the standalone session-feedback skill."""

from __future__ import annotations

import argparse
import json
import sys
from typing import Any

from session_feedback import ValidationError, canonical_json, run_feedback


MAX_INPUT_BYTES = 256 * 1024
INPUT_FIELDS = frozenset(
    {"current_session_id", "visible_session_ids", "visible_items"}
)


def _read_input() -> dict[str, Any]:
    raw = sys.stdin.buffer.read(MAX_INPUT_BYTES + 1)
    if len(raw) > MAX_INPUT_BYTES:
        raise ValidationError(f"visible-evidence input exceeds the {MAX_INPUT_BYTES} byte limit")
    try:
        document = json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValidationError(f"visible-evidence input must be one UTF-8 JSON object: {error}") from error
    if not isinstance(document, dict):
        raise ValidationError("visible-evidence input must be one JSON object")

    unknown = sorted(set(document) - INPUT_FIELDS)
    if unknown:
        raise ValidationError(f"visible-evidence input contains unknown fields: {unknown}")
    missing = sorted({"visible_session_ids", "visible_items"} - set(document))
    if missing:
        raise ValidationError(f"visible-evidence input is missing required fields: {missing}")
    return document


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Run one bounded, review-only session-feedback invocation."
    )
    parser.add_argument("session_id", nargs="?", help="optional visible session id")
    args = parser.parse_args(argv)

    try:
        document = _read_input()
        result = run_feedback(
            requested_session_id=args.session_id,
            current_session_id=document.get("current_session_id"),
            visible_session_ids=document["visible_session_ids"],
            visible_items=document["visible_items"],
            librarian_summary_path=None,
        )
    except ValidationError as error:
        print(f"session-feedback: {error}", file=sys.stderr)
        return 2

    print(canonical_json(result))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
