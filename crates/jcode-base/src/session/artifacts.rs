use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use super::session_journal_path_from_snapshot;
use super::storage_paths::{
    lifecycle_artifact_paths_in_dir, lifecycle_path_in_dir, session_path_in_dir,
};

pub fn session_artifact_paths_in_dir(base: &Path, session_id: &str) -> Result<Vec<PathBuf>> {
    let snapshot = session_path_in_dir(base, session_id);
    let journal = session_journal_path_from_snapshot(&snapshot);
    let lifecycle = lifecycle_artifact_paths_in_dir(base, session_id)?;
    let mut paths = vec![
        snapshot.clone(),
        journal.clone(),
        snapshot.with_extension("json.bak"),
        snapshot.with_extension("bak"),
    ];
    paths.push(lifecycle.active);
    paths.extend(lifecycle.rotations);

    let sessions_dir = base.join("sessions");
    if let Ok(entries) = fs::read_dir(&sessions_dir) {
        let snapshot_backup_prefix = format!("{session_id}.json.pre-wipe-");
        let journal_backup_prefix = format!("{session_id}.journal.jsonl.pre-wipe-");
        let corrupt_journal = format!("{session_id}.journal.corrupt.jsonl");
        for entry in entries {
            let entry = entry.context("read session artifact directory entry")?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if (name.starts_with(&snapshot_backup_prefix)
                || name.starts_with(&journal_backup_prefix))
                && name.ends_with(".bak")
                || name == corrupt_journal
            {
                paths.push(entry.path());
            }
        }
    }
    Ok(paths)
}

pub fn remove_session_artifacts_in_dir(base: &Path, session_id: &str) -> Result<usize> {
    // Calling the validated lifecycle helper first prevents a textual prefix
    // from ever becoming a broad deletion boundary.
    let _ = lifecycle_path_in_dir(base, session_id)?;
    let mut removed = 0;
    for path in session_artifact_paths_in_dir(base, session_id)? {
        match fs::remove_file(&path) {
            Ok(()) => removed += 1,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).with_context(|| format!("remove {}", path.display())),
        }
    }
    Ok(removed)
}
