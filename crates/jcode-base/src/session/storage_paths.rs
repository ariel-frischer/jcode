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

/// Fixed accounting ring: file count does not grow with session cardinality.
pub(super) fn memory_usage_artifact_paths(base: &Path) -> LifecycleArtifactPaths {
    let directory = base.join("memory-usage");
    LifecycleArtifactPaths {
        active: directory.join("requests.v1.jsonl"),
        rotations: (1..=LIFECYCLE_MAX_ROTATIONS)
            .map(|n| directory.join(format!("requests.v1.{n}.jsonl")))
            .collect(),
    }
}

/// Fail closed on links, non-regular files and non-private existing artifacts.
/// The data root is trusted; the task-owned child must be private before use.
pub(super) fn private_diagnostic_directory(path: &Path, create: bool) -> std::io::Result<()> {
    if create {
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir() || !private_diagnostic_metadata(&metadata) {
        return Err(std::io::ErrorKind::PermissionDenied.into());
    }
    #[cfg(windows)]
    if create {
        jcode_core::fs::set_directory_permissions_owner_only(path)?;
    }
    Ok(())
}

pub(super) fn private_diagnostic_file(path: &Path, append: bool) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true).append(append).create(append);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_OPEN_REPARSE_POINT: inspect the link itself, not its target.
        options.custom_flags(0x00200000);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || !private_diagnostic_metadata(&metadata) {
        return Err(std::io::ErrorKind::PermissionDenied.into());
    }
    #[cfg(windows)]
    if append {
        jcode_core::fs::set_permissions_owner_only(path)?;
    }
    Ok(file)
}

fn private_diagnostic_metadata(metadata: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        metadata.permissions().mode() & 0o077 == 0 && (metadata.is_dir() || metadata.nlink() == 1)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Writer applies the existing protected owner-only ACL before data is
        // written. Read-only reporting does not modify ACLs.
        metadata.file_attributes() & 0x400 == 0
    }
}

pub(super) fn current_working_dir_string() -> Option<String> {
    // Working-directory metadata is optional when the directory was removed.
    match std::env::current_dir() {
        Ok(path) => Some(path.to_string_lossy().to_string()),
        Err(_) => None,
    }
}
