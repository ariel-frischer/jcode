use super::*;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

const MAX_INLINE_FILE_BYTES: u64 = 512 * 1024;

// Inline previews intentionally allow user-clicked local paths outside the
// repository. This only renders selected local text and never sends or mutates it.
pub(super) fn resolve_local_file_target(
    target: &str,
    working_dir: Option<&std::path::Path>,
) -> Option<std::path::PathBuf> {
    let path_target = target.split(['#', '?']).next().unwrap_or(target);
    if path_target.contains("://") || path_target.starts_with("mailto:") {
        return None;
    }

    if path_target == "~" {
        return dirs::home_dir();
    }
    if let Some(rest) = path_target
        .strip_prefix("~/")
        .or_else(|| path_target.strip_prefix("~\\"))
    {
        return dirs::home_dir().map(|home| home.join(rest));
    }

    let candidate = std::path::Path::new(path_target);
    if candidate.is_absolute() {
        return Some(candidate.to_path_buf());
    }

    working_dir
        .map(std::path::Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .map(|directory| directory.join(candidate))
}

impl App {
    pub(super) fn try_collapse_inline_file_preview_at(&mut self, mouse: MouseEvent) -> bool {
        if !matches!(mouse.kind, MouseEventKind::Up(MouseButton::Left))
            || self.copy_selection_mode
            || self.copy_selection_dragging
        {
            return false;
        }
        let Some(anchor) = self.copy_selection_pending_anchor else {
            return false;
        };
        if anchor.pane != crate::tui::CopySelectionPane::Chat
            || crate::tui::ui::copy_point_from_screen(mouse.column, mouse.row) != Some(anchor)
        {
            return false;
        }
        let Some(message_index) =
            crate::tui::ui::chat_inline_file_preview_message_from_screen(mouse.column, mouse.row)
        else {
            return false;
        };
        self.copy_selection_pending_anchor = None;
        self.copy_selection_edge_autoscroll = None;
        self.try_collapse_inline_file_preview(message_index)
    }

    pub(super) fn try_collapse_inline_file_preview(&mut self, message_index: usize) -> bool {
        let Some(message_hash) = self
            .display_messages
            .get(message_index)
            .map(DisplayMessage::stable_cache_hash)
        else {
            return false;
        };
        if self.inline_file_previews.remove(&message_hash).is_none() {
            return false;
        }
        self.inline_file_previews_version = self.inline_file_previews_version.wrapping_add(1);
        self.set_status_notice("Collapsed inline file preview");
        true
    }

    pub(super) fn try_toggle_inline_file_preview(
        &mut self,
        target: &str,
        message_index: usize,
    ) -> bool {
        let path_target = target.split(['#', '?']).next().unwrap_or(target);
        if path_target.contains("://") || path_target.starts_with("mailto:") {
            return false;
        }
        let Some(path) = resolve_local_file_target(
            path_target,
            self.session
                .working_dir
                .as_deref()
                .map(std::path::Path::new),
        ) else {
            return false;
        };
        if !path.is_file() {
            return false;
        }

        let Some(message_hash) = self
            .display_messages
            .get(message_index)
            .map(DisplayMessage::stable_cache_hash)
        else {
            return false;
        };
        if self
            .inline_file_previews
            .get(&message_hash)
            .is_some_and(|preview| preview.display_path == path_target)
        {
            self.inline_file_previews.remove(&message_hash);
            self.inline_file_previews_version = self.inline_file_previews_version.wrapping_add(1);
            self.set_status_notice(format!("Collapsed file: {path_target}"));
            return true;
        }

        let metadata = match std::fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                self.set_status_notice(format!("Failed to inspect file: {error}"));
                return true;
            }
        };
        if metadata.len() > MAX_INLINE_FILE_BYTES {
            self.set_status_notice(format!(
                "File is too large for inline preview: {path_target}"
            ));
            return true;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) => {
                self.set_status_notice(format!("File is not readable text: {error}"));
                return true;
            }
        };
        let markdown = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "md" | "mdx" | "markdown"
                )
            });
        self.inline_file_previews.insert(
            message_hash,
            crate::tui::InlineFilePreview {
                display_path: path_target.to_string(),
                content,
                markdown,
            },
        );
        self.inline_file_previews_version = self.inline_file_previews_version.wrapping_add(1);
        self.set_status_notice(format!("Expanded file: {path_target}"));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_local_file_target;

    #[test]
    fn resolve_local_file_target_expands_home_and_session_relative_paths() {
        let home = dirs::home_dir().expect("test home directory");
        let expanded = resolve_local_file_target("~/.jcode/report.html", None)
            .expect("tilde path should resolve");
        assert_eq!(expanded, home.join(".jcode/report.html"));

        let repository = tempfile::tempdir().expect("repository tempdir");
        let relative = resolve_local_file_target("docs/report.html", Some(repository.path()))
            .expect("relative path should resolve against session directory");
        assert_eq!(relative, repository.path().join("docs/report.html"));
    }
}
