use super::*;

const FILE_MENTION_BATCH_SIZE: usize = 32;
const FILE_MENTION_MAX_MATCHES: usize = 5000;
const FILE_MENTION_POLL_BATCHES: usize = 8;
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::tui::app) struct FileMentionRequest {
    pub(in crate::tui::app) root: PathBuf,
    pub(in crate::tui::app) query: String,
    pub(in crate::tui::app) ignore_patterns: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::tui::app) struct FileMentionCandidate {
    pub(in crate::tui::app) score: i32,
    pub(in crate::tui::app) path: String,
    pub(in crate::tui::app) is_dir: bool,
}

#[derive(Debug)]
pub(in crate::tui::app) struct FileMentionBatch {
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
    pub completed: bool,
}

#[cfg(test)]
pub(in crate::tui::app) fn discover_file_mentions(
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
    reason = "Bounded discovery state is explicit"
)]
fn process_file_mention_entry(
    root: &Path,
    query: &str,
    ignored_paths: &ignore::gitignore::Gitignore,
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
    let text = relative.to_string_lossy().replace(MAIN_SEPARATOR, "/");
    let is_dir = entry
        .file_type()
        .is_some_and(|file_type| file_type.is_dir());
    if ignored_paths
        .matched_path_or_any_parents(path, is_dir)
        .is_ignore()
    {
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
            is_dir,
        });
        *match_count += 1;
    }
    if batch.len() >= FILE_MENTION_BATCH_SIZE {
        return send_file_mention_batch(sender, generation, batch, false);
    }
    true
}

fn should_walk_file_mention_entry(
    root: &Path,
    ignored_paths: &ignore::gitignore::Gitignore,
    entry: &ignore::DirEntry,
) -> bool {
    let path = entry.path();
    if path == root {
        return true;
    }
    let is_dir = entry
        .file_type()
        .is_some_and(|file_type| file_type.is_dir());
    !ignored_paths
        .matched_path_or_any_parents(path, is_dir)
        .is_ignore()
}

/// Test-only entry point for deterministic batch and responsiveness checks.
#[cfg(test)]
pub(in crate::tui::app) fn start_file_mention_discovery_for_test(
    root: PathBuf,
    query: String,
    ignore_patterns: Vec<String>,
    generation: u64,
) -> (mpsc::Receiver<FileMentionBatch>, Arc<AtomicBool>) {
    start_file_mention_discovery(root, query, ignore_patterns, generation)
}

#[cfg(test)]
pub(in crate::tui::app) fn completed_file_mention_discovery_for_test(
    root: PathBuf,
    generation: u64,
    path: &str,
) -> FileMentionDiscovery {
    let (sender, receiver) = mpsc::channel();
    assert!(
        sender
            .send(FileMentionBatch {
                generation,
                candidates: vec![FileMentionCandidate {
                    score: 0,
                    path: path.to_string(),
                    is_dir: false,
                }],
                done: true,
            })
            .is_ok(),
        "send completed file mention batch"
    );
    drop(sender);
    FileMentionDiscovery {
        request: FileMentionRequest {
            root,
            query: String::new(),
            ignore_patterns: Vec::new(),
        },
        generation,
        receiver,
        cancel: Arc::new(AtomicBool::new(true)),
        candidates: Vec::new(),
        completed: false,
    }
}

fn discover_file_mentions_batched(
    root: &Path,
    query: &str,
    ignore_patterns: &[String],
    generation: u64,
    cancel: &Arc<AtomicBool>,
    sender: &mpsc::Sender<FileMentionBatch>,
) {
    let mut ignore_builder = ignore::gitignore::GitignoreBuilder::new(root);
    for pattern in BUILTIN_IGNORE_PATTERNS
        .iter()
        .copied()
        .chain(ignore_patterns.iter().map(String::as_str))
    {
        if let Err(error) = ignore_builder.add_line(None, pattern) {
            crate::logging::warn(&format!(
                "Ignoring invalid file mention exclusion pattern {pattern:?}: {error}"
            ));
        }
    }
    let ignored_paths = match ignore_builder.build() {
        Ok(ignored_paths) => Arc::new(ignored_paths),
        Err(error) => {
            crate::logging::warn(&format!(
                "Failed to build file mention exclusion matcher: {error}"
            ));
            let _ = sender.send(FileMentionBatch {
                generation,
                candidates: Vec::new(),
                done: true,
            });
            return;
        }
    };
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
    let direct_root = root.to_path_buf();
    let direct_ignored_paths = Arc::clone(&ignored_paths);
    direct_builder.filter_entry(move |entry| {
        should_walk_file_mention_entry(&direct_root, &direct_ignored_paths, entry)
    });
    for entry in direct_builder.build().filter_map(Result::ok) {
        if !process_file_mention_entry(
            root,
            query,
            &ignored_paths,
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
        let recursive_root = root.to_path_buf();
        let recursive_ignored_paths = Arc::clone(&ignored_paths);
        recursive_builder.filter_entry(move |entry| {
            should_walk_file_mention_entry(&recursive_root, &recursive_ignored_paths, entry)
        });
        for entry in recursive_builder.build().filter_map(Result::ok) {
            if entry.depth() <= 1 {
                continue;
            }
            if !process_file_mention_entry(
                root,
                query,
                &ignored_paths,
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
        let _ = send_file_mention_batch(sender, generation, &mut batch, true);
    } else {
        let _ = sender.send(FileMentionBatch {
            generation,
            candidates: Vec::new(),
            done: true,
        });
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
    pub(in crate::tui::app) fn poll_file_mention_discovery(&mut self) -> bool {
        let mut changed = false;
        let mut disconnected = false;
        let mut pending = self.file_mention_discovery.borrow_mut();
        let Some(discovery) = pending.as_mut() else {
            return false;
        };
        if discovery.completed {
            return false;
        }
        for _ in 0..FILE_MENTION_POLL_BATCHES {
            let batch = match discovery.receiver.try_recv() {
                Ok(batch) => batch,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            };
            if batch.generation != discovery.generation {
                continue;
            }
            discovery.candidates.extend(batch.candidates);
            changed = true;
            if batch.done {
                discovery.completed = true;
                break;
            }
        }
        if disconnected {
            pending.take();
            drop(pending);
            self.set_status_notice("File mention scan stopped unexpectedly");
            crate::logging::warn("File mention discovery worker disconnected before completion");
            changed = true;
        }
        if changed {
            *self.command_suggestions_cache.borrow_mut() = None;
        }
        changed
    }

    pub(in crate::tui::app) fn clear_file_mention_discovery(&self) {
        if let Some(discovery) = self.file_mention_discovery.borrow_mut().take() {
            discovery.cancel.store(false, Ordering::Relaxed);
        }
    }

    pub(in crate::tui::app) fn ensure_file_mention_discovery(&self, request: FileMentionRequest) {
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
            completed: false,
        });
    }
}
