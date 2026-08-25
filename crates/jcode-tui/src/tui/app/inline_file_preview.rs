use super::*;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use std::io::Read;

const MAX_INLINE_FILE_BYTES: u64 = 512 * 1024;

type InlineFilePreviewKey = (usize, u64);

#[derive(Default)]
pub(super) struct InlineFilePreviewState {
    pub(super) loaded: HashMap<InlineFilePreviewKey, crate::tui::InlineFilePreview>,
    pub(super) pending: HashMap<InlineFilePreviewKey, PendingInlineFilePreviewLoad>,
    pub(super) version: u64,
}

pub(super) struct PendingInlineFilePreviewLoad {
    display_messages_version: u64,
    display_path: String,
    receiver: std::sync::mpsc::Receiver<Result<crate::tui::InlineFilePreview, String>>,
}

pub(super) fn preview_path_target(target: &str) -> &str {
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

fn canonical_file_within(
    root: &std::path::Path,
    candidate: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let Ok(root) = root.canonicalize() else {
        return None;
    };
    let Ok(candidate) = candidate.canonicalize() else {
        return None;
    };
    (candidate.starts_with(&root) && candidate.is_file()).then_some(candidate)
}

// Inline previews intentionally allow explicitly clicked absolute and home-relative
// local paths outside the repository. Relative paths resolve only from the session
// working directory, except when the user explicitly supplies parent-relative components.
pub(super) fn resolve_local_file_target(
    target: &str,
    working_dir: Option<&std::path::Path>,
) -> Option<std::path::PathBuf> {
    let path_target = preview_path_target(target);
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

    let working_dir = if let Some(working_dir) = working_dir {
        working_dir.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(working_dir) => working_dir,
            Err(error) => {
                crate::logging::warn(&format!(
                    "Failed to resolve inline preview working directory: {error}"
                ));
                return None;
            }
        }
    };
    let local = working_dir.join(candidate);
    if candidate
        .components()
        .next()
        .is_some_and(|component| component == std::path::Component::ParentDir)
    {
        let Ok(local) = local.canonicalize() else {
            return None;
        };
        return local.is_file().then_some(local);
    }
    canonical_file_within(&working_dir, &local)
}

impl App {
    fn inline_file_preview_key(&self, message_index: usize) -> Option<InlineFilePreviewKey> {
        self.display_messages
            .get(message_index)
            .map(|message| (message_index, message.stable_cache_hash()))
    }

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
        let Some(key) = self.inline_file_preview_key(message_index) else {
            return false;
        };
        self.inline_file_preview_state.pending.remove(&key);
        if self.inline_file_preview_state.loaded.remove(&key).is_none() {
            return false;
        }
        self.inline_file_preview_state.version =
            self.inline_file_preview_state.version.wrapping_add(1);
        self.set_status_notice("Collapsed inline file preview");
        true
    }

    pub(super) fn try_toggle_inline_file_preview(
        &mut self,
        target: &str,
        message_index: usize,
    ) -> bool {
        let display_path = preview_path_target(target).to_string();
        if display_path.contains("://") || display_path.starts_with("mailto:") {
            return false;
        }
        let working_dir = self
            .session
            .working_dir
            .clone()
            .map(std::path::PathBuf::from);
        let target = target.to_string();
        let display_path_for_load = display_path.clone();
        self.start_inline_file_preview_load_with(display_path, message_index, move || {
            let path = resolve_local_file_target(&target, working_dir.as_deref())
                .ok_or_else(|| format!("File is not available: {display_path_for_load}"))?;
            read_inline_file_preview(path, display_path_for_load)
        })
    }

    pub(super) fn start_inline_file_preview_load_with(
        &mut self,
        display_path: String,
        message_index: usize,
        load: impl FnOnce() -> Result<crate::tui::InlineFilePreview, String> + Send + 'static,
    ) -> bool {
        self.start_inline_file_preview_load(display_path, message_index, load)
    }

    fn start_inline_file_preview_load(
        &mut self,
        display_path: String,
        message_index: usize,
        load: impl FnOnce() -> Result<crate::tui::InlineFilePreview, String> + Send + 'static,
    ) -> bool {
        let Some(key) = self.inline_file_preview_key(message_index) else {
            return false;
        };
        if self
            .inline_file_preview_state
            .loaded
            .get(&key)
            .is_some_and(|preview| preview.display_path == display_path)
        {
            self.inline_file_preview_state.loaded.remove(&key);
            self.inline_file_preview_state.pending.remove(&key);
            self.inline_file_preview_state.version =
                self.inline_file_preview_state.version.wrapping_add(1);
            self.set_status_notice(format!("Collapsed file: {display_path}"));
            return true;
        }
        if self
            .inline_file_preview_state
            .pending
            .get(&key)
            .is_some_and(|pending| pending.display_path == display_path)
        {
            self.inline_file_preview_state.pending.remove(&key);
            self.set_status_notice(format!("Cancelled file preview: {display_path}"));
            return true;
        }
        self.inline_file_preview_state.pending.remove(&key);

        let display_messages_version = self.display_messages_version;
        let (sender, receiver) = std::sync::mpsc::channel();
        let task = move || {
            if sender.send(load()).is_err() {
                crate::logging::warn("Inline file preview result receiver was dropped");
            }
        };
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn_blocking(task);
        } else {
            std::thread::spawn(task);
        }
        self.inline_file_preview_state.pending.insert(
            key,
            PendingInlineFilePreviewLoad {
                display_messages_version,
                display_path: display_path.clone(),
                receiver,
            },
        );
        self.set_status_notice(format!("Loading file: {display_path}"));
        true
    }

    pub(super) fn poll_inline_file_preview_loads(&mut self) -> bool {
        let keys = self
            .inline_file_preview_state
            .pending
            .keys()
            .copied()
            .collect::<Vec<_>>();
        let mut changed = false;
        for key in keys {
            let received = match self
                .inline_file_preview_state
                .pending
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
            let Some(pending) = self.inline_file_preview_state.pending.remove(&key) else {
                continue;
            };
            changed = true;
            let message_is_current = pending.display_messages_version
                == self.display_messages_version
                && self
                    .display_messages
                    .get(key.0)
                    .is_some_and(|message| message.stable_cache_hash() == key.1);
            if !message_is_current {
                continue;
            }
            match result {
                Ok(preview) if preview.display_path == pending.display_path => {
                    let display_path = preview.display_path.clone();
                    self.inline_file_preview_state.loaded.insert(key, preview);
                    self.inline_file_preview_state.version =
                        self.inline_file_preview_state.version.wrapping_add(1);
                    self.set_status_notice(format!("Expanded file: {display_path}"));
                }
                Ok(_) => {}
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
    let file =
        std::fs::File::open(&path).map_err(|_| format!("File is not available: {display_path}"))?;
    if !file
        .metadata()
        .map_err(|_| format!("File is not available: {display_path}"))?
        .is_file()
    {
        return Err(format!("File is not available: {display_path}"));
    }
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

#[cfg(test)]
mod tests {
    use super::{preview_path_target, resolve_local_file_target};

    #[test]
    fn preview_path_target_strips_numeric_locations_from_the_right() {
        assert_eq!(preview_path_target("src/main.rs:42"), "src/main.rs");
        assert_eq!(preview_path_target("src/main.rs:42:7"), "src/main.rs");
        assert_eq!(
            preview_path_target("C:\\repo\\main.rs:42:7"),
            "C:\\repo\\main.rs"
        );
        assert_eq!(preview_path_target("notes:latest.md"), "notes:latest.md");
    }

    #[test]
    fn relative_resolution_does_not_search_sibling_directories() {
        let root = tempfile::tempdir().expect("workspace");
        let current = root.path().join("current");
        let sibling = root.path().join("sibling");
        std::fs::create_dir_all(current.join(".git")).expect("current repo");
        std::fs::create_dir_all(sibling.join(".git")).expect("sibling repo");
        std::fs::create_dir_all(sibling.join("docs")).expect("docs");
        std::fs::write(sibling.join("docs/guide.md"), "# Guide").expect("guide");

        assert_eq!(
            resolve_local_file_target("docs/guide.md", Some(&current)),
            None
        );
    }

    #[test]
    fn explicit_parent_relative_path_keeps_existing_local_preview_behavior() {
        let root = tempfile::tempdir().expect("workspace");
        let current = root.path().join("current");
        let sibling = root.path().join("sibling");
        std::fs::create_dir_all(&current).expect("current directory");
        std::fs::create_dir_all(sibling.join("docs")).expect("sibling docs");
        std::fs::write(sibling.join("docs/guide.md"), "# Guide").expect("guide");

        assert_eq!(
            resolve_local_file_target("../sibling/docs/guide.md", Some(&current)),
            Some(
                sibling
                    .join("docs/guide.md")
                    .canonicalize()
                    .expect("canonical parent-relative guide")
            )
        );
    }
}
