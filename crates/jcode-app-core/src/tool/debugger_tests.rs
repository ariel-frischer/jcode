use super::debugger::DebuggerTool;
use super::{Tool, ToolContext, ToolExecutionMode};
use jcode_dap::DapSessionManager;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

fn context() -> ToolContext {
    ToolContext {
        session_id: "session".to_owned(),
        message_id: "message".to_owned(),
        tool_call_id: "tool-call".to_owned(),
        working_dir: Some(PathBuf::from("/tmp")),
        stdin_request_tx: None,
        graceful_shutdown_signal: None,
        execution_mode: ToolExecutionMode::Direct,
    }
}

#[test]
fn debugger_tool_exposes_bounded_action_schema() {
    let tool = DebuggerTool::new(Arc::new(DapSessionManager::new()));
    let schema = tool.parameters_schema();
    let actions = schema["properties"]["action"]["enum"]
        .as_array()
        .expect("action enum");
    assert!(actions.iter().any(|action| action == "launch"));
    assert!(actions.iter().any(|action| action == "stack_trace"));
    assert!(actions.iter().any(|action| action == "write_memory"));
}

#[tokio::test]
async fn debugger_status_returns_structured_bounded_output() {
    let tool = DebuggerTool::new(Arc::new(DapSessionManager::new()));
    let output = tool
        .execute(json!({"action": "sessions"}), context())
        .await
        .expect("status output");
    assert!(output.output.contains("sessions"));
    assert!(output.metadata.expect("metadata")["action"] == "sessions");
}

#[tokio::test]
async fn debugger_rejects_launch_without_a_session_working_directory() {
    let tool = DebuggerTool::new(Arc::new(DapSessionManager::new()));
    let mut ctx = context();
    ctx.working_dir = None;
    let error = tool
        .execute(json!({"action": "launch", "program": "/bin/echo"}), ctx)
        .await
        .expect_err("working directory required");
    assert!(error.to_string().contains("working directory"));
}
