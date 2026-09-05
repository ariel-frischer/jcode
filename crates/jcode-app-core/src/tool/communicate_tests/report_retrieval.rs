#[test]
fn read_report_preserves_full_content_and_resolves_targets_safely() {
    let report = format!("{}\nTAIL-EVIDENCE", "🦢é".repeat(5000));
    let mut members = vec![AgentInfo {
        session_id: "worker-id".into(),
        friendly_name: Some("worker".into()),
        latest_completion_report: Some(report.clone()),
        ..Default::default()
    }];
    for target in ["worker-id", "worker"] {
        assert_eq!(
            super::read_member_report(target, &members).unwrap().output,
            report
        );
    }
    assert!(super::read_member_report("other-swarm-member", &members).is_err());
    members.push(AgentInfo {
        session_id: "second-id".into(),
        friendly_name: Some("worker".into()),
        ..Default::default()
    });
    assert!(
        super::read_member_report("worker", &members)
            .unwrap_err()
            .to_string()
            .contains("Ambiguous")
    );
    assert!(
        super::read_member_report("second-id", &members)
            .unwrap_err()
            .to_string()
            .contains("No completion report")
    );
    assert_eq!(
        super::read_member_report("worker-id", &members)
            .unwrap()
            .output,
        report
    );
    let schema = CommunicateTool::new().parameters_schema();
    assert!(
        schema["properties"]["action"]["enum"]
            .as_array()
            .unwrap()
            .contains(&json!("read_report"))
    );
}

#[tokio::test]
async fn read_report_roundtrips_full_report_through_private_server() {
    let _env_lock = crate::storage::lock_test_env();
    let runtime_dir = tempfile::TempDir::new().expect("runtime tempdir");
    let _home = EnvGuard::set("JCODE_HOME", runtime_dir.path());
    let repo_dir = std::env::current_dir().expect("repo cwd");
    let socket_path = runtime_dir.path().join("jcode.sock");
    let _runtime = EnvGuard::set("JCODE_RUNTIME_DIR", runtime_dir.path());
    let _socket = EnvGuard::set("JCODE_SOCKET", &socket_path);
    let _debug = EnvGuard::set("JCODE_DEBUG_CONTROL", "1");
    let provider: Arc<dyn Provider> = Arc::new(DelayedTestProvider {
        delay: Duration::ZERO,
    });
    let server = Arc::new(Server::new(provider));
    let mut server_task = tokio::spawn(async move { server.run().await });
    wait_for_server_socket(&socket_path, &mut server_task)
        .await
        .expect("socket ready");
    let mut reader = RawClient::connect(&socket_path).await.unwrap();
    reader.subscribe(&repo_dir).await.unwrap();
    let reader_id = reader.session_id().await.unwrap();
    let tool = CommunicateTool::new();
    let reader_ctx = test_ctx(&reader_id, &repo_dir);
    // Independent subscriptions are separate swarms. Create an idle child of
    // the reader using the in-process test provider, without a model turn.
    let spawned = tool
        .execute(
            json!({
                "action": "spawn", "label": "report-writer", "spawn_mode": "headless"
            }),
            reader_ctx.clone(),
        )
        .await
        .expect("spawn idle test child");
    let worker_id = spawned
        .output
        .strip_prefix("Spawned new agent: ")
        .expect("spawn result")
        .trim()
        .to_string();
    let worker_ctx = test_ctx(&worker_id, &repo_dir);
    let read_input = json!({"action": "read_report", "target_session": worker_id});
    assert!(
        tool.execute(read_input.clone(), reader_ctx.clone())
            .await
            .is_err()
    );
    let report = format!("{}\nTAIL-EVIDENCE", "🦢é".repeat(5000));
    tool.execute(
        json!({
            "action": "report", "status": "completed", "message": report,
            "tldr": "Long Unicode report regression"
        }),
        worker_ctx.clone(),
    )
    .await
    .expect("record full report");
    let output = tool
        .execute(read_input.clone(), reader_ctx.clone())
        .await
        .expect("read full report");
    assert_eq!(output.output, report);
    let alias = tool
        .execute(
            json!({"action": "read_report", "to_session": worker_id}),
            reader_ctx.clone(),
        )
        .await
        .unwrap();
    assert_eq!(alias.output, report);
    let outside_dir = tempfile::TempDir::new().expect("unrelated workspace");
    let mut outsider = RawClient::connect(&socket_path).await.unwrap();
    outsider.subscribe(outside_dir.path()).await.unwrap();
    let outsider_id = outsider.session_id().await.unwrap();
    assert!(
        tool.execute(
            read_input.clone(),
            test_ctx(&outsider_id, outside_dir.path())
        )
        .await
        .is_err()
    );
    assert!(
        tool.execute(
            json!({"action": "read_report", "target_session": "missing"}),
            reader_ctx.clone()
        )
        .await
        .is_err()
    );
    assert!(
        tool.execute(json!({"action": "read_report"}), reader_ctx.clone())
            .await
            .is_err()
    );
    // A short replacement is returned without stale content from the previous report.
    tool.execute(
        json!({"action": "report", "message": "short replacement"}),
        worker_ctx,
    )
    .await
    .unwrap();
    assert_eq!(
        tool.execute(read_input, reader_ctx).await.unwrap().output,
        "short replacement"
    );
    server_task.abort();
}
