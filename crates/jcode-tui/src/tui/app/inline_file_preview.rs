use super::*;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use std::io::Read;

const MAX_INLINE_FILE_BYTES: u64 = 512 * 1024;

fn preview_path_target(target: &str) -> &str {
    let target = target.split(['#', '?']).next().unwrap_or(target);
    let Some((candidate, suffix)) = target.rsplit_once(':') else {
        return target;
    };
    if suffix.parse::<u64>().is_err() {
        return target;
    }

    let Some((path, line)) = candidate.rsplit_once(':') else {
        return candidate;
    };
    if line.parse::<u64>().is_ok() {
        path
    } else {
        candidate
    }
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
        let path_target = preview_path_target(target);
        if path_target.contains("://") || path_target.starts_with("mailto:") {
            return false;
        }
        let candidate = if let Some(home_relative) = path_target.strip_prefix("~/") {
            dirs::home_dir()
                .map(|home| home.join(home_relative))
                .unwrap_or_else(|| std::path::PathBuf::from(path_target))
        } else {
            std::path::PathBuf::from(path_target)
        };
        // Inline previews intentionally allow user-clicked local paths outside the
        // repository. Unlike repository Markdown navigation, this does not send or
        // mutate file contents; it only renders the selected local text in the TUI.
        let path = if candidate.is_absolute() {
            candidate
        } else {
            let working_dir = if let Some(working_dir) = self.session.working_dir.as_deref() {
                std::path::PathBuf::from(working_dir)
            } else {
                match std::env::current_dir() {
                    Ok(working_dir) => working_dir,
                    Err(error) => {
                        crate::logging::warn(&format!(
                            "Failed to resolve inline preview working directory: {error}"
                        ));
                        return false;
                    }
                }
            };
            working_dir.join(candidate)
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

        let file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(error) => {
                self.set_status_notice(format!("File is not readable text: {error}"));
                return true;
            }
        };
        let mut bytes = Vec::with_capacity((MAX_INLINE_FILE_BYTES + 1) as usize);
        if let Err(error) = file.take(MAX_INLINE_FILE_BYTES + 1).read_to_end(&mut bytes) {
            self.set_status_notice(format!("File is not readable text: {error}"));
            return true;
        }
        if bytes.len() as u64 > MAX_INLINE_FILE_BYTES {
            self.set_status_notice(format!(
                "File is too large for inline preview: {path_target}"
            ));
            return true;
        }
        let content = match String::from_utf8(bytes) {
            Ok(content) => content,
            Err(error) => {
                self.set_status_notice(format!("File is not readable text: {error}"));
                return true;
            }
        };
        if content.contains('\0') {
            self.set_status_notice("File is not readable text: binary content".to_string());
            return true;
        }
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
