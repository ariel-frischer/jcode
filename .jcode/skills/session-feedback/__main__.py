#!/usr/bin/env python3
"""Thin noninteractive entry point for the standalone session-feedback skill."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Mapping, Sequence

SKILL_DIR = Path(__file__).resolve().parent
if str(SKILL_DIR) not in sys.path:
    sys.path.insert(0, str(SKILL_DIR))

from session_feedback import (
    ValidationError,
    bootstrap_feedback_store,
    canonical_json,
    persist_review_proposal,
    prepare_feedback_invocation,
    resolve_feedback_config,
    run_feedback,
)


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


def orchestrate_feedback(
    *,
    requested_session_id: str | None,
    current_session_id: str | None,
    visible_session_ids: Sequence[str],
    visible_items: Sequence[Mapping[str, Any]],
    librarian_summary_path: str | Path | None = None,
    target_root: str | Path = ".",
    feedback_root: str | Path | None = None,
    runner: Any | None = None,
    bd_runner: Any | None = None,
) -> dict[str, Any]:
    """Run one reusable, review-only feedback workflow."""
    root = (
        Path(feedback_root).expanduser()
        if feedback_root is not None
        else Path.home() / ".jcode" / "feedback"
    )
    resolved_config = resolve_feedback_config(config_path=root / "config.json")
    prepare_feedback_invocation(
        requested_session_id=requested_session_id,
        current_session_id=current_session_id,
        visible_session_ids=visible_session_ids,
        visible_items=visible_items,
        librarian_summary_path=librarian_summary_path,
    )
    bootstrap_feedback_store(feedback_root=root, bd_runner=bd_runner)

    result = run_feedback(
        requested_session_id=requested_session_id,
        current_session_id=current_session_id,
        visible_session_ids=visible_session_ids,
        visible_items=visible_items,
        librarian_summary_path=librarian_summary_path,
        target_root=target_root,
        effective_config=resolved_config["values"],
        runner=runner,
    )
    proposal_locations = []
    observed_at = datetime.now(UTC).isoformat()
    for proposal in result["proposals"]:
        occurrence_material = {
            "session_id": result["session_id"],
            "observed_at": observed_at,
            "evidence_references": proposal["evidence_references"],
            "fingerprint": proposal["fingerprint"],
        }
        evidence_digest = hashlib.sha256(
            canonical_json(occurrence_material).encode("utf-8")
        ).hexdigest()
        persistence = persist_review_proposal(
            proposal=proposal,
            evidence_occurrence={
                "occurrence_id": evidence_digest,
                "session_id": result["session_id"],
                "observed_at": observed_at,
                "evidence_references": proposal["evidence_references"],
                "evidence_digest": evidence_digest,
            },
            feedback_root=root,
            bd_runner=bd_runner,
        )
        proposal_locations.extend(
            persistence[path]
            for path in ("proposal_json", "proposal_markdown")
            if path in persistence
        )

    result["proposal_locations"] = proposal_locations
    return result


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Run one bounded, review-only session-feedback invocation."
    )
    parser.add_argument("session_id", nargs="?", help="optional visible session id")
    args = parser.parse_args(argv)

    try:
        document = _read_input()
        result = orchestrate_feedback(
            requested_session_id=args.session_id,
            current_session_id=document.get("current_session_id"),
            visible_session_ids=document["visible_session_ids"],
            visible_items=document["visible_items"],
            librarian_summary_path=None,
            target_root=Path.cwd(),
        )
    except ValidationError as error:
        print(f"session-feedback: {error}", file=sys.stderr)
        return 2

    print(canonical_json(result))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
