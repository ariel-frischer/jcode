use super::*;
use crate::bus::WorkflowHealth;

fn tasks(status: &str) -> String {
    format!(
        "phases:\n- number: 1\n  tasks:\n  - id: T001\n    title: Validate\n    status: {status}\n"
    )
}

#[test]
fn observer_keeps_last_good_progress_and_checkpoint_on_partial_artifacts() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tasks.yaml");
    std::fs::write(&path, tasks("InProgress")).unwrap();
    let mut registry = registry::Registry::default();
    registry
        .register(
            "owner",
            registry::ObserveInput {
                working_dir: root.path().into(),
                tasks_file: "tasks.yaml".into(),
                status_file: None,
                label: None,
            },
            100,
        )
        .unwrap();
    let run = &mut registry.registrations[0];
    let first = observe(run, 100, 30);
    assert_eq!(first.total, Some(1));
    assert_eq!(first.activity_age_secs, None);
    assert_eq!(first.checkpoint_age_secs, Some(0));
    assert_eq!(observe(run, 140, 30).checkpoint_age_secs, Some(40));
    assert_eq!(observe(run, 140, 30).health, WorkflowHealth::Quiet);
    std::fs::write(&path, b"phases: [").unwrap();
    let partial = observe(run, 150, 30);
    assert_eq!(partial.health, WorkflowHealth::ObserverError);
    assert_eq!(partial.total, Some(1));
    assert_eq!(partial.checkpoint_age_secs, Some(50));
    std::fs::write(&path, tasks("Completed")).unwrap();
    let complete = observe(run, 160, 30);
    assert_eq!(complete.completed, Some(1));
    assert_eq!(complete.checkpoint_age_secs, Some(0));
    assert_ne!(complete.health, WorkflowHealth::Completed);
    assert_eq!(complete.activity_age_secs, Some(0));
}

#[test]
fn controller_failure_is_sticky_safe_and_explicit_retry_can_recover() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("tasks.yaml"), tasks("InProgress")).unwrap();
    let status = root.path().join("status.json");
    std::fs::write(
        &status,
        br#"{"state":"failed","error_code":"insufficient_quota","message":"SECRET"}"#,
    )
    .unwrap();
    let mut registry = registry::Registry::default();
    registry
        .register(
            "owner",
            registry::ObserveInput {
                working_dir: root.path().into(),
                tasks_file: "tasks.yaml".into(),
                status_file: Some("status.json".into()),
                label: None,
            },
            100,
        )
        .unwrap();
    let run = &mut registry.registrations[0];
    let failed = observe(run, 100, 30);
    assert_eq!(failed.health, WorkflowHealth::Failed);
    assert_eq!(failed.detail.as_deref(), Some("Credits exhausted"));
    assert!(!serde_json::to_string(&failed).unwrap().contains("SECRET"));
    let saved = serde_json::to_vec(run).unwrap();
    *run = serde_json::from_slice(&saved).unwrap();
    std::fs::remove_file(&status).unwrap();
    let missing = observe(run, 140, 30);
    assert_eq!(missing.health, WorkflowHealth::Failed);
    assert!(missing.detail.unwrap().contains("Credits exhausted"));
    std::fs::write(&status, br#"{"state":"retrying"}"#).unwrap();
    assert_eq!(observe(run, 150, 30).health, WorkflowHealth::Waiting);
    std::fs::write(&status, br#"{"state":"completed"}"#).unwrap();
    assert_eq!(observe(run, 160, 30).health, WorkflowHealth::Completed);
}
