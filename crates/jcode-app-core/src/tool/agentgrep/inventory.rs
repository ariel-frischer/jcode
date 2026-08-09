use ::agentgrep::cli::FindArgs;
use ::agentgrep::find::{FindResult, run_find, run_find_with_inventory};
use ::agentgrep::workspace::{
    FileInventory, SearchScope, collect_file_inventory, inventory_is_fresh,
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

const MAX_REPOSITORIES: usize = 8;
const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_INVENTORY_BYTES: usize = 16 * 1024 * 1024;

struct CachedInventory {
    root: PathBuf,
    inventory: Arc<FileInventory>,
    bytes: usize,
    used: u64,
}
static CACHE: LazyLock<Mutex<Vec<CachedInventory>>> = LazyLock::new(|| Mutex::new(Vec::new()));

pub fn find(root: &Path, args: &FindArgs) -> FindResult {
    if args.hidden || args.no_ignore {
        return run_find(root, args);
    }
    let scope = SearchScope {
        root,
        file_type: None,
        glob: None,
        hidden: false,
        no_ignore: false,
    };
    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let candidate = {
        let guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        let next_use = guard.iter().map(|item| item.used).max().unwrap_or(0) + 1;
        guard
            .iter()
            .find(|item| item.root == canonical)
            .map(|item| (item.inventory.clone(), next_use))
    };
    if let Some((inventory, next_use)) =
        candidate.filter(|(inventory, _)| inventory_is_fresh(root, &inventory.freshness))
    {
        let result = run_find_with_inventory(root, args, &inventory.entries);
        if let Some(item) = CACHE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter_mut()
            .find(|item| item.root == canonical)
        {
            item.used = next_use;
        }
        return result;
    }
    let inventory = Arc::new(collect_file_inventory(&scope));
    let bytes = inventory
        .entries
        .iter()
        .map(|entry| entry.relative_path.len() + entry.path.as_os_str().len() + 32)
        .sum();
    let result = run_find_with_inventory(root, args, &inventory.entries);
    if bytes <= MAX_INVENTORY_BYTES {
        let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        let next_use = guard.iter().map(|item| item.used).max().unwrap_or(0) + 1;
        guard.retain(|item| item.root != canonical);
        guard.push(CachedInventory {
            root: canonical,
            inventory,
            bytes,
            used: next_use,
        });
        while guard.len() > MAX_REPOSITORIES
            || guard.iter().map(|item| item.bytes).sum::<usize>() > MAX_BYTES
        {
            if let Some(index) = guard
                .iter()
                .enumerate()
                .min_by_key(|(_, item)| item.used)
                .map(|(index, _)| index)
            {
                guard.remove(index);
            } else {
                break;
            }
        }
    }
    result
}

#[cfg(test)]
pub fn clear_for_tests() {
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).clear();
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

    #[test]
    fn warm_result_matches_fresh_and_create_invalidates() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/first.rs"), "fn first() {}\n").unwrap();
        clear_for_tests();
        let first = find(dir.path(), &args("first"));
        let fresh = run_find(dir.path(), &args("first"));
        assert_eq!(
            serde_json::to_value(&first.files).unwrap(),
            serde_json::to_value(&fresh.files).unwrap()
        );
        fs::write(dir.path().join("src/second.rs"), "fn second() {}\n").unwrap();
        let current = find(dir.path(), &args("second"));
        let fresh_current = run_find(dir.path(), &args("second"));
        assert_eq!(
            serde_json::to_value(&current.files).unwrap(),
            serde_json::to_value(&fresh_current.files).unwrap()
        );
    }

    #[test]
    fn ignore_policy_change_invalidates_inventory() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("visible.rs"), "fn visible() {}\n").unwrap();
        let scope = SearchScope {
            root: dir.path(),
            file_type: None,
            glob: None,
            hidden: false,
            no_ignore: false,
        };
        let manifest = collect_file_inventory(&scope).freshness;
        assert!(inventory_is_fresh(dir.path(), &manifest));
        fs::write(dir.path().join(".gitignore"), "visible.rs\n").unwrap();
        assert!(!inventory_is_fresh(dir.path(), &manifest));
    }
}
