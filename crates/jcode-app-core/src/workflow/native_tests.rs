use super::{NativeSample, WorkflowStore};
use crate::{bus::WorkflowHealth, config::WorkflowConfig};

fn sample(id: &str, owner: Option<&str>) -> NativeSample {
    NativeSample {
        session_id: id.into(),
        owner: owner.map(str::to_owned),
        working_dir: None,
        started_at: 100,
        allow_registration: true,
        label: "Validator".into(),
        health: WorkflowHealth::Running,
        detail: None,
        activity: Some("Tool activity".into()),
        activity_at: Some(110),
        checkpoint_at: Some(105),
        progress: Some((1, 3)),
    }
}

#[test]
fn expired_absent_native_records_free_capacity_without_resurrecting_running() {
    let root = tempfile::tempdir().unwrap();
    let store = WorkflowStore::open(
        root.path().join("registry.json"),
        WorkflowConfig {
            enabled: true,
            terminal_retention_seconds: 0,
            ..Default::default()
        },
    )
    .unwrap();
    let terminal = (0..256)
        .map(|id| {
            let mut child = sample(&format!("child-{id}"), Some("owner"));
            child.health = WorkflowHealth::Failed;
            child
        })
        .collect();
    assert_eq!(
        store.snapshots_with_native(120, terminal).unwrap().len(),
        256
    );
    assert!(store.snapshots(121).unwrap().is_empty());
    let values = store
        .snapshots_with_native(122, vec![sample("new-child", Some("owner"))])
        .unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].1.id, "native-new-child");
}

#[test]
fn native_only_observation_preserves_clocks_owner_and_terminal_reconnect() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("registry.json");
    let config = WorkflowConfig {
        enabled: true,
        ..Default::default()
    };
    let store = WorkflowStore::open(path.clone(), config.clone()).unwrap();
    let child = sample("child", Some("owner"));
    let values = store
        .snapshots_with_native(120, vec![child.clone()])
        .unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].0, "owner");
    assert_eq!(values[0].1.activity_age_secs, Some(10));
    assert_eq!(values[0].1.checkpoint_age_secs, Some(15));
    let mut failed = child;
    failed.health = WorkflowHealth::Failed;
    failed.detail = Some("Credits exhausted".into());
    store.snapshots_with_native(125, vec![failed]).unwrap();
    drop(store);
    let restored = WorkflowStore::open(path, config).unwrap();
    let values = restored.snapshots(130).unwrap();
    assert_eq!(values[0].1.health, WorkflowHealth::Failed);
    assert_eq!(values[0].1.detail.as_deref(), Some("Credits exhausted"));
    assert!(restored.snapshots(500).unwrap().is_empty());
}

#[test]
fn native_ready_metadata_does_not_clear_failure_but_running_retry_does() {
    let root = tempfile::tempdir().unwrap();
    let store = WorkflowStore::open(
        root.path().join("registry.json"),
        WorkflowConfig {
            enabled: true,
            ..Default::default()
        },
    )
    .unwrap();
    let mut child = sample("child", Some("owner"));
    child.health = WorkflowHealth::Failed;
    child.detail = Some("Credits exhausted".into());
    store
        .snapshots_with_native(120, vec![child.clone()])
        .unwrap();
    child.health = WorkflowHealth::Waiting;
    child.detail = Some("Waiting".into());
    assert_eq!(
        store
            .snapshots_with_native(130, vec![child.clone()])
            .unwrap()[0]
            .1
            .health,
        WorkflowHealth::Failed
    );
    child.health = WorkflowHealth::Running;
    child.activity_at = Some(140);
    child.detail = None;
    assert_eq!(
        store.snapshots_with_native(140, vec![child]).unwrap()[0]
            .1
            .health,
        WorkflowHealth::Running
    );
}

#[test]
fn registered_phases_keep_identity_counts_and_cannot_steal_explicit_owner() {
    let root = tempfile::tempdir().unwrap();
    let store = WorkflowStore::open(
        root.path().join("registry.json"),
        WorkflowConfig {
            enabled: true,
            autospec_enabled: true,
            ..Default::default()
        },
    )
    .unwrap();
    std::fs::write(root.path().join("tasks.yaml"), "phases:\n- number: 1\n  tasks:\n  - id: T1\n    title: work\n    status: Completed\n  - id: T2\n    title: check\n    status: InProgress\n").unwrap();
    let id = store
        .register(
            "owner",
            super::ObserveInput {
                working_dir: root.path().into(),
                tasks_file: "tasks.yaml".into(),
                status_file: None,
                label: None,
            },
            90,
        )
        .unwrap();
    let mut first = sample("phase-a", None);
    first.working_dir = Some(root.path().into());
    first.health = WorkflowHealth::Failed;
    first.detail = Some("Credits exhausted".into());
    let values = store.snapshots_with_native(120, vec![first]).unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].1.id, id);
    assert_eq!(values[0].1.health, WorkflowHealth::Failed);
    assert_eq!(values[0].1.total, Some(2));
    let values = store.snapshots(125).unwrap();
    assert_eq!(values[0].1.health, WorkflowHealth::Failed);
    let mut second = sample("phase-b", None);
    second.working_dir = Some(root.path().into());
    second.started_at = 130;
    let values = store
        .snapshots_with_native(140, vec![second.clone()])
        .unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].1.id, id);
    assert_eq!(values[0].1.completed, Some(1));
    assert_eq!(values[0].1.health, WorkflowHealth::Running);
    second.session_id = "unrelated".into();
    second.owner = Some("other-owner".into());
    let values = store.snapshots_with_native(145, vec![second]).unwrap();
    assert_eq!(
        values
            .iter()
            .filter(|(owner, _)| owner == "other-owner")
            .count(),
        1
    );
    assert_eq!(
        values.iter().filter(|(owner, _)| owner == "owner").count(),
        1
    );
}

#[test]
fn workflow_review_absent_waiting_churn_does_not_freeze_artifacts_or_restart() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("registry.json");
    let config = WorkflowConfig {
        enabled: true,
        autospec_enabled: true,
        terminal_retention_seconds: 10,
        ..Default::default()
    };
    let store = WorkflowStore::open(path.clone(), config.clone()).unwrap();
    let tasks = root.path().join("tasks.yaml");
    std::fs::write(
        &tasks,
        "phases:\n- number: 1\n  tasks:\n  - id: T1\n    title: work\n    status: InProgress\n",
    )
    .unwrap();
    let id = store
        .register(
            "artifact-owner",
            super::ObserveInput {
                working_dir: root.path().into(),
                tasks_file: "tasks.yaml".into(),
                status_file: None,
                label: None,
            },
            90,
        )
        .unwrap();
    let waiting = (0..256)
        .map(|n| {
            let mut s = sample(&format!("old-{n}"), Some("owner"));
            s.health = WorkflowHealth::Waiting;
            s
        })
        .collect();
    store.snapshots_with_native(120, waiting).unwrap();
    std::fs::write(
        &tasks,
        "phases:\n- number: 1\n  tasks:\n  - id: T1\n    title: changed\n    status: Completed\n",
    )
    .unwrap();
    let values = store
        .snapshots_with_native(121, vec![sample("new", Some("owner"))])
        .unwrap();
    assert_eq!(
        values.iter().find(|(_, s)| s.id == id).unwrap().1.completed,
        Some(1)
    );
    assert!(values.iter().any(|(_, s)| s.id == "native-capacity"));
    drop(store);
    let store = WorkflowStore::open(path, config).unwrap();
    let values = store
        .snapshots_with_native(140, vec![sample("new", Some("owner"))])
        .unwrap();
    assert!(values.iter().any(|(_, s)| s.id == "native-new"));
    assert!(!values.iter().any(|(_, s)| s.id.starts_with("native-old-")));
}

#[test]
fn workflow_review_blocked_survives_metadata_and_reopen_until_running_retry() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("registry.json");
    let config = WorkflowConfig {
        enabled: true,
        ..Default::default()
    };
    let store = WorkflowStore::open(path.clone(), config.clone()).unwrap();
    let mut child = sample("blocked", Some("owner"));
    child.health = WorkflowHealth::Blocked;
    child.detail = Some("Blocked".into());
    store
        .snapshots_with_native(120, vec![child.clone()])
        .unwrap();
    drop(store);
    let store = WorkflowStore::open(path, config).unwrap();
    for health in [
        WorkflowHealth::Waiting,
        WorkflowHealth::ObserverError,
        WorkflowHealth::Quiet,
    ] {
        child.health = health;
        assert_eq!(
            store
                .snapshots_with_native(130, vec![child.clone()])
                .unwrap()[0]
                .1
                .health,
            WorkflowHealth::Blocked
        );
    }
    child.health = WorkflowHealth::Running;
    child.activity_at = Some(140);
    assert_eq!(
        store.snapshots_with_native(140, vec![child]).unwrap()[0]
            .1
            .health,
        WorkflowHealth::Running
    );
}

#[test]
fn workflow_review_new_controller_completion_beats_old_native_failure_after_reopen() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("registry.json");
    let config = WorkflowConfig {
        enabled: true,
        autospec_enabled: true,
        ..Default::default()
    };
    let store = WorkflowStore::open(path.clone(), config.clone()).unwrap();
    std::fs::write(
        root.path().join("tasks.yaml"),
        "phases:\n- number: 1\n  tasks:\n  - id: T1\n    title: work\n    status: InProgress\n",
    )
    .unwrap();
    let status = root.path().join("status.json");
    std::fs::write(&status, r#"{"state":"running"}"#).unwrap();
    store
        .register(
            "owner",
            super::ObserveInput {
                working_dir: root.path().into(),
                tasks_file: "tasks.yaml".into(),
                status_file: Some("status.json".into()),
                label: None,
            },
            90,
        )
        .unwrap();
    let mut child = sample("phase", None);
    child.working_dir = Some(root.path().into());
    child.health = WorkflowHealth::Failed;
    assert_eq!(
        store.snapshots_with_native(120, vec![child]).unwrap()[0]
            .1
            .health,
        WorkflowHealth::Failed
    );
    drop(store);
    let store = WorkflowStore::open(path, config).unwrap();
    assert_eq!(
        store.snapshots(125).unwrap()[0].1.health,
        WorkflowHealth::Failed
    );
    std::fs::write(&status, r#"{"state":"completed"}"#).unwrap();
    assert_eq!(
        store.snapshots(130).unwrap()[0].1.health,
        WorkflowHealth::Completed
    );
    assert_eq!(
        store.snapshots(425).unwrap()[0].1.health,
        WorkflowHealth::Completed
    );
    std::fs::write(&status, r#"{"state":"retrying"}"#).unwrap();
    assert_eq!(
        store.snapshots(430).unwrap()[0].1.health,
        WorkflowHealth::Waiting
    );
    std::fs::write(&status, r#"{"state":"running"}"#).unwrap();
    assert_eq!(
        store.snapshots(440).unwrap()[0].1.health,
        WorkflowHealth::Running
    );
}

#[test]
fn workflow_review_persisted_native_consistency_is_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("registry.json");
    let config = WorkflowConfig {
        enabled: true,
        ..Default::default()
    };
    let store = WorkflowStore::open(path.clone(), config.clone()).unwrap();
    store
        .snapshots_with_native(120, vec![sample("child", Some("owner"))])
        .unwrap();
    drop(store);
    let base: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for (pointer, value) in [
        ("/native/0/owner", serde_json::json!("child")),
        ("/native/0/snapshot/id", serde_json::json!("wrong")),
        ("/native/0/snapshot/source", serde_json::json!("wrong")),
        ("/native/0/snapshot/completed", serde_json::json!(4)),
        ("/native/0/terminal_at", serde_json::json!(110)),
        ("/native/0/snapshot/health", serde_json::json!("failed")),
    ] {
        let mut invalid = base.clone();
        *invalid.pointer_mut(pointer).unwrap() = value;
        std::fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert!(
            WorkflowStore::open(path.clone(), config.clone()).is_err(),
            "accepted {pointer}"
        );
    }
}
