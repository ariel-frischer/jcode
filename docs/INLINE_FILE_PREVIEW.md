# Inline file previews

The TUI renders a clicked local text-file path directly beneath the message that contains it. Click the same path or the rendered preview body to collapse it. Preview text participates in normal chat scrolling and copy selection.

## Recognized targets

Clickable targets include `@path` mentions, Markdown links, and conservative plain file paths. Quoted and plain Markdown paths open inline without requiring an `@` prefix. Supported forms include:

- relative, parent-relative, absolute, and home-relative paths such as `docs/guide.md`, `../src/main.rs`, `/tmp/log.txt`, and `~/notes.md`;
- common extensionless project files such as `Makefile`, `Dockerfile`, and `README`;
- Markdown anchors and source locations such as `docs/guide.md#setup` and `src/main.rs:42:7`.

Ordinary dotted prose such as `self.input`, `foo.bar`, and `example.com` is not treated as a local file unless it has a supported file extension or path shape.

## Resolution and safety

Relative paths resolve only against the session working directory. Jcode does not search parent directories, sibling repositories, or other workspaces for a missing relative path. To reference a file elsewhere, use an explicit parent-relative, absolute, or home-relative path.

When a session has no working directory, relative paths use the process working directory. `~/` resolves against the user home directory. Anchors and numeric line or column suffixes are removed before reading the underlying file.

Preview loading is local-only and does not send or mutate file contents. User-clicked absolute and home-relative paths may point outside the repository. Resolution and bounded reads run in background work so slow filesystems do not block TUI input. Completed reads are discarded when their originating message changed.

Files larger than 512 KiB, binary or non-UTF-8 files, missing files, URLs, and mail links are rejected without opening a preview. Clearing the transcript releases loaded and pending preview state.

HTML files continue to follow the configured `display.html_file_open` mode.
