use anyhow::Result;
use std::path::{Path, PathBuf};

use super::PersistVectorMode;
use crate::storage;

pub(crate) fn session_path_in_dir(base: &std::path::Path, session_id: &str) -> PathBuf {
    base.join("sessions").join(format!("{}.json", session_id))
}

pub const LIFECYCLE_MAX_ROTATIONS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleArtifactPaths {
    pub active: PathBuf,
    pub rotations: Vec<PathBuf>,
}

fn validate_lifecycle_session_id(session_id: &str) -> Result<()> {
    if session_id.is_empty()
        || session_id == "."
        || session_id == ".."
        || !session_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        anyhow::bail!("invalid lifecycle session identifier")
    }
    Ok(())
}

pub fn lifecycle_path_in_dir(base: &Path, session_id: &str) -> Result<PathBuf> {
    validate_lifecycle_session_id(session_id)?;
    Ok(base
        .join("sessions")
        .join(format!("{session_id}.lifecycle.jsonl")))
}

pub fn lifecycle_rotation_path_in_dir(
    base: &Path,
    session_id: &str,
    rotation: usize,
) -> Result<PathBuf> {
    validate_lifecycle_session_id(session_id)?;
    if !(1..=LIFECYCLE_MAX_ROTATIONS).contains(&rotation) {
        anyhow::bail!("lifecycle rotation index must be between 1 and {LIFECYCLE_MAX_ROTATIONS}")
    }
    Ok(base
        .join("sessions")
        .join(format!("{session_id}.lifecycle.{rotation}.jsonl")))
}

pub fn lifecycle_path(session_id: &str) -> Result<PathBuf> {
    let base = storage::jcode_dir()?;
    lifecycle_path_in_dir(&base, session_id)
}

pub fn lifecycle_artifact_paths_in_dir(
    base: &Path,
    session_id: &str,
) -> Result<LifecycleArtifactPaths> {
    let active = lifecycle_path_in_dir(base, session_id)?;
    let rotations = (1..=LIFECYCLE_MAX_ROTATIONS)
        .map(|rotation| lifecycle_rotation_path_in_dir(base, session_id, rotation))
        .collect::<Result<Vec<_>>>()?;
    Ok(LifecycleArtifactPaths { active, rotations })
}

pub(super) use crate::process_memory::estimate_json_bytes;

pub(super) fn file_len_or_zero(path: &Path) -> u64 {
    std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

pub(super) fn persist_vector_mode_label(mode: PersistVectorMode) -> &'static str {
    match mode {
        PersistVectorMode::Clean => "clean",
        PersistVectorMode::Append => "append",
        PersistVectorMode::Full => "full",
    }
}

pub fn session_path(session_id: &str) -> Result<PathBuf> {
    let base = storage::jcode_dir()?;
    Ok(session_path_in_dir(&base, session_id))
}

pub fn session_journal_path_from_snapshot(path: &Path) -> PathBuf {
    let mut name = path
        .file_stem()
        .map(|stem| stem.to_os_string())
        .unwrap_or_default();
    name.push(".journal.jsonl");
    path.with_file_name(name)
}

pub fn session_journal_path(session_id: &str) -> Result<PathBuf> {
    Ok(session_journal_path_from_snapshot(&session_path(
        session_id,
    )?))
}

pub fn session_exists(session_id: &str) -> bool {
    session_path(session_id)
        .map(|path| path.exists())
        .unwrap_or(false)
}
