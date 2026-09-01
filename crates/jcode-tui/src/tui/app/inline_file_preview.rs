use super::*;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use std::io::Read;

const MAX_INLINE_FILE_BYTES: u64 = 512 * 1024;

pub(super) struct PendingInlineFilePreviewLoad {
    display_messages_version: u64,
    display_path: String,
    receiver: std::sync::mpsc::Receiver<Result<crate::tui::InlineFilePreview, String>>,
}

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
        let key = crate::tui::InlineFilePreviewKey {
            message_index,
            message_hash,
        };
        if self.inline_file_previews.remove(&key).is_none() {
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
        let display_path = preview_path_target(target).to_string();
        self.start_inline_file_preview_load_with(target, message_index, move |path| {
            read_inline_file_preview(path, display_path)
        })
    }

    pub(super) fn start_inline_file_preview_load_with(
        &mut self,
        target: &str,
        message_index: usize,
        load: impl FnOnce(std::path::PathBuf) -> Result<crate::tui::InlineFilePreview, String>
        + Send
        + 'static,
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
        let Some(message_hash) = self
            .display_messages
            .get(message_index)
            .map(DisplayMessage::stable_cache_hash)
        else {
            return false;
        };
        let key = crate::tui::InlineFilePreviewKey {
            message_index,
            message_hash,
        };
        if self
            .inline_file_previews
            .get(&key)
            .is_some_and(|preview| preview.display_path == path_target)
        {
            self.inline_file_previews.remove(&key);
            self.inline_file_previews_version = self.inline_file_previews_version.wrapping_add(1);
            self.set_status_notice(format!("Collapsed file: {path_target}"));
            return true;
        }

        if self
            .pending_inline_file_preview_loads
            .get(&key)
            .is_some_and(|pending| pending.display_path == path_target)
        {
            self.pending_inline_file_preview_loads.remove(&key);
            self.set_status_notice(format!("Cancelled file preview: {path_target}"));
            return true;
        }
        self.pending_inline_file_preview_loads.remove(&key);

        let display_messages_version = self.display_messages_version;
        let (sender, receiver) = std::sync::mpsc::channel();
        let task = move || {
            let _ = sender.send(load(path));
        };
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn_blocking(task);
        } else {
            std::thread::spawn(task);
        }
        self.pending_inline_file_preview_loads.insert(
            key,
            PendingInlineFilePreviewLoad {
                display_messages_version,
                display_path: path_target.to_string(),
                receiver,
            },
        );
        self.set_status_notice(format!("Loading file: {path_target}"));
        true
    }

    pub(super) fn poll_inline_file_preview_loads(&mut self) -> bool {
        let keys = self
            .pending_inline_file_preview_loads
            .keys()
            .copied()
            .collect::<Vec<_>>();
        let mut changed = false;

        for key in keys {
            let received = match self
                .pending_inline_file_preview_loads
                .get(&key)
                .map(|pending| pending.receiver.try_recv())
            {
                Some(Ok(result)) => Some(result),
                Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => Some(Err(
                    "File preview load failed: background worker stopped".to_string(),
                )),
                Some(Err(std::sync::mpsc::TryRecvError::Empty)) | None => None,
            };
            let Some(result) = received else {
                continue;
            };
            let Some(pending) = self.pending_inline_file_preview_loads.remove(&key) else {
                continue;
            };
            changed = true;

            let message_is_current = pending.display_messages_version
                == self.display_messages_version
                && self
                    .display_messages
                    .get(key.message_index)
                    .is_some_and(|message| message.stable_cache_hash() == key.message_hash);
            if !message_is_current {
                continue;
            }

            match result {
                Ok(preview) => {
                    let display_path = preview.display_path.clone();
                    self.inline_file_previews.insert(key, preview);
                    self.inline_file_previews_version =
                        self.inline_file_previews_version.wrapping_add(1);
                    self.set_status_notice(format!("Expanded file: {display_path}"));
                }
                Err(error) => self.set_status_notice(error),
            }
        }

        changed
    }
}

fn read_inline_file_preview(
    path: std::path::PathBuf,
    display_path: String,
) -> Result<crate::tui::InlineFilePreview, String> {
    let metadata =
        std::fs::metadata(&path).map_err(|_| format!("File is not available: {display_path}"))?;
    if !metadata.is_file() {
        return Err(format!("File is not available: {display_path}"));
    }

    let file = std::fs::File::open(&path)
        .map_err(|error| format!("File is not readable text: {error}"))?;
    let mut bytes = Vec::with_capacity((MAX_INLINE_FILE_BYTES + 1) as usize);
    file.take(MAX_INLINE_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("File is not readable text: {error}"))?;
    if bytes.len() as u64 > MAX_INLINE_FILE_BYTES {
        return Err(format!(
            "File is too large for inline preview: {display_path}"
        ));
    }
    let content =
        String::from_utf8(bytes).map_err(|error| format!("File is not readable text: {error}"))?;
    if content.contains('\0') {
        return Err("File is not readable text: binary content".to_string());
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
    Ok(crate::tui::InlineFilePreview {
        display_path,
        content,
        markdown,
    })
}
