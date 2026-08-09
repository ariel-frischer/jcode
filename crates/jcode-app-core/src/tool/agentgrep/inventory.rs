//! A bounded, daemon-owned snapshot for repeated `agentgrep find` calls.
//!
//! The snapshot stores the rendered result, not arbitrary file contents.  A
//! cold call builds a manifest of directory metadata and policy/result file
//! fingerprints.  Warm calls validate those known paths with `stat` and small
//! result-file hashes, avoiding another recursive walk.  Any uncertainty falls
//! back to the canonical agentgrep traversal.

use ::agentgrep::cli::FindArgs;
use ::agentgrep::find::{FindResult, run_find};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::SystemTime;

const MAX_REPOSITORIES: usize = 8;
const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
const MAX_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct QueryKey {
    root: PathBuf,
    query_parts: Vec<String>,
    file_type: Option<String>,
    paths_only: bool,
    debug_score: bool,
    max_files: usize,
    hidden: bool,
    no_ignore: bool,
    glob: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
    is_dir: bool,
    /// Unix ctime changes for writes even when a caller preserves mtime and
    /// length.  On platforms without an equivalent, freshness falls back to
    /// the stored content digest below.
    change_id: Option<(u64, u64, i64, i64)>,
}

#[derive(Debug, Clone)]
struct Manifest {
    directories: Vec<(PathBuf, Stamp)>,
    policy_files: Vec<(PathBuf, Stamp, u64)>,
    files: Vec<(PathBuf, Stamp, u64)>,
}

#[derive(Debug)]
struct Snapshot {
    key: QueryKey,
    result: Arc<FindResult>,
    manifest: Manifest,
    bytes: usize,
    used: u64,
}

#[derive(Default)]
struct State {
    next_use: u64,
    snapshots: Vec<Snapshot>,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::default()));

pub fn find(root: &Path, args: &FindArgs) -> FindResult {
    // These modes deliberately retain agentgrep's live traversal semantics.
    // In particular, hidden/no-ignore requests must not reuse a normal-policy
    // snapshot.
    if args.hidden || args.no_ignore {
        return run_find(root, args);
    }

    let canonical = match root.canonicalize() {
        Ok(path) => path,
        Err(_) => return run_find(root, args),
    };
    let key = QueryKey {
        root: canonical.clone(),
        query_parts: args.query_parts.clone(),
        file_type: args.file_type.clone(),
        paths_only: args.paths_only,
        debug_score: args.debug_score,
        max_files: args.max_files,
        hidden: args.hidden,
        no_ignore: args.no_ignore,
        glob: args.glob.clone(),
    };

    if let Some(result) = lookup(&key, root) {
        return result;
    }

    let result = run_find(root, args);
    let Some(manifest) = build_manifest(&canonical, &result) else {
        return result;
    };
    let bytes = estimate_bytes(&key, &result, &manifest);
    if bytes > MAX_SNAPSHOT_BYTES {
        return result;
    }

    let mut state = STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    state.next_use = state.next_use.wrapping_add(1);
    let used = state.next_use;
    state.snapshots.retain(|snapshot| snapshot.key != key);
    state.snapshots.push(Snapshot {
        key,
        result: Arc::new(result.clone()),
        manifest,
        bytes,
        used,
    });
    evict(&mut state);
    result
}

fn lookup(key: &QueryKey, display_root: &Path) -> Option<FindResult> {
    let mut state = STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let index = state.snapshots.iter().position(|snapshot| {
        snapshot.key == *key && manifest_is_fresh(&snapshot.key.root, &snapshot.manifest)
    })?;
    state.next_use = state.next_use.wrapping_add(1);
    let used = state.next_use;
    let snapshot = &mut state.snapshots[index];
    snapshot.used = used;
    let mut result = (*snapshot.result).clone();
    // Canonicalization is an internal key detail. Preserve the caller-visible
    // root spelling used by an uncached invocation.
    result.root = display_root.display().to_string();
    Some(result)
}

fn build_manifest(root: &Path, _result: &FindResult) -> Option<Manifest> {
    let mut directories = Vec::new();
    let mut policy_files = Vec::new();
    collect_directories(root, &mut directories, &mut policy_files)?;

    let mut files = Vec::new();
    collect_files(root, &mut files)?;
    directories.sort_by(|a, b| a.0.cmp(&b.0));
    policy_files.sort_by(|a, b| a.0.cmp(&b.0));
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Some(Manifest {
        directories,
        policy_files,
        files,
    })
}

fn collect_files(directory: &Path, files: &mut Vec<(PathBuf, Stamp, u64)>) -> Option<()> {
    for entry in fs::read_dir(directory).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        let file_type = entry.file_type().ok()?;
        if file_type.is_dir() {
            collect_files(&path, files)?;
        } else if file_type.is_file()
            && !path
                .components()
                .any(|component| component.as_os_str() == ".git")
        {
            files.push((path.clone(), stamp(&path)?, digest_file(&path)?));
        }
    }
    Some(())
}

fn collect_directories(
    directory: &Path,
    directories: &mut Vec<(PathBuf, Stamp)>,
    policy_files: &mut Vec<(PathBuf, Stamp, u64)>,
) -> Option<()> {
    directories.push((directory.to_path_buf(), stamp(directory)?));
    let entries = fs::read_dir(directory).ok()?;
    for entry in entries {
        let entry = entry.ok()?;
        let path = entry.path();
        let file_type = entry.file_type().ok()?;
        if file_type.is_dir() {
            collect_directories(&path, directories, policy_files)?;
        } else if file_type.is_file()
            && matches!(path.file_name().and_then(|name| name.to_str()),
                Some(".gitignore" | ".ignore" | ".rgignore"))
        {
            policy_files.push((path.clone(), stamp(&path)?, digest_file(&path)?));
        }
    }
    Some(())
}

fn manifest_is_fresh(root: &Path, manifest: &Manifest) -> bool {
    if manifest
        .directories
        .iter()
        .any(|(path, expected)| stamp(path).as_ref() != Some(expected))
    {
        return false;
    }

    let exclude = root.join(".git/info/exclude");
    if let Some(expected) = manifest
        .policy_files
        .iter()
        .find(|(path, _, _)| path == &exclude)
    {
        if !policy_matches(expected) {
            return false;
        }
    } else if exclude.exists() {
        return false;
    }

    manifest.policy_files.iter().all(policy_matches)
        && manifest.files.iter().all(|(path, expected, digest)| {
            let Some(actual) = stamp(path) else {
                return false;
            };
            actual == *expected
                && (expected.change_id.is_some() || digest_file(path) == Some(*digest))
        })
}

fn policy_matches((path, expected, digest): &(PathBuf, Stamp, u64)) -> bool {
    stamp(path).as_ref() == Some(expected) && digest_file(path) == Some(*digest)
}

fn stamp(path: &Path) -> Option<Stamp> {
    let metadata = fs::metadata(path).ok()?;
    Some(Stamp {
        modified: metadata.modified().ok(),
        len: metadata.len(),
        is_dir: metadata.is_dir(),
        change_id: change_id(&metadata),
    })
}

#[cfg(unix)]
fn change_id(metadata: &std::fs::Metadata) -> Option<(u64, u64, i64, i64)> {
    use std::os::unix::fs::MetadataExt;

    Some((metadata.dev(), metadata.ino(), metadata.ctime(), metadata.ctime_nsec()))
}

#[cfg(not(unix))]
fn change_id(_metadata: &std::fs::Metadata) -> Option<(u64, u64, i64, i64)> {
    None
}

fn digest_file(path: &Path) -> Option<u64> {
    let bytes = fs::read(path).ok()?;
    Some(bytes.iter().fold(1469598103934665603u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(1099511628211)
    }))
}

fn estimate_bytes(key: &QueryKey, result: &FindResult, manifest: &Manifest) -> usize {
    let key_bytes = key.root.as_os_str().len()
        + key.query_parts.iter().map(String::len).sum::<usize>()
        + key.file_type.as_deref().map_or(0, str::len)
        + key.glob.as_deref().map_or(0, str::len);
    let result_bytes = result.root.len()
        + result
            .files
            .iter()
            .map(|file| {
                file.path.len()
                    + file.role.len()
                    + file.language.len()
                    + file.why.iter().map(String::len).sum::<usize>()
                    + file
                        .structure
                        .items
                        .iter()
                        .map(|item| item.label.len())
                        .sum::<usize>()
            })
            .sum::<usize>();
    let manifest_bytes = manifest
        .directories
        .iter()
        .map(|(path, _)| path.as_os_str().len() + 32)
        .sum::<usize>()
        + manifest
            .policy_files
            .iter()
            .map(|(path, _, _)| path.as_os_str().len() + 40)
            .sum::<usize>()
        + manifest
            .files
            .iter()
            .map(|(path, _, _)| path.as_os_str().len() + 40)
            .sum::<usize>();
    key_bytes + result_bytes + manifest_bytes
}

fn evict(state: &mut State) {
    while state.snapshots.len() > MAX_REPOSITORIES
        || state.snapshots.iter().map(|snapshot| snapshot.bytes).sum::<usize>()
            > MAX_TOTAL_BYTES
    {
        let Some(index) = state
            .snapshots
            .iter()
            .enumerate()
            .min_by_key(|(_, snapshot)| snapshot.used)
            .map(|(index, _)| index)
        else {
            break;
        };
        state.snapshots.remove(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn args(query: &str) -> FindArgs {
        FindArgs {
            query_parts: vec![query.into()],
            file_type: None,
            json: false,
            paths_only: false,
            debug_score: false,
            max_files: 10,
            hidden: false,
            no_ignore: false,
            path: None,
            glob: None,
        }
    }

    fn clear() {
        STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).snapshots.clear();
    }

    #[test]
    fn snapshot_matches_uncached_and_invalidates_create_delete_and_policy() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("visible.rs"), "fn visible() {}\n").unwrap();
        clear();
        let first = find(directory.path(), &args("visible"));
        let uncached = run_find(directory.path(), &args("visible"));
        assert_eq!(serde_json::to_value(&first.files).unwrap(), serde_json::to_value(&uncached.files).unwrap());

        fs::write(directory.path().join("new_visible.rs"), "fn new_visible() {}\n").unwrap();
        let created = find(directory.path(), &args("new_visible"));
        assert_eq!(
            serde_json::to_value(&created.files).unwrap(),
            serde_json::to_value(&run_find(directory.path(), &args("new_visible")).files).unwrap()
        );

        fs::remove_file(directory.path().join("visible.rs")).unwrap();
        let deleted = find(directory.path(), &args("visible"));
        assert_eq!(
            serde_json::to_value(&deleted.files).unwrap(),
            serde_json::to_value(&run_find(directory.path(), &args("visible")).files).unwrap()
        );

        fs::write(directory.path().join(".gitignore"), "new_visible.rs\n").unwrap();
        let ignored = find(directory.path(), &args("new_visible"));
        assert_eq!(
            serde_json::to_value(&ignored.files).unwrap(),
            serde_json::to_value(&run_find(directory.path(), &args("new_visible")).files).unwrap()
        );
    }

    #[test]
    fn snapshots_are_bounded_and_filter_keys_do_not_collide() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("one.rs"), "fn one() {}\n").unwrap();
        fs::write(directory.path().join("one.txt"), "one\n").unwrap();
        clear();
        let _ = find(directory.path(), &args("one"));
        let mut typed = args("one");
        typed.file_type = Some("rs".into());
        let typed_result = find(directory.path(), &typed);
        assert!(typed_result.files.iter().all(|file| file.path.ends_with(".rs")));
        let state = STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(state.snapshots.len() <= MAX_REPOSITORIES);
        assert!(state.snapshots.iter().map(|snapshot| snapshot.bytes).sum::<usize>() <= MAX_TOTAL_BYTES);
    }

    #[test]
    fn same_length_file_write_invalidates_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("same.rs");
        fs::write(&path, "fn visible() {}\n").unwrap();
        clear();
        let first = find(directory.path(), &args("visible"));
        assert_eq!(first.files.len(), 1);

        // Keep length stable so mtime/size-only validation cannot accept stale
        // results.  Unix ctime is the cheap warm-path write detector.
        fs::write(&path, "fn hiddenx() {}\n").unwrap();
        let current = find(directory.path(), &args("visible"));
        let uncached = run_find(directory.path(), &args("visible"));
        assert_eq!(current.files, uncached.files);
        assert!(current.files.is_empty());
    }
}
