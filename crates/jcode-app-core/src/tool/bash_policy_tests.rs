use super::*;

#[test]
fn bash_execution_policy_separates_soft_yield_from_hard_timeout() {
    let defaults = BashExecutionPolicy::resolve(None, None, false);
    assert_eq!(defaults.soft_yield, Some(Duration::from_millis(10_000)));
    assert_eq!(defaults.hard_timeout, None);
    assert!(!defaults.run_in_background);

    let disabled = BashExecutionPolicy::resolve(Some(0), None, false);
    assert_eq!(disabled.soft_yield, None);
    assert_eq!(disabled.hard_timeout, None);

    let bounded = BashExecutionPolicy::resolve(Some(250), Some(3_600_000), true);
    assert_eq!(bounded.soft_yield, None);
    assert_eq!(bounded.hard_timeout, Some(Duration::from_millis(1_800_000)));
    assert!(bounded.run_in_background);
}

#[test]
fn bash_input_accepts_optional_policy_fields_and_rejects_invalid_types() {
    let omitted: BashInput = serde_json::from_value(json!({"command": "true"})).unwrap();
    assert_eq!(omitted.soft_yield_ms, None);
    assert_eq!(omitted.timeout, None);
    assert_eq!(omitted.wake, None);

    let configured: BashInput = serde_json::from_value(json!({
        "command": "true",
        "soft_yield_ms": 0,
        "timeout": 1_800_000,
        "run_in_background": true,
        "wake": false
    }))
    .unwrap();
    assert_eq!(configured.soft_yield_ms, Some(0));
    assert_eq!(configured.timeout, Some(1_800_000));
    assert_eq!(configured.run_in_background, Some(true));
    assert_eq!(configured.wake, Some(false));

    assert!(
        serde_json::from_value::<BashInput>(json!({
            "command": "true",
            "soft_yield_ms": "soon"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<BashInput>(json!({
            "command": "true",
            "timeout": "later"
        }))
        .is_err()
    );
}

#[test]
fn bash_schema_documents_distinct_execution_modes() {
    let schema = BashTool::new().parameters_schema();
    let properties = schema["properties"].as_object().expect("schema properties");
    let soft_yield = properties["soft_yield_ms"]["description"]
        .as_str()
        .expect("soft-yield description");
    let timeout = properties["timeout"]["description"]
        .as_str()
        .expect("timeout description");
    let background = properties["run_in_background"]["description"]
        .as_str()
        .expect("background description");

    assert!(soft_yield.contains("MILLISECONDS"));
    assert!(soft_yield.contains("10000"));
    assert!(soft_yield.contains("0 disables"));
    assert!(soft_yield.contains("does not terminate"));
    assert!(timeout.contains("1800000"));
    assert!(timeout.contains("exit 124"));
    assert!(timeout.contains("Omit for no timeout"));
    assert!(!timeout.contains("background"));
    assert!(background.contains("immediately"));
}

#[tokio::test]
async fn zero_soft_yield_disables_automatic_backgrounding() {
    let result = BashTool::new()
        .execute(
            json!({"command": "sleep 0.15; echo soft_yield_disabled_ok", "soft_yield_ms": 0}),
            make_ctx(None),
        )
        .await
        .expect("disabled soft yield should await direct completion");
    assert!(result.metadata.is_none());
    assert!(result.output.contains("soft_yield_disabled_ok"));
}

#[tokio::test]
async fn foreground_hard_timeout_returns_124_without_backgrounding() {
    let result = BashTool::new()
        .execute(
            json!({"command": "sleep 5; echo should_not_print", "soft_yield_ms": 0, "timeout": 100}),
            make_ctx(None),
        )
        .await
        .expect("hard timeout should return a terminal command result");
    let metadata = result.metadata.expect("hard-timeout metadata");
    assert_eq!(metadata["exit_code"], 124);
    assert_eq!(metadata["timed_out"], true);
    assert!(result.output.contains("timed out after 100ms"));
    assert!(result.output.contains("Exit code: 124"));
    assert!(!result.output.contains("should_not_print"));
}

#[tokio::test]
async fn hard_timeout_remains_terminal_after_soft_yield() {
    let result = BashTool::new()
        .execute(
            json!({"command": "sleep 5; echo should_not_print", "soft_yield_ms": 50, "timeout": 100}),
            make_ctx(None),
        )
        .await
        .expect("command should soft-yield before its hard timeout");
    let metadata = result.metadata.expect("soft-yield metadata");
    let task_id = metadata["task_id"].as_str().expect("task id");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let status = crate::background::global()
            .status(task_id)
            .await
            .expect("adopted task status");
        if status.status != BackgroundTaskStatus::Running {
            assert_eq!(status.status, BackgroundTaskStatus::Failed);
            assert_eq!(status.exit_code, Some(124));
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
