---
name: session-librarian
description: Generate a bounded summary for the current session or one explicitly named persisted session.
allowed-tools: session_librarian
---

# Session Librarian

Invoke `session_librarian` exactly once, then report its result.

- If no argument is present, invoke it for the current session without a `session_id`.
- If a trailing persisted session identifier is present, preserve it exactly and pass it as `session_librarian.session_id`.
