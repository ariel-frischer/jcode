# Session Feedback Fixtures

This directory contains the deterministic fixtures for the standalone
`session-feedback` skill. The complete skill tree is intentionally rooted at the
skill directory and must work unchanged from either location:

- `.jcode/skills/session-feedback/`
- `~/.jcode/skills/session-feedback/`

Fixture code and documentation must resolve sibling resources relative to the
skill directory. Do not embed either installation path, the repository root, a
specific home directory, or another machine-local absolute path.

## Fixture manifest

Every committed fixture must be:

- **Synthetic:** invented for this test suite and not copied from a real session.
- **Bounded:** limited to the smallest payload needed to exercise one contract.
- **Allowlisted:** composed only of fields and evidence categories permitted by
  the versioned session-feedback schemas.
- **Deterministic:** stable across runs, machines, time zones, and installation
  locations. Use fixed identifiers, timestamps, ordering, and expected hashes.

Fixtures must not contain:

- session transcripts or repeated startup instructions;
- raw patches, edit payloads, or repository file contents;
- images, binary data, or base64-encoded data;
- credentials, tokens, secrets, or realistic credential-shaped placeholders;
- bulk successful tool output.

Use short synthetic failure excerpts and minimal synthetic receipts only when a
test explicitly requires those allowlisted evidence forms. Any new fixture must
state the contract boundary it exercises and remain readable as plain UTF-8 text.

## Intended standalone layout

```text
session-feedback/
├── SKILL.md
├── session_feedback.py
├── schemas/
├── fixtures/
│   └── README.md
└── tests/
```

The skill remains a removable artifact. Fixtures must not require Cargo dependencies,
a production Rust surface, automatic hooks, or a `jcode feedback` command.
