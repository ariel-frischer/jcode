use super::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

const FILE_MENTION_BATCH_SIZE: usize = 32;
const FILE_MENTION_MAX_MATCHES: usize = 5000;
const FILE_MENTION_POLL_BATCHES: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileMentionRequest {
    pub(super) root: PathBuf,
    pub(super) query: String,
    pub(super) ignore_patterns: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileMentionCandidate {
    pub(super) score: i32,
    pub(super) path: String,
    pub(super) is_dir: bool,
}

#[derive(Debug)]
pub(super) struct FileMentionBatch {
    pub generation: u64,
    pub candidates: Vec<FileMentionCandidate>,
    pub done: bool,
}

#[derive(Debug)]
pub(crate) struct FileMentionDiscovery {
    pub request: FileMentionRequest,
    pub generation: u64,
    pub receiver: mpsc::Receiver<FileMentionBatch>,
    pub cancel: Arc<AtomicBool>,
    pub candidates: Vec<FileMentionCandidate>,
}

#[cfg(test)]
fn discover_file_mentions(
    root: &Path,
    query: &str,
    ignore_patterns: &[String],
) -> Vec<(i32, String, bool)> {
    let (sender, receiver) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(true));
    discover_file_mentions_batched(root, query, ignore_patterns, 0, &cancel, &sender);
    drop(sender);
    receiver
        .into_iter()
        .flat_map(|batch| {
            batch
                .candidates
                .into_iter()
                .map(|candidate| (candidate.score, candidate.path, candidate.is_dir))
        })
        .collect()
}

fn start_file_mention_discovery(
    root: PathBuf,
    query: String,
    ignore_patterns: Vec<String>,
    generation: u64,
) -> (mpsc::Receiver<FileMentionBatch>, Arc<AtomicBool>) {
    let (sender, receiver) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(true));
    let worker_cancel = Arc::clone(&cancel);
    std::thread::spawn(move || {
        discover_file_mentions_batched(
            &root,
            &query,
            &ignore_patterns,
            generation,
            &worker_cancel,
            &sender,
        );
    });
    (receiver, cancel)
}

#[expect(
    clippy::too_many_arguments,
    reason = "Entry processing keeps the bounded discovery state explicit"
)]
fn process_file_mention_entry(
    root: &Path,
    query: &str,
    ignored_patterns: &[&str],
    entry: ignore::DirEntry,
    cancel: &AtomicBool,
    sender: &mpsc::Sender<FileMentionBatch>,
    generation: u64,
    batch: &mut Vec<FileMentionCandidate>,
    match_count: &mut usize,
) -> bool {
    if !cancel.load(Ordering::Relaxed) {
        return false;
    }
    let path = entry.path();
    if path == root {
        return true;
    }
    let Ok(relative) = path.strip_prefix(root) else {
        return true;
    };
    let text = relative
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/");
    if ignored_patterns.iter().any(|pattern| {
        let pattern = pattern.trim().trim_start_matches("./");
        let pattern = pattern.trim_end_matches('/');
        text.split('/').any(|component| component == pattern)
            || text == pattern
            || text.starts_with(&format!("{pattern}/"))
    }) {
        return true;
    }
    let score = if query.is_empty() {
        Some(0)
    } else {
        jcode_fuzzy::fuzzy_score(query, &text)
    };
    if let Some(score) = score {
        batch.push(FileMentionCandidate {
            score,
            path: text,
            is_dir: entry.file_type().is_some_and(|t| t.is_dir()),
        });
        *match_count += 1;
    }
    if batch.len() >= FILE_MENTION_BATCH_SIZE {
        return send_file_mention_batch(sender, generation, batch, false);
    }
    true
}

/// Test-only entry point for deterministic batch and responsiveness checks.
#[cfg(test)]
pub(super) fn start_file_mention_discovery_for_test(
    root: PathBuf,
    query: String,
    ignore_patterns: Vec<String>,
    generation: u64,
) -> (mpsc::Receiver<FileMentionBatch>, Arc<AtomicBool>) {
    start_file_mention_discovery(root, query, ignore_patterns, generation)
}

fn discover_file_mentions_batched(
    root: &Path,
    query: &str,
    ignore_patterns: &[String],
    generation: u64,
    cancel: &Arc<AtomicBool>,
    sender: &mpsc::Sender<FileMentionBatch>,
) {
    const BUILTIN_IGNORE_PATTERNS: &[&str] = &[
        "node_modules/",
        "target/",
        "vendor/",
        ".venv/",
        "venv/",
        "__pycache__/",
        ".pytest_cache/",
        ".mypy_cache/",
        ".ruff_cache/",
        ".tox/",
        ".nox/",
        "dist/",
        "build/",
        "out/",
        "coverage/",
        ".cache/",
        ".next/",
        ".nuxt/",
        ".svelte-kit/",
        ".turbo/",
        ".gradle/",
        ".terraform/",
        ".git/",
    ];
    let ignored_patterns: Vec<&str> = BUILTIN_IGNORE_PATTERNS
        .iter()
        .copied()
        .chain(ignore_patterns.iter().map(String::as_str))
        .collect();
    let mut batch = Vec::with_capacity(FILE_MENTION_BATCH_SIZE);
    let mut match_count = 0;
    let mut stopped = false;

    // Emit direct children first. A depth-first walk can otherwise fill the
    // first bounded batches with a large nested directory and hide files that
    // are immediately in the user's working directory.
    let mut direct_builder = ignore::WalkBuilder::new(root);
    direct_builder
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .max_depth(Some(1));
    for entry in direct_builder.build().filter_map(Result::ok) {
        if !process_file_mention_entry(
            root,
            query,
            &ignored_patterns,
            entry,
            cancel,
            sender,
            generation,
            &mut batch,
            &mut match_count,
        ) {
            stopped = true;
            break;
        }
        if match_count >= FILE_MENTION_MAX_MATCHES {
            break;
        }
    }

    if !stopped && match_count < FILE_MENTION_MAX_MATCHES {
        let mut recursive_builder = ignore::WalkBuilder::new(root);
        recursive_builder
            .hidden(false)
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false);
        for entry in recursive_builder.build().filter_map(Result::ok) {
            if entry.depth() <= 1 {
                continue;
            }
            if !process_file_mention_entry(
                root,
                query,
                &ignored_patterns,
                entry,
                cancel,
                sender,
                generation,
                &mut batch,
                &mut match_count,
            ) {
                stopped = true;
                break;
            }
            if match_count >= FILE_MENTION_MAX_MATCHES {
                break;
            }
        }
    }

    if stopped {
        return;
    }
    if !batch.is_empty() || match_count == 0 {
        send_file_mention_batch(sender, generation, &mut batch, true);
    } else if sender
        .send(FileMentionBatch {
            generation,
            candidates: Vec::new(),
            done: true,
        })
        .is_err()
    {
        return;
    }
}

fn send_file_mention_batch(
    sender: &mpsc::Sender<FileMentionBatch>,
    generation: u64,
    candidates: &mut Vec<FileMentionCandidate>,
    done: bool,
) -> bool {
    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.path.cmp(&right.path))
    });
    let candidates = std::mem::take(candidates);
    sender
        .send(FileMentionBatch {
            generation,
            candidates,
            done,
        })
        .is_ok()
}

impl App {
    pub(super) fn poll_file_mention_discovery(&mut self) -> bool {
        let mut changed = false;
        let mut pending = self.file_mention_discovery.borrow_mut();
        let Some(discovery) = pending.as_mut() else {
            return false;
        };
        for _ in 0..FILE_MENTION_POLL_BATCHES {
            let batch = match discovery.receiver.try_recv() {
                Ok(batch) => batch,
                Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => break,
            };
            if batch.generation != discovery.generation {
                continue;
            }
            discovery.candidates.extend(batch.candidates);
            changed = true;
            if batch.done {
                break;
            }
        }
        if changed {
            *self.command_suggestions_cache.borrow_mut() = None;
        }
        changed
    }

    pub(super) fn clear_file_mention_discovery(&self) {
        if let Some(discovery) = self.file_mention_discovery.borrow_mut().take() {
            discovery.cancel.store(false, Ordering::Relaxed);
        }
    }

    fn ensure_file_mention_discovery(&self, request: FileMentionRequest) {
        if self
            .file_mention_discovery
            .borrow()
            .as_ref()
            .is_some_and(|discovery| discovery.request == request)
        {
            return;
        }

        let generation = self.file_mention_generation.get().wrapping_add(1);
        self.file_mention_generation.set(generation);
        if let Some(discovery) = self.file_mention_discovery.borrow_mut().take() {
            discovery.cancel.store(false, Ordering::Relaxed);
        }
        let (receiver, cancel) = start_file_mention_discovery(
            request.root.clone(),
            request.query.clone(),
            request.ignore_patterns.clone(),
            generation,
        );
        *self.file_mention_discovery.borrow_mut() = Some(FileMentionDiscovery {
            request,
            generation,
            receiver,
            cancel,
            candidates: Vec::new(),
        });
    }
}

fn effective_file_mention_ignores() -> Vec<String> {
    crate::config::config().file_mentions.ignore.clone()
}

impl App {
    pub(super) fn file_mention_suggestions(&self, input: &str) -> Vec<(String, &'static str)> {
        let cursor = self.cursor_pos.min(input.len());
        let before_cursor = &input[..cursor];
        let Some(at) = before_cursor.rfind('@') else {
            self.clear_file_mention_discovery();
            return Vec::new();
        };
        if before_cursor[at + 1..].contains(char::is_whitespace) {
            self.clear_file_mention_discovery();
            return Vec::new();
        }
        let query = &before_cursor[at + 1..];
        let launch_cwd;
        let root = if let Some(working_dir) = self.session.working_dir.as_deref() {
            Path::new(working_dir)
        } else {
            launch_cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            &launch_cwd
        };
        let request = FileMentionRequest {
            root: root.to_path_buf(),
            query: query.to_owned(),
            ignore_patterns: effective_file_mention_ignores(),
        };
        self.ensure_file_mention_discovery(request.clone());
        let pending = self.file_mention_discovery.borrow();
        let Some(discovery) = pending
            .as_ref()
            .filter(|discovery| discovery.request == request)
        else {
            return Vec::new();
        };
        discovery
            .candidates
            .iter()
            .take(100)
            .map(|candidate| {
                let path = &candidate.path;
                let dir = candidate.is_dir;
                let mut replacement = input[..at].to_string();
                replacement.push('@');
                replacement.push_str(path);
                if dir {
                    replacement.push('/');
                }
                replacement.push_str(&input[cursor..]);
                (replacement, if dir { "Directory" } else { "File" })
            })
            .collect()
    }
}

/// Expand repository-local `@path` references before sending a prompt.
///
/// The picker only changes the text in the composer. The provider must receive
/// the referenced contents too, matching Claude Code's accepted file-reference
/// behavior. Unresolved references are intentionally preserved:
/// `@someone` and prose containing `@` are not file errors.
pub(super) fn expand_file_mentions(
    input: &str,
    working_dir: Option<&str>,
    enabled: bool,
) -> String {
    let Some(working_dir) = working_dir.filter(|_| enabled) else {
        return input.to_owned();
    };
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while cursor < input.len() {
        let Some(relative_at) = input[cursor..].find('@') else {
            output.push_str(&input[cursor..]);
            break;
        };
        let at = cursor + relative_at;
        output.push_str(&input[cursor..at]);

        // An @ embedded in an identifier or email address is not a file
        // reference. A file mention starts at the beginning or after whitespace.
        let valid_start = at == 0
            || input[..at]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        let end = input[at + 1..]
            .find(char::is_whitespace)
            .map_or(input.len(), |offset| at + 1 + offset);
        let mention = &input[at + 1..end];
        if !valid_start || mention.is_empty() {
            output.push('@');
            cursor = at + 1;
            continue;
        }

        let path = PathBuf::from(mention);
        let resolved = if path.is_absolute() {
            path
        } else {
            PathBuf::from(working_dir).join(path)
        };
        let replacement = match resolved.metadata() {
            Ok(metadata)
                if metadata.is_file()
                    && metadata.len() <= super::input::MAX_SUBMITTED_TEXT_BYTES as u64 =>
            {
                match std::fs::read_to_string(&resolved) {
                    Ok(contents) => Some(contents),
                    Err(_) => None,
                }
            }
            Ok(_) | Err(_) => None,
        }
        .map(|contents| {
            let escaped_path = mention
                .replace('&', "&amp;")
                .replace('"', "&quot;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            format!("<file path=\"{escaped_path}\">\n{contents}\n</file>")
        })
        .filter(|replacement| {
            output.len() + replacement.len() + input.len().saturating_sub(end)
                <= super::input::MAX_SUBMITTED_TEXT_BYTES
        });
        if let Some(replacement) = replacement {
            output.push_str(&replacement);
        } else {
            output.push_str(&input[at..end]);
        }
        cursor = end;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_mentions_skip_vendor_directories_and_honor_custom_patterns() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(temp.path().join("src")).expect("src");
        std::fs::create_dir_all(temp.path().join("node_modules/pkg")).expect("node_modules");
        std::fs::create_dir_all(temp.path().join("generated")).expect("generated");
        std::fs::write(temp.path().join("src/main.rs"), "fn main() {}").expect("source");
        std::fs::write(temp.path().join("node_modules/pkg/index.js"), "").expect("vendor");
        std::fs::write(temp.path().join("generated/api.rs"), "").expect("generated file");

        let defaults = vec!["node_modules/".into()];
        let paths = discover_file_mentions(temp.path(), "", &defaults);
        let names: Vec<_> = paths.iter().map(|(_, path, _)| path.as_str()).collect();
        assert!(names.contains(&"src"), "discovered names: {names:?}");
        assert!(names.contains(&"src/main.rs"));
        assert!(!names.iter().any(|path| path.starts_with("node_modules/")));

        let custom = vec!["node_modules/".into(), "generated/".into()];
        let paths = discover_file_mentions(temp.path(), "", &custom);
        assert!(
            !paths
                .iter()
                .any(|(_, path, _)| path.starts_with("generated/"))
        );
    }
    #[test]
    fn file_mentions_expand_relative_paths_against_working_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("docs")).unwrap();
        std::fs::write(dir.path().join("docs/boundaries.md"), "# boundaries\n").unwrap();

        let expanded = expand_file_mentions(
            "Please inspect @docs/boundaries.md",
            Some(dir.path().to_str().unwrap()),
            true,
        );

        assert_eq!(
            expanded,
            "Please inspect <file path=\"docs/boundaries.md\">\n# boundaries\n\n</file>"
        );
    }

    #[test]
    fn file_mentions_preserve_unresolved_and_embedded_at_signs() {
        let dir = tempfile::tempdir().unwrap();
        let input = "email me@example.com about @missing.md";

        assert_eq!(
            expand_file_mentions(input, Some(dir.path().to_str().unwrap()), true),
            input
        );
    }

    #[test]
    fn disabled_file_mentions_leave_existing_paths_literal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.md"), "private context").unwrap();

        assert_eq!(
            expand_file_mentions(
                "Inspect @notes.md",
                Some(dir.path().to_str().unwrap()),
                false,
            ),
            "Inspect @notes.md"
        );
    }
}
