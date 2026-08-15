use super::Action;
use super::session::{
    AttachRequest, DapSessionManager, LaunchRequest, prepare_breakpoint_state,
    request_launch_with_configuration,
};
use super::session::{SessionSnapshot, SessionStatus, append_output};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, split};

#[tokio::test]
async fn launch_requires_an_explicit_program_and_does_not_spawn_arbitrary_processes() {
    let manager = DapSessionManager::new();
    let error = manager
        .launch(LaunchRequest {
            adapter: Some("missing".to_owned()),
            program: PathBuf::new(),
            args: Vec::new(),
            cwd: PathBuf::from("/tmp"),
            parent_session_id: None,
        })
        .await
        .expect_err("empty launch target");
    assert!(error.to_string().contains("program"));
}

#[tokio::test]
async fn attach_is_a_separate_path_and_requires_an_endpoint_or_pid() {
    let manager = DapSessionManager::new();
    let error = manager
        .attach(AttachRequest {
            adapter: Some("missing".to_owned()),
            cwd: PathBuf::from("/tmp"),
            host: None,
            port: None,
            pid: None,
            parent_session_id: None,
        })
        .await
        .expect_err("missing attach target");
    assert!(error.to_string().contains("attach"));
}

#[test]
fn capability_requirements_are_explicit_for_optional_actions() {
    assert_eq!(
        Action::WriteMemory.required_capability(),
        Some("supportsWriteMemoryRequest")
    );
    assert_eq!(
        Action::Modules.required_capability(),
        Some("supportsModulesRequest")
    );
    assert_eq!(Action::StackTrace.required_capability(), None);
}

#[test]
fn output_bounds_never_split_utf8_codepoints() {
    let mut snapshot = SessionSnapshot {
        id: "test".into(),
        adapter: "fake".into(),
        cwd: PathBuf::from("/tmp"),
        program: None,
        status: SessionStatus::Running,
        stop_reason: None,
        output: String::new(),
        output_truncated: false,
        capabilities: BTreeMap::new(),
        parent_session_id: None,
        child_session_ids: Vec::new(),
        breakpoints: BTreeMap::new(),
    };

    append_output(&mut snapshot, "αβγδε", 5);
    assert!(snapshot.output.is_char_boundary(0));
    assert!(snapshot.output.is_char_boundary(snapshot.output.len()));
    assert!(snapshot.output.len() <= 5);
    assert!(snapshot.output_truncated);
}

#[test]
fn breakpoint_preparation_preserves_existing_state_until_commit() {
    let existing = BTreeMap::from([("main.rs".to_owned(), vec![json!({"line": 1})])]);
    let (arguments, (_, complete)) = prepare_breakpoint_state(
        Action::SetBreakpoint,
        &json!({"source": {"path": "main.rs"}, "breakpoints": [{"line": 2}]}),
        &existing,
    )
    .expect("breakpoint arguments");

    assert_eq!(existing["main.rs"].len(), 1);
    assert_eq!(complete.len(), 2);
    assert_eq!(arguments["breakpoints"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn launch_sends_configuration_done_when_adapter_emits_initialized_first() {
    let (client_stream, adapter_stream) = tokio::io::duplex(4096);
    let (client_reader, client_writer) = split(client_stream);
    let client = super::transport::DapClient::from_stream(client_reader, client_writer).await;
    let (mut adapter_reader, mut adapter_writer) = split(adapter_stream);

    tokio::spawn(async move {
        let mut codec = super::transport::FrameCodec::default();
        let mut bytes = [0_u8; 4096];
        let mut launch_request = None;
        loop {
            let count = adapter_reader.read(&mut bytes).await.expect("request read");
            assert!(count > 0, "client closed before configurationDone");
            for payload in codec.push(&bytes[..count]).expect("request frame") {
                let request: serde_json::Value =
                    serde_json::from_slice(&payload).expect("request json");
                match request["command"].as_str() {
                    Some("launch") => {
                        launch_request = Some(request["seq"].clone());
                        let initialized = json!({
                            "seq": 2,
                            "type": "event",
                            "event": "initialized"
                        });
                        adapter_writer
                            .write_all(&frame(&initialized.to_string()))
                            .await
                            .expect("initialized event");
                    }
                    Some("configurationDone") => {
                        let response = json!({
                            "seq": 3,
                            "type": "response",
                            "request_seq": request["seq"],
                            "success": true,
                            "command": "configurationDone"
                        });
                        adapter_writer
                            .write_all(&frame(&response.to_string()))
                            .await
                            .expect("configurationDone response");
                        let response = json!({
                            "seq": 4,
                            "type": "response",
                            "request_seq": launch_request.expect("launch request"),
                            "success": true,
                            "command": "launch"
                        });
                        adapter_writer
                            .write_all(&frame(&response.to_string()))
                            .await
                            .expect("launch response");
                        return;
                    }
                    _ => {}
                }
            }
        }
    });

    let (response, configuration_done_sent) = request_launch_with_configuration(
        client,
        json!({"request": "launch"}),
        Duration::from_secs(1),
        true,
    )
    .await
    .expect("launch completes after configurationDone");
    assert!(response.success);
    assert!(configuration_done_sent);
}

fn frame(body: &str) -> Vec<u8> {
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body).into_bytes()
}
