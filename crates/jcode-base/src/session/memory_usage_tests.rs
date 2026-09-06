use super::*;
use chrono::Utc;
use jcode_session_types::memory_usage::*;
use std::fs;

pub(crate) fn record(id: &str) -> MemoryRequestObservation {
    MemoryRequestObservation {
        schema_version: MEMORY_USAGE_SCHEMA_VERSION,
        request_id: id.into(),
        context: MemoryCallContext {
            session_id: Some("session-a".into()),
            operation_id: "operation-a".into(),
            operation_kind: MemoryOperationKind::Rerank,
        },
        recorded_at: Utc::now(),
        provider: "openai".into(),
        model: "gpt-5.6-luna".into(),
        effort: Some(ReasoningEffort::Xhigh),
        auth_class: AuthClass::Oauth,
        outcome: RequestOutcome::Success,
        usage: TokenUsage {
            input_tokens: Some(10),
            output_tokens: Some(5),
            ..Default::default()
        },
        attempt_coverage: AttemptCoverage::PhysicalAttempt,
        pricing: CostEstimate {
            basis: PricingBasis::Unknown,
            estimate_nano_usd: None,
            known_subtotal_nano_usd: 0,
        },
    }
}
fn active(base: &Path) -> std::path::PathBuf {
    base.join("memory-usage").join("requests.v1.jsonl")
}

#[test]
fn roundtrip_private_deduplicated_and_session_filtered() {
    let dir = tempfile::tempdir().unwrap();
    let first = record("request-a");
    append_in_dir(dir.path(), &first).unwrap();
    append_in_dir(dir.path(), &first).unwrap();
    let mut other = record("request-b");
    other.context.session_id = None;
    append_in_dir(dir.path(), &other).unwrap();
    let history = read_in_dir(dir.path(), None).unwrap();
    assert_eq!(history.calls.len(), 2);
    assert!(history.warnings.contains(&StorageWarning::DuplicateRecord));
    assert!(
        history
            .warnings
            .contains(&StorageWarning::LossHistoryUnavailable)
    );
    assert_eq!(
        read_in_dir(dir.path(), Some("session-a")).unwrap().calls,
        vec![first]
    );
    assert_eq!(
        read_in_dir(dir.path(), Some("missing"))
            .unwrap()
            .calls
            .len(),
        0
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(active(dir.path()))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(active(dir.path()).parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}

#[test]
fn invalid_selectors_and_content_fields_are_rejected_without_echo() {
    let dir = tempfile::tempdir().unwrap();
    for id in ["../SENSITIVE_SENTINEL", "/root", "", ".."] {
        assert!(
            !read_in_dir(dir.path(), Some(id))
                .unwrap_err()
                .to_string()
                .contains("SENSITIVE_SENTINEL")
        );
        let mut bad = record("valid");
        bad.context.session_id = Some(id.into());
        assert!(append_in_dir(dir.path(), &bad).is_err());
    }
    let mut bad = record("valid");
    bad.model = "x".repeat(129);
    assert!(append_in_dir(dir.path(), &bad).is_err());
    assert!(!active(dir.path()).exists());
}

#[test]
fn corrupt_oversized_and_torn_records_recover_with_safe_warnings() {
    let dir = tempfile::tempdir().unwrap();
    append_in_dir(dir.path(), &record("good")).unwrap();
    let path = active(dir.path());
    let mut content = fs::read(&path).unwrap();
    content.extend_from_slice(b"{\"prompt\":\"SENSITIVE_SENTINEL\"}\n");
    content.extend_from_slice(&vec![b'x'; MAX_RECORD_BYTES + 1]);
    content.push(b'\n');
    content.extend_from_slice(b"{\"secret\":\"SENSITIVE_SENTINEL");
    fs::write(path, content).unwrap();
    let history = read_in_dir(dir.path(), None).unwrap();
    assert_eq!(history.calls.len(), 1);
    assert!(history.warnings.contains(&StorageWarning::MalformedRecord));
    assert!(
        !serde_json::to_string(&history)
            .unwrap()
            .contains("SENSITIVE_SENTINEL")
    );
}

#[test]
fn fixed_rotation_and_expired_records_are_not_lifetime_zero() {
    let dir = tempfile::tempdir().unwrap();
    append_in_dir(dir.path(), &record("initial")).unwrap();
    for n in 0..6 {
        // Fill active to the exact shared 1 MiB rotation boundary.
        let file = fs::OpenOptions::new()
            .write(true)
            .open(active(dir.path()))
            .unwrap();
        file.set_len(super::super::LIFECYCLE_MAX_FILE_BYTES)
            .unwrap();
        append_in_dir(dir.path(), &record(&format!("r{n}"))).unwrap();
    }
    assert_eq!(
        fs::read_dir(active(dir.path()).parent().unwrap())
            .unwrap()
            .count(),
        5
    ); // active, 3 rotations, lock
    let mut old = record("expired");
    old.recorded_at -= chrono::Duration::days(31);
    append_in_dir(dir.path(), &old).unwrap();
    let history = read_in_dir(dir.path(), None).unwrap();
    assert!(!history.calls.iter().any(|r| r.request_id == "expired"));
    assert!(history.warnings.contains(&StorageWarning::ExpiredRecords));
    assert!(
        history
            .warnings
            .contains(&StorageWarning::RetainedWindowOnly)
    );
    assert!(history.calls.len() <= MAX_RECORDS);
    assert!(history.warnings.len() <= 7);
}

#[test]
fn storage_failure_and_empty_restart_expose_uncertainty() {
    let dir = tempfile::tempdir().unwrap();
    let history = read_in_dir(dir.path(), None).unwrap();
    assert!(history.calls.is_empty());
    assert!(
        history
            .warnings
            .contains(&StorageWarning::LossHistoryUnavailable)
    );
    fs::write(dir.path().join("memory-usage"), b"not a directory").unwrap();
    assert!(append_in_dir(dir.path(), &record("r")).is_err());
    assert!(
        read_in_dir(dir.path(), None)
            .unwrap()
            .warnings
            .contains(&StorageWarning::StorageUnavailable)
    );
}

#[cfg(unix)]
#[test]
fn symlinks_and_nonprivate_existing_files_are_refused() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside");
    fs::write(&outside, b"SENSITIVE_SENTINEL").unwrap();
    fs::create_dir(dir.path().join("memory-usage")).unwrap();
    fs::set_permissions(
        dir.path().join("memory-usage"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    symlink(&outside, active(dir.path())).unwrap();
    assert!(append_in_dir(dir.path(), &record("r")).is_err());
    let history = read_in_dir(dir.path(), None).unwrap();
    assert!(
        history
            .warnings
            .contains(&StorageWarning::StorageUnavailable)
    );
    assert_eq!(fs::read(&outside).unwrap(), b"SENSITIVE_SENTINEL");
    fs::remove_file(active(dir.path())).unwrap();
    fs::write(active(dir.path()), b"{}").unwrap();
    fs::set_permissions(active(dir.path()), fs::Permissions::from_mode(0o644)).unwrap();
    assert!(append_in_dir(dir.path(), &record("r")).is_err());
}

#[test]
fn expired_files_are_hidden_on_read_and_pruned_on_append() {
    let dir = tempfile::tempdir().unwrap();
    append_in_dir(dir.path(), &record("old-file")).unwrap();
    let file = fs::OpenOptions::new()
        .write(true)
        .open(active(dir.path()))
        .unwrap();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(31 * 86400);
    file.set_times(fs::FileTimes::new().set_modified(old))
        .unwrap();
    let history = read_in_dir(dir.path(), None).unwrap();
    assert!(history.calls.is_empty());
    assert!(history.warnings.contains(&StorageWarning::ExpiredRecords));
    assert!(
        active(dir.path()).exists(),
        "read does not mutate retention"
    );
    append_in_dir(dir.path(), &record("new-file")).unwrap();
    assert_eq!(read_in_dir(dir.path(), None).unwrap().calls.len(), 1);
    assert!(
        !fs::read_to_string(active(dir.path()))
            .unwrap()
            .contains("old-file")
    );
}

#[test]
fn oversized_file_scan_and_busy_lock_are_bounded() {
    let dir = tempfile::tempdir().unwrap();
    append_in_dir(dir.path(), &record("good")).unwrap();
    let file = fs::OpenOptions::new()
        .write(true)
        .open(active(dir.path()))
        .unwrap();
    file.set_len(super::super::LIFECYCLE_MAX_FILE_BYTES * 10)
        .unwrap();
    let history = read_in_dir(dir.path(), None).unwrap();
    assert_eq!(history.calls.len(), 1);
    assert!(history.warnings.contains(&StorageWarning::ScanLimit));
    let lock = fs::File::open(dir.path().join("memory-usage/writer.lock")).unwrap();
    lock.try_lock().unwrap();
    let start = std::time::Instant::now();
    assert!(append_in_dir(dir.path(), &record("blocked")).is_err());
    assert!(
        read_in_dir(dir.path(), None)
            .unwrap()
            .warnings
            .contains(&StorageWarning::StorageUnavailable)
    );
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
}

#[test]
fn decoded_state_limit_and_valid_record_unknown_fields_are_enforced() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    append_in_dir(dir.path(), &record("initial")).unwrap();
    let paths = super::super::storage_paths::memory_usage_artifact_paths(dir.path());
    for (index, path) in std::iter::once(&paths.active)
        .chain(&paths.rotations)
        .enumerate()
    {
        let mut file = super::super::storage_paths::private_diagnostic_file(path, true).unwrap();
        file.set_len(0).unwrap();
        for n in 0..1200 {
            let mut bytes = serde_json::to_vec(&record(&format!("r{index}-{n}"))).unwrap();
            bytes.push(b'\n');
            file.write_all(&bytes).unwrap();
        }
        assert!(file.metadata().unwrap().len() <= super::super::LIFECYCLE_MAX_FILE_BYTES);
    }
    let history = read_in_dir(dir.path(), None).unwrap();
    assert_eq!(history.calls.len(), MAX_RECORDS);
    assert!(history.warnings.contains(&StorageWarning::ScanLimit));
    let isolated = tempfile::tempdir().unwrap();
    append_in_dir(isolated.path(), &record("known")).unwrap();
    let mut value = serde_json::to_value(record("bad")).unwrap();
    value["prompt"] = "SENSITIVE_SENTINEL".into();
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(active(isolated.path()))
        .unwrap();
    writeln!(file, "{value}").unwrap();
    let history = read_in_dir(isolated.path(), None).unwrap();
    assert_eq!(history.calls.len(), 1);
    assert!(history.warnings.contains(&StorageWarning::MalformedRecord));
    assert!(
        !serde_json::to_string(&history)
            .unwrap()
            .contains("SENSITIVE_SENTINEL")
    );
}
