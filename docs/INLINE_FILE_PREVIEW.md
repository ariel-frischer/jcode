# Inline file previews

The TUI can render a clicked local text-file path directly beneath the message that contains it. Click the same path or the rendered preview body to collapse it. Preview text participates in normal chat scrolling and copy selection.

## Validate this feature

```bash
mkdir -p /tmp/jcode-inline-preview-demo
printf 'INLINE_PREVIEW_DEMO=works\n' > /tmp/jcode-inline-preview-demo/demo.txt
cd /tmp/jcode-inline-preview-demo
jcode
```

Ask the agent to `Reply with exactly: demo.txt`, then click `demo.txt` in the
response. The file content should appear directly below that message. Click the
path or preview again and it should collapse.

## Recognized targets

Clickable targets include `@path` mentions, URLs, and conservative plain file paths.
Existing repository Markdown links continue to open in the side panel. Prefix a
Markdown path with `@` when an inline preview is desired. Plain paths support:

- relative, parent-relative, absolute, and home-relative paths such as `docs/guide.md`, `../src/main.rs`, `/tmp/log.txt`, and `~/notes.md`;
- common extensionless project files such as `Makefile`, `Dockerfile`, and `README`;
- names that begin with digits, such as `2026-report.md`;
- Markdown anchors and source locations, such as `docs/guide.md#setup` and `src/main.rs:42:7`.

Ordinary dotted prose such as `self.input`, `foo.bar`, and `example.com` is not treated as a local file unless it has a supported file extension or path shape.

## Resolution and safety

Relative paths resolve against the session working directory, then the process working directory when the session has no working directory. `~/` resolves against the user home directory. Anchors and numeric line or column suffixes are preserved as click targets but removed before reading the underlying file.

A preview is local-only and does not send or mutate file contents. User-clicked absolute and home-relative paths may point outside the repository. Files larger than 512 KiB, binary or otherwise non-UTF-8 files, missing files, URLs, and mail links are rejected without opening a preview. Clearing the transcript also releases all retained preview content.
