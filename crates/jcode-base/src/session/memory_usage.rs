//! Bounded private local request diagnostics, never a lifetime ledger.
//! Fixed ring and scan bounds apply globally, not per session. Reads are read-only.
use super::lifecycle::{
    LIFECYCLE_MAX_AGE_DAYS, LIFECYCLE_MAX_FILE_BYTES, rotate_lifecycle_artifacts,
};
use super::storage_paths::{
    memory_usage_artifact_paths, private_diagnostic_directory, private_diagnostic_file,
};
use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use jcode_session_types::memory_usage::{MemoryRequestObservation, validate_accounting_identifier};
use serde::Serialize;
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::Path;

pub const MAX_RECORD_BYTES: usize = 4096;
pub const MAX_RECORDS: usize = 4096;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageWarning {
    RetainedWindowOnly,
    /// No durable lifetime loss ledger. Abrupt exit, disabled intervals, and lost
    /// writes from past processes cannot be reconstructed, even with zero live loss.
    LossHistoryUnavailable,
    MalformedRecord,
    ScanLimit,
    ExpiredRecords,
    StorageUnavailable,
    DuplicateRecord,
}
#[derive(Debug, Serialize)]
pub struct UsageHistory {
    pub calls: Vec<MemoryRequestObservation>,
    pub warnings: Vec<StorageWarning>,
}
impl UsageHistory {
    fn warn(&mut self, warning: StorageWarning) {
        if !self.warnings.contains(&warning) {
            self.warnings.push(warning);
        }
    }
}

pub fn append_in_dir(base: &Path, record: &MemoryRequestObservation) -> Result<()> {
    record.validate()?;
    let mut line = serde_json::to_vec(record).context("accounting serialization failed")?;
    line.push(b'\n');
    anyhow::ensure!(
        line.len() <= MAX_RECORD_BYTES,
        "accounting record too large"
    );
    let directory = base.join("memory-usage");
    private_diagnostic_directory(&directory, true).context("accounting directory unavailable")?;
    let lock = private_diagnostic_file(&directory.join("writer.lock"), true)
        .context("accounting lock unavailable")?;
    lock.try_lock().context("accounting storage busy")?;
    let paths = memory_usage_artifact_paths(base);
    let cutoff = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(
            LIFECYCLE_MAX_AGE_DAYS * 86400,
        ))
        .unwrap_or(std::time::UNIX_EPOCH);
    // Validate every artifact before rotation, including destinations. Do not
    // follow symlinks, chmod user files or delete anything outside this fixed set.
    for path in std::iter::once(&paths.active).chain(&paths.rotations) {
        match private_diagnostic_file(path, false) {
            Ok(file) => {
                anyhow::ensure!(
                    file.metadata()?.len() <= LIFECYCLE_MAX_FILE_BYTES,
                    "oversized accounting artifact"
                );
                if file.metadata()?.modified()? < cutoff {
                    std::fs::remove_file(path).context("accounting retention failed")?;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => anyhow::bail!("accounting artifact unavailable"),
        }
    }
    let active = private_diagnostic_file(&paths.active, true)
        .context("accounting active file unavailable")?;
    let len = active.metadata()?.len();
    drop(active);
    if len.saturating_add(line.len() as u64) > LIFECYCLE_MAX_FILE_BYTES {
        rotate_lifecycle_artifacts(&paths)
            .map_err(|_| anyhow::anyhow!("accounting rotation failed"))?;
    }
    let mut file = private_diagnostic_file(&paths.active, true)
        .context("accounting active file unavailable")?;
    file.write_all(&line).context("accounting write failed")?;
    file.flush().context("accounting flush failed")?;
    Ok(())
}

pub fn read_in_dir(base: &Path, session: Option<&str>) -> Result<UsageHistory> {
    if let Some(session) = session {
        validate_accounting_identifier(session)?;
    }
    let mut history = UsageHistory {
        calls: Vec::new(),
        warnings: vec![
            StorageWarning::RetainedWindowOnly,
            StorageWarning::LossHistoryUnavailable,
        ],
    };
    let directory = base.join("memory-usage");
    match private_diagnostic_directory(&directory, false) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(history),
        Err(_) => {
            history.warn(StorageWarning::StorageUnavailable);
            return Ok(history);
        }
    }
    // Reader never creates files or mutates retention. Exclude expired artifacts
    // and records here, prune expired files at the next authorized append.
    let lock = match private_diagnostic_file(&directory.join("writer.lock"), false) {
        Ok(lock) => lock,
        Err(_) => {
            history.warn(StorageWarning::StorageUnavailable);
            return Ok(history);
        }
    };
    if lock.try_lock().is_err() {
        history.warn(StorageWarning::StorageUnavailable);
        return Ok(history);
    }
    let paths = memory_usage_artifact_paths(base);
    let mut seen = HashSet::new();
    for path in std::iter::once(&paths.active).chain(&paths.rotations) {
        read_file(path, &mut history, &mut seen);
    }
    if let Some(session) = session {
        history
            .calls
            .retain(|r| r.context.session_id.as_deref() == Some(session));
    }
    history
        .calls
        .sort_by(|a, b| (a.recorded_at, &a.request_id).cmp(&(b.recorded_at, &b.request_id)));
    Ok(history)
}

fn read_file(path: &Path, history: &mut UsageHistory, seen: &mut HashSet<String>) {
    let file = match private_diagnostic_file(path, false) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(_) => {
            history.warn(StorageWarning::StorageUnavailable);
            return;
        }
    };
    let cutoff = Utc::now() - Duration::days(LIFECYCLE_MAX_AGE_DAYS as i64);
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(_) => {
            history.warn(StorageWarning::StorageUnavailable);
            return;
        }
    };
    match metadata.modified() {
        Ok(modified) if chrono::DateTime::<Utc>::from(modified) < cutoff => {
            history.warn(StorageWarning::ExpiredRecords);
            return;
        }
        Err(_) => {
            history.warn(StorageWarning::StorageUnavailable);
            return;
        }
        _ => {}
    }
    if metadata.len() > LIFECYCLE_MAX_FILE_BYTES {
        history.warn(StorageWarning::ScanLimit);
    }
    let mut bytes = Vec::new();
    if file
        .take(LIFECYCLE_MAX_FILE_BYTES)
        .read_to_end(&mut bytes)
        .is_err()
    {
        history.warn(StorageWarning::StorageUnavailable);
        return;
    }
    for line in bytes.split_inclusive(|b| *b == b'\n') {
        if line.len() > MAX_RECORD_BYTES || !line.ends_with(b"\n") {
            history.warn(StorageWarning::MalformedRecord);
            continue;
        }
        let record = match serde_json::from_slice::<MemoryRequestObservation>(line) {
            Ok(record) if record.validate().is_ok() => record,
            _ => {
                history.warn(StorageWarning::MalformedRecord);
                continue;
            }
        };
        if record.recorded_at < cutoff {
            history.warn(StorageWarning::ExpiredRecords);
            continue;
        }
        if seen.contains(&record.request_id) {
            history.warn(StorageWarning::DuplicateRecord);
            continue;
        }
        if seen.len() >= MAX_RECORDS {
            history.warn(StorageWarning::ScanLimit);
            return;
        }
        seen.insert(record.request_id.clone());
        history.calls.push(record);
    }
}

#[cfg(test)]
#[path = "memory_usage_tests.rs"]
pub(crate) mod tests;
