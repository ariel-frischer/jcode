use anyhow::{Context, Result};
use jcode_session_types::lifecycle::{
    LIFECYCLE_SCHEMA_VERSION, LifecycleCompatibilityWarning, LifecycleEventEnvelope,
    LifecycleObservabilityStatus, SessionLifecycleStream,
};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime};

use super::storage_paths::{
    LIFECYCLE_MAX_ROTATIONS, LifecycleArtifactPaths, lifecycle_artifact_paths_in_dir,
};

pub const LIFECYCLE_MAX_FILE_BYTES: u64 = 1024 * 1024;
pub const LIFECYCLE_MAX_AGE_DAYS: u64 = 30;
const MAX_COMPATIBILITY_WARNINGS: usize = 32;

pub fn append_lifecycle_event_in_dir(base: &Path, event: &LifecycleEventEnvelope) -> Result<()> {
    if event.schema_version != LIFECYCLE_SCHEMA_VERSION {
        anyhow::bail!(
            "cannot persist unsupported lifecycle schema version {}",
            event.schema_version
        );
    }
    let paths = lifecycle_artifact_paths_in_dir(base, &event.session_id)?;
    let line = serde_json::to_vec(event).context("serialize lifecycle event")?;
    let line_len = line.len() as u64 + 1;
    if line_len > LIFECYCLE_MAX_FILE_BYTES {
        anyhow::bail!("lifecycle event exceeds the 1 MiB record boundary");
    }

    let sessions_dir = paths
        .active
        .parent()
        .context("lifecycle sidecar has no parent directory")?;
    fs::create_dir_all(sessions_dir).context("create lifecycle sessions directory")?;
    prune_lifecycle_artifacts_in_dir(base, &event.session_id, SystemTime::now())?;

    let active_len = fs::metadata(&paths.active)
        .map(|meta| meta.len())
        .unwrap_or(0);
    if active_len > 0 && active_len.saturating_add(line_len) > LIFECYCLE_MAX_FILE_BYTES {
        rotate_lifecycle_artifacts(&paths)?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.active)
        .context("open lifecycle sidecar")?;
    file.write_all(&line).context("append lifecycle event")?;
    file.write_all(b"\n").context("terminate lifecycle event")?;
    file.flush().context("flush lifecycle event")?;
    Ok(())
}

pub fn read_lifecycle_stream_in_dir(
    base: &Path,
    session_id: &str,
    status: LifecycleObservabilityStatus,
) -> Result<SessionLifecycleStream> {
    // Queries are also a maintenance boundary. Prune before reading so an
    // expired active file cannot reappear in the returned stream, and so the
    // retained artifact set remains bounded even when a session is idle.
    prune_lifecycle_artifacts_in_dir(base, session_id, SystemTime::now())?;
    let paths = lifecycle_artifact_paths_in_dir(base, session_id)?;
    let mut events = Vec::new();
    let mut warnings = Vec::new();

    for path in paths
        .rotations
        .iter()
        .rev()
        .chain(std::iter::once(&paths.active))
    {
        read_lifecycle_file(path, session_id, &mut events, &mut warnings)?;
    }

    events.sort_by_key(|event| event.sequence);
    Ok(SessionLifecycleStream {
        session_id: session_id.to_string(),
        status,
        events,
        warnings,
    })
}

pub fn prune_lifecycle_artifacts_in_dir(
    base: &Path,
    session_id: &str,
    now: SystemTime,
) -> Result<usize> {
    let paths = lifecycle_artifact_paths_in_dir(base, session_id)?;
    let cutoff = now
        .checked_sub(Duration::from_secs(LIFECYCLE_MAX_AGE_DAYS * 24 * 60 * 60))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut removed = 0;
    for path in std::iter::once(paths.active).chain(paths.rotations) {
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        let modified = metadata
            .modified()
            .with_context(|| format!("read modification time for {}", path.display()))?;
        if modified < cutoff {
            match fs::remove_file(&path) {
                Ok(()) => removed += 1,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| format!("remove {}", path.display()));
                }
            }
        }
    }
    Ok(removed)
}

pub(super) fn rotate_lifecycle_artifacts(paths: &LifecycleArtifactPaths) -> Result<()> {
    for rotation in (1..LIFECYCLE_MAX_ROTATIONS).rev() {
        let source = &paths.rotations[rotation - 1];
        let destination = &paths.rotations[rotation];
        if source.exists() {
            if destination.exists() {
                fs::remove_file(destination)
                    .with_context(|| format!("remove {}", destination.display()))?;
            }
            fs::rename(source, destination).with_context(|| {
                format!("rotate {} to {}", source.display(), destination.display())
            })?;
        }
    }
    if paths.active.exists() {
        let first = &paths.rotations[0];
        if first.exists() {
            fs::remove_file(first).with_context(|| format!("remove {}", first.display()))?;
        }
        fs::rename(&paths.active, first)
            .with_context(|| format!("rotate {} to {}", paths.active.display(), first.display()))?;
    }
    Ok(())
}

fn read_lifecycle_file(
    path: &Path,
    session_id: &str,
    events: &mut Vec<LifecycleEventEnvelope>,
    warnings: &mut Vec<LifecycleCompatibilityWarning>,
) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let contents = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let lines = contents.split_inclusive('\n').collect::<Vec<_>>();
    for (index, raw_line) in lines.iter().enumerate() {
        let line_number = index + 1;
        let has_newline = raw_line.ends_with('\n');
        let line = raw_line.trim_end_matches('\n').trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => {
                let warning = if !has_newline && index + 1 == lines.len() {
                    LifecycleCompatibilityWarning::TornTail { line: line_number }
                } else {
                    LifecycleCompatibilityWarning::MalformedRecord { line: line_number }
                };
                push_warning(warnings, warning);
                continue;
            }
        };
        let Some(schema_version) = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .map(|value| value as u16)
        else {
            push_warning(
                warnings,
                LifecycleCompatibilityWarning::MalformedRecord { line: line_number },
            );
            continue;
        };
        if schema_version > LIFECYCLE_SCHEMA_VERSION {
            push_warning(
                warnings,
                LifecycleCompatibilityWarning::UnsupportedSchemaVersion {
                    line: line_number,
                    version: schema_version,
                },
            );
            continue;
        }
        match serde_json::from_value::<LifecycleEventEnvelope>(value) {
            Ok(event) if event.session_id == session_id => events.push(event),
            Ok(_) | Err(_) => push_warning(
                warnings,
                LifecycleCompatibilityWarning::MalformedRecord { line: line_number },
            ),
        }
    }
    Ok(())
}

fn push_warning(
    warnings: &mut Vec<LifecycleCompatibilityWarning>,
    warning: LifecycleCompatibilityWarning,
) {
    if warnings.len() < MAX_COMPATIBILITY_WARNINGS {
        warnings.push(warning);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;

    #[test]
    fn constants_match_the_product_retention_contract() {
        assert_eq!(LIFECYCLE_MAX_FILE_BYTES, 1024 * 1024);
        assert_eq!(LIFECYCLE_MAX_ROTATIONS, 3);
        assert_eq!(LIFECYCLE_MAX_AGE_DAYS, 30);
        assert_eq!(
            ChronoDuration::days(LIFECYCLE_MAX_AGE_DAYS as i64).num_days(),
            30
        );
    }
}
