use super::registry::ObserveInput;
use super::store::WorkflowStore;
use crate::config::WorkflowConfig;

fn enabled() -> WorkflowConfig {
    WorkflowConfig {
        enabled: true,
        autospec_enabled: true,
        ..Default::default()
    }
}

#[test]
fn disabled_store_performs_no_registry_io_or_registration() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("state.json");
    std::fs::write(&path, b"invalid private state").unwrap();
    let store = WorkflowStore::open(path.clone(), WorkflowConfig::default()).unwrap();
    assert!(store.snapshots(100).unwrap().is_empty());
    assert!(
        store
            .register(
                "owner",
                ObserveInput {
                    working_dir: root.path().into(),
                    tasks_file: "tasks.yaml".into(),
                    status_file: None,
                    label: None,
                },
                100
            )
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"invalid private state");
}

#[test]
fn store_reconnect_is_owned_and_failed_save_does_not_claim_worktree() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("private/state.json");
    let store = WorkflowStore::open(path.clone(), enabled()).unwrap();
    let input = || ObserveInput {
        working_dir: root.path().into(),
        tasks_file: "tasks.yaml".into(),
        status_file: None,
        label: None,
    };
    let id = store.register("owner", input(), 100).unwrap();
    assert!(!store.unobserve("intruder", &id).unwrap());
    drop(store);
    let restored = WorkflowStore::open(path.clone(), enabled()).unwrap();
    assert_eq!(restored.register("owner", input(), 110).unwrap(), id);
    let snapshots = restored.snapshots(120).unwrap();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].0, "owner");
    assert_eq!(snapshots[0].1.id, id);
    assert!(restored.unobserve("owner", &id).unwrap());
    assert!(restored.snapshots(121).unwrap().is_empty());
    // Replace only this test's private target with a directory to force atomic save failure.
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(restored.register("owner", input(), 130).is_err());
    assert!(restored.snapshots(131).unwrap().is_empty());
}

#[test]
fn snapshot_persistence_failure_is_visible_and_retries_without_source_changes() {
    use crate::bus::WorkflowHealth;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("private/state.json");
    let store = WorkflowStore::open(path.clone(), enabled()).unwrap();
    store
        .register(
            "owner",
            ObserveInput {
                working_dir: root.path().into(),
                tasks_file: "tasks.yaml".into(),
                status_file: None,
                label: None,
            },
            100,
        )
        .unwrap();
    std::fs::write(root.path().join("tasks.yaml"), "phases:\n- number: 1\n  tasks:\n  - id: T001\n    title: Validate\n    status: InProgress\n").unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    let failed = store.snapshots(110).unwrap();
    assert_eq!(failed[0].1.total, Some(1));
    assert_eq!(failed[0].1.health, WorkflowHealth::ObserverError);
    assert!(
        failed[0]
            .1
            .detail
            .as_deref()
            .unwrap()
            .contains("persistence failed")
    );
    std::fs::remove_dir(&path).unwrap();
    let recovered = store.snapshots(111).unwrap();
    assert_eq!(recovered[0].1.health, WorkflowHealth::Running);
    drop(store);
    let restored = WorkflowStore::open(path, enabled()).unwrap();
    assert_eq!(restored.snapshots(112).unwrap()[0].1.total, Some(1));
}

#[test]
fn workflow_registry_has_one_writer_across_store_instances() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("private/state.json");
    let first = WorkflowStore::open(path.clone(), enabled()).unwrap();
    assert!(WorkflowStore::open(path.clone(), enabled()).is_err());
    drop(first);
    assert!(WorkflowStore::open(path, enabled()).is_ok());
}
