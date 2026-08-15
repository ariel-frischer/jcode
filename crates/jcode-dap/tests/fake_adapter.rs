use jcode_dap::{
    Action, AdapterConfig, AdapterRegistry, DapPolicy, DapSessionManager, LaunchRequest,
    SessionStatus,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use tempfile::tempdir;

#[tokio::test]
async fn launches_configured_stdio_adapter_and_reaps_it() {
    let python_available = Command::new("python3")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !python_available {
        return;
    }

    let temp = tempdir().expect("temp directory");
    let program = temp.path().join("main.py");
    std::fs::write(&program, "print('debug')\n").expect("program");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fake_adapter.py");
    let mut adapters = BTreeMap::new();
    adapters.insert(
        "fake".to_owned(),
        AdapterConfig {
            command: "python3".to_owned(),
            args: vec![fixture.to_string_lossy().into_owned()],
            file_types: vec![".py".to_owned()],
            ..AdapterConfig::default()
        },
    );
    let registry = AdapterRegistry::from_toml_layers(adapters, &[]).expect("registry");
    let policy = DapPolicy {
        allow_memory_write: true,
        ..DapPolicy::default()
    };
    let manager = DapSessionManager::with_registry(policy, registry);

    let snapshot = manager
        .launch(LaunchRequest {
            adapter: Some("fake".to_owned()),
            program,
            args: Vec::new(),
            cwd: temp.path().to_path_buf(),
            parent_session_id: None,
        })
        .await
        .expect("launch");
    assert_eq!(snapshot.adapter, "fake");

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let output = loop {
        let output = manager
            .execute(&snapshot.id, Action::Output, json!({}), None)
            .await
            .expect("output");
        if output["status"] == json!(SessionStatus::Stopped)
            && output["output"]
                .as_str()
                .unwrap_or_default()
                .contains("fake adapter")
        {
            break output;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "adapter events not observed: {output}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert!(
        output["output"]
            .as_str()
            .unwrap_or_default()
            .contains("fake adapter")
    );
    assert_eq!(output["status"], json!(SessionStatus::Stopped));

    let write_error = manager
        .execute(&snapshot.id, Action::WriteMemory, json!({}), None)
        .await
        .expect_err("unsupported memory write");
    assert!(
        write_error
            .to_string()
            .contains("supportsWriteMemoryRequest")
    );

    let threads = manager
        .execute(&snapshot.id, Action::Threads, json!({}), None)
        .await
        .expect("threads");
    assert_eq!(threads["threads"][0]["name"], "fake");

    manager
        .execute(
            &snapshot.id,
            Action::SetBreakpoint,
            json!({"source": {"path": "main.py"}, "breakpoints": [{"line": 1}]}),
            None,
        )
        .await
        .expect("breakpoint");
    assert_eq!(
        manager
            .get(&snapshot.id)
            .await
            .expect("snapshot")
            .breakpoints["main.py"]
            .len(),
        1
    );
    manager
        .execute(
            &snapshot.id,
            Action::SetBreakpoint,
            json!({"source": {"path": "main.py"}, "breakpoints": [{"line": 2}]}),
            None,
        )
        .await
        .expect("second breakpoint");
    assert_eq!(
        manager
            .get(&snapshot.id)
            .await
            .expect("snapshot")
            .breakpoints["main.py"]
            .len(),
        2
    );
    manager
        .execute(
            &snapshot.id,
            Action::RemoveBreakpoint,
            json!({"source": {"path": "main.py"}, "breakpoints": [{"line": 1}]}),
            None,
        )
        .await
        .expect("remove breakpoint");
    assert_eq!(
        manager
            .get(&snapshot.id)
            .await
            .expect("snapshot")
            .breakpoints["main.py"][0]["line"],
        2
    );
    manager
        .execute(&snapshot.id, Action::StepOver, json!({"threadId": 1}), None)
        .await
        .expect("step");

    let attached = manager
        .attach(jcode_dap::AttachRequest {
            adapter: Some("fake".to_owned()),
            cwd: temp.path().to_path_buf(),
            host: None,
            port: None,
            pid: Some(1234),
            parent_session_id: Some(snapshot.id.clone()),
        })
        .await
        .expect("attach");
    let parent = manager.get(&snapshot.id).await.expect("parent session");
    assert!(parent.child_session_ids.contains(&attached.id));
    manager.disconnect(&attached.id).await.expect("disconnect");

    assert_eq!(manager.reap_idle(Duration::ZERO).await, 1);
    assert!(manager.list().await.is_empty());
}
