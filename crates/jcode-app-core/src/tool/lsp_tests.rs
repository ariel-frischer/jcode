use super::lsp::LspTool;
use super::{Tool, ToolContext, ToolExecutionMode};
use jcode_lsp::LspSessionManager;
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
fn lsp_tool_exposes_bounded_language_actions() {
    let tool = LspTool::new(Arc::new(LspSessionManager::new()));
    let schema = tool.parameters_schema();
    let actions = schema["properties"]["action"]["enum"]
        .as_array()
        .expect("action enum");
    for action in [
        "diagnostics",
        "hover",
        "definition",
        "references",
        "symbols",
        "formatting",
        "rename",
        "code_actions",
        "feedback",
    ] {
        assert!(
            actions.iter().any(|value| value == action),
            "missing action {action}"
        );
    }
}

#[tokio::test]
async fn lsp_status_returns_structured_output_without_a_server() {
    let tool = LspTool::new(Arc::new(LspSessionManager::new()));
    let output = tool
        .execute(json!({"action": "status"}), context())
        .await
        .expect("status output");
    assert!(output.output.contains("sessions"));
    assert_eq!(output.metadata.expect("metadata")["action"], "status");
}

#[tokio::test]
async fn lsp_start_is_inactive_by_default() {
    let tool = LspTool::new(Arc::new(LspSessionManager::new()));
    let temp = tempfile::tempdir().expect("tempdir");
    let mut context = context();
    context.working_dir = Some(temp.path().to_path_buf());
    let output = tool
        .execute(json!({"action": "start", "file": "main.rs"}), context)
        .await
        .expect("default-off result");
    assert!(output.output.contains("no enabled language server"));
}

#[tokio::test]
async fn lsp_requires_working_directory_for_file_actions() {
    let tool = LspTool::new(Arc::new(LspSessionManager::new()));
    let mut context = context();
    context.working_dir = None;
    let error = tool
        .execute(json!({"action": "start", "file": "main.rs"}), context)
        .await
        .expect_err("working directory required");
    assert!(error.to_string().contains("working directory"));
}
