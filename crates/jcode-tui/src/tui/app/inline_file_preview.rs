use super::*;

impl App {
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
        let candidate = std::path::Path::new(path_target);
        let path = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            let Some(working_dir) = self.session.working_dir.as_deref() else {
                return false;
            };
            std::path::Path::new(working_dir).join(candidate)
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

        const MAX_INLINE_FILE_BYTES: u64 = 512 * 1024;
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
