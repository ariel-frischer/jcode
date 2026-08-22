# File Mentions

The TUI file picker discovers paths after `@` without blocking input. Selecting a
suggestion inserts the compact `@path` token. The visible and persisted transcript
keeps that compact form, while the provider-facing message receives readable file
contents at final dispatch.

## Use

1. Type `@` followed by part of a path.
2. Use the arrow keys to select a suggestion.
3. Press `Tab` to insert the selected path.
4. Send normally.

Queueing, retrieving, or persisting a pending prompt keeps the compact token.
Expansion happens only at the final local or remote dispatch boundary, so editing a
queued prompt does not expose an expanded payload.

## Validate this feature

```bash
mkdir -p /tmp/jcode-file-mention-demo
printf 'FILE_MENTION_DEMO=works\n' > /tmp/jcode-file-mention-demo/demo.txt
cd /tmp/jcode-file-mention-demo
jcode
```

In the TUI, type `Explain @demo.txt`, select `demo.txt`, press `Tab`, and send.
The visible prompt should keep the compact `@demo.txt` token, while the response
should identify `FILE_MENTION_DEMO=works` from the expanded provider context.

## Configuration

File mentions are enabled by default. Disable filesystem-backed suggestions and
expansion, or add gitignore-style exclusions, in `~/.jcode/config.toml`:

```toml
[file_mentions]
enabled = true
ignore = ["private/", "*.generated.*"]
```

Custom patterns are additive to the built-in exclusions for generated and
dependency directories.

## Expansion and limits

Existing UTF-8 files are wrapped as file context for the provider. Literal closing
`</file>` text inside a file is escaped so it cannot terminate the wrapper. Missing,
binary, unreadable, or oversized paths remain literal for the agent to handle.

The final expanded message is checked against the normal input-size limit as one
payload. If it is too large, submission is rejected and the compact prompt is
restored for editing.

## Remote sessions

Discovery and expansion use the connecting client's current working directory.
Only the expanded provider payload is sent to the remote agent. The transcript and
stored session history retain the compact `@path` form.
