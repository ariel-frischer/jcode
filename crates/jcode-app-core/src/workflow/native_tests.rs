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
