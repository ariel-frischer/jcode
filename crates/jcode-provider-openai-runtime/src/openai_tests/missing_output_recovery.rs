// End-to-end, loopback-only tests of complete()'s persistent-to-fresh path.
const MISSING_TOOL_OUTPUT: &str = "No tool output found for function call call_saved.";

async fn missing_output_scenario(
    history: Vec<ChatMessage>,
    before_error: Vec<Value>,
    rejection: &str,
    reject_fresh: bool,
) -> (Vec<Result<StreamEvent>>, Vec<Value>, Option<String>) {
    missing_output_scenario_with_codes(history, before_error, rejection, reject_fresh, [None, None])
        .await
}

async fn missing_output_scenario_with_codes(
    history: Vec<ChatMessage>,
    before_error: Vec<Value>,
    rejection: &str,
    reject_fresh: bool,
    error_codes: [Option<&str>; 2],
) -> (Vec<Result<StreamEvent>>, Vec<Value>, Option<String>) {
    // Match main.rs. Fresh websocket setup initializes Rustls even for ws://.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let home = tempfile::tempdir().unwrap();
    let _home = EnvVarGuard::set_path("HOME", home.path());
    let _jcode_home = EnvVarGuard::set_path("JCODE_HOME", home.path());
    let _codex_home = EnvVarGuard::set_path("CODEX_HOME", home.path());
    let _config_home = EnvVarGuard::set_path("XDG_CONFIG_HOME", home.path());
    // A native agent's inherited OAuth route causes new() to replace supplied
    // credentials from disk. Never let it override this loopback-only fixture.
    let _route = EnvVarGuard::remove("JCODE_RUNTIME_PROVIDER");
    let _model = EnvVarGuard::set("JCODE_OPENAI_MODEL", "gpt-5.6-sol");
    jcode_base::provider::populate_account_models(vec!["gpt-5.6-sol".to_string()]);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _base = EnvVarGuard::set("JCODE_OPENAI_API_BASE", &format!("http://{addr}"));
    let rejection = rejection.to_string();
    let error_codes = error_codes.map(|code| code.map(str::to_string));
    let server =
        tokio::spawn(async move {
            let mut requests = Vec::new();
            for (attempt, error_code) in error_codes.iter().enumerate() {
                let accepted =
                    tokio::time::timeout(Duration::from_secs(1), listener.accept()).await;
                let Ok(Ok((stream, _))) = accepted else { break };
                let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
                let request = ws.next().await.unwrap().unwrap().into_text().unwrap();
                requests.push(serde_json::from_str::<Value>(&request).unwrap());
                if attempt == 0 {
                    for event in &before_error {
                        ws.send(WsMessage::Text(event.to_string())).await.unwrap();
                    }
                }
                if attempt == 0 || reject_fresh {
                    ws.send(WsMessage::Text(
                        serde_json::json!({
                            "type": "error", "error": {
                                "type": "invalid_request_error", "message": rejection,
                        "code": error_code, "retry_after": 17
                            }
                        })
                        .to_string(),
                    ))
                    .await
                    .unwrap();
                    // A rejected response closes without a completion marker.
                    // The runtime must not forward Error then replay again on EOF.
                    continue;
                } else {
                    ws.send(WsMessage::Text(
                        serde_json::json!({
                            "type": "response.created", "response": {"id": "resp_fresh"}
                        })
                        .to_string(),
                    ))
                    .await
                    .unwrap();
                    ws.send(WsMessage::Text(
                        serde_json::json!({
                            "type": "response.output_text.delta", "delta": "recovered"
                        })
                        .to_string(),
                    ))
                    .await
                    .unwrap();
                }
                let _ = ws.send(WsMessage::Text(serde_json::json!({
                "type": "response.completed", "response": {"id": "resp_fresh", "output": []}
            }).to_string())).await;
            }
            // No third request, HTTPS fallback, or repeated chain recovery is allowed.
            assert!(
                tokio::time::timeout(Duration::from_millis(100), listener.accept())
                    .await
                    .is_err()
            );
            requests
        });
    let provider = OpenAIProvider::new(CodexCredentials {
        access_token: "loopback-only".into(),
        refresh_token: String::new(),
        id_token: None,
        account_id: None,
        expires_at: None,
    });
    {
        let credentials = provider.credentials.read().await;
        // Use boolean assertions, never failure output that could print a key.
        assert!(credentials.access_token == "loopback-only");
        assert!(credentials.refresh_token.is_empty());
        assert!(credentials.id_token.is_none());
        assert!(credentials.expires_at.is_none());
        assert!(OpenAIProvider::responses_ws_url(&credentials) == format!("ws://{addr}/responses"));
    }
    *provider.transport_mode.write().await = OpenAITransportMode::WebSocket;
    let input = build_responses_input(&history);
    let (ws_stream, _) = connect_async(format!("ws://{addr}")).await.unwrap();
    *provider.persistent_ws.lock().await = Some(PersistentWsState {
        ws_stream,
        last_response_id: "resp_stale".into(),
        connected_at: Instant::now(),
        last_activity_at: Instant::now(),
        last_response_completed_at: Instant::now(),
        message_count: 1,
        // Reproduce prefix mutation: the saved result is now BEFORE the old
        // count boundary, so count-only continuation sends just two messages.
        last_input_item_count: input.len() - 2,
    });
    let events = tokio::time::timeout(Duration::from_secs(5), async {
        provider
            .complete(&history, &[], "test", None)
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await
    })
    .await
    .expect("bounded provider completion");
    let response_id = provider
        .persistent_ws
        .lock()
        .await
        .as_ref()
        .map(|state| state.last_response_id.clone());
    (events, server.await.unwrap(), response_id)
}

fn recovery_history() -> Vec<ChatMessage> {
    vec![
        assistant_tool_use(
            "call_saved",
            "bash",
            serde_json::json!({"command": "never execute this"}),
        ),
        ChatMessage::tool_result("call_saved", "saved result", false),
        user_text("changed prefix"),
        user_text("continue"),
    ]
}

fn terminal_errors(events: &[Result<StreamEvent>]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            Err(error) => Some(error.to_string()),
            Ok(StreamEvent::Error { message, .. }) => Some(message.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn missing_output_recovers_omitted_saved_result_with_one_fresh_request() {
    let _guard = jcode_base::storage::lock_test_env();
    let (events, requests, response_id) =
        missing_output_scenario(recovery_history(), vec![], MISSING_TOOL_OUTPUT, false).await;
    assert_eq!(response_id.as_deref(), Some("resp_fresh"));
    assert!(
        terminal_errors(&events).is_empty(),
        "internal recovery must not surface an error to auto-poke: {:?}",
        terminal_errors(&events)
    );
    assert_eq!(requests.len(), 2, "one delta then one fresh replay");
    assert_eq!(requests[0]["previous_response_id"], "resp_stale");
    assert_eq!(requests[0]["input"].as_array().unwrap().len(), 2);
    assert!(
        requests[0]["input"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["type"] == "message")
    );
    assert!(requests[1].get("previous_response_id").is_none());
    assert_eq!(
        requests[1]["input"],
        serde_json::json!(build_responses_input(&recovery_history()))
    );
    assert_eq!(
        function_call_outputs(requests[1]["input"].as_array().unwrap(), "call_saved"),
        vec!["saved result"]
    );
    assert_eq!(
        events
            .iter()
            .filter(
                |event| matches!(event, Ok(StreamEvent::TextDelta(text)) if text == "recovered")
            )
            .count(),
        1
    );
    assert!(!events.iter().any(|event| matches!(
        event,
        Ok(StreamEvent::RetryRollback { .. } | StreamEvent::ToolUseStart { .. })
    )));
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn missing_output_without_saved_result_is_terminal_even_with_synthetic_placeholder() {
    let _guard = jcode_base::storage::lock_test_env();
    let mut history = recovery_history();
    history.remove(1);
    assert!(
        function_call_outputs(&build_responses_input(&history), "call_saved")[0]
            .contains(TOOL_OUTPUT_MISSING_TEXT)
    );
    let (events, requests, cleared) =
        missing_output_scenario(history, vec![], MISSING_TOOL_OUTPUT, false).await;
    assert_eq!(requests.len(), 1);
    assert!(cleared.is_none());
    assert_eq!(terminal_errors(&events).len(), 1);
    assert!(terminal_errors(&events)[0].contains("saved tool call and result"));
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn missing_output_after_visible_output_is_terminal_without_rollback() {
    let _guard = jcode_base::storage::lock_test_env();
    for visible in [
        serde_json::json!({"type": "response.output_text.delta", "delta": "partial"}),
        serde_json::json!({"type": "response.reasoning_summary_text.delta", "delta": "thinking"}),
        serde_json::json!({"type": "response.output_item.done", "output_index": 0, "item": {"type": "function_call", "id": "fc_new", "call_id": "call_new", "name": "bash", "arguments": "{}"}}),
    ] {
        let (events, requests, cleared) = missing_output_scenario(
            recovery_history(),
            vec![visible],
            MISSING_TOOL_OUTPUT,
            false,
        )
        .await;
        assert_eq!(requests.len(), 1);
        assert!(cleared.is_none());
        assert_eq!(terminal_errors(&events).len(), 1);
        assert!(terminal_errors(&events)[0].contains("output was already emitted"));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, Ok(StreamEvent::RetryRollback { .. })))
        );
    }
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn missing_output_repeated_rejection_is_terminal() {
    let _guard = jcode_base::storage::lock_test_env();
    let (events, requests, cleared) =
        missing_output_scenario(recovery_history(), vec![], MISSING_TOOL_OUTPUT, true).await;
    assert_eq!(requests.len(), 2);
    assert!(cleared.is_none());
    assert_eq!(terminal_errors(&events).len(), 1);
    assert!(terminal_errors(&events)[0].contains("full-history recovery failed"));
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn missing_output_recovery_does_not_retry_unrelated_invalid_request() {
    let _guard = jcode_base::storage::lock_test_env();
    let (events, requests, _) = missing_output_scenario(
        recovery_history(),
        vec![],
        "Invalid value for reasoning.effort",
        false,
    )
    .await;
    assert_eq!(requests.len(), 1);
    assert_eq!(terminal_errors(&events).len(), 1);
    assert!(!is_retryable_error(&MISSING_TOOL_OUTPUT.to_lowercase()));
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn missing_output_without_saved_call_or_matching_id_is_terminal() {
    let _guard = jcode_base::storage::lock_test_env();
    let mut no_call = recovery_history();
    no_call.remove(0);
    for (history, rejection) in [
        (no_call, MISSING_TOOL_OUTPUT),
        (
            recovery_history(),
            "No tool output found for function call call_other.",
        ),
        (
            recovery_history(),
            "No tool output found for function call .",
        ),
        (
            recovery_history(),
            "No tool output found for function call call_saved extra text",
        ),
    ] {
        let (events, requests, cleared) =
            missing_output_scenario(history, vec![], rejection, false).await;
        assert_eq!(requests.len(), 1);
        assert!(cleared.is_none());
        assert_eq!(terminal_errors(&events).len(), 1);
    }
}

#[test]
fn missing_output_classifier_is_narrow_and_preserves_call_id_case() {
    use super::openai_stream_runtime::missing_tool_output_call_id;
    for message in [
        "No tool output found for function call call_ABC-12",
        "invalid_request_error: No tool output found for function call call_ABC-12.",
    ] {
        assert_eq!(missing_tool_output_call_id(message), Some("call_ABC-12"));
    }
    for message in [
        "Invalid reasoning effort",
        "invalid_request_error arbitrary text: No tool output found for function call call_1.",
        "No tool output found for function call ",
        "No tool output found for function call .",
        "No tool output found for function call call_1; retry",
        "server_error: No tool output found for function call call_1.",
        "Message quoted No tool output found for function call call_1.",
    ] {
        assert_eq!(missing_tool_output_call_id(message), None, "{message}");
    }
}

#[test]
fn missing_output_replay_requires_one_serialized_call_and_output() {
    use super::openai_stream_runtime::{has_replayable_tool_pair, saved_tool_result_ids};
    let history = recovery_history();
    let input = build_responses_input(&history);
    assert!(saved_tool_result_ids(&history).contains("call_saved"));
    assert!(has_replayable_tool_pair(&input, "call_saved"));
    assert!(!has_replayable_tool_pair(&input, "call_other"));
    assert!(!has_replayable_tool_pair(&input[1..], "call_saved"));
    let mut duplicate = input.clone();
    duplicate.push(input[1].clone());
    assert!(!has_replayable_tool_pair(&duplicate, "call_saved"));
    let mut malformed = input;
    malformed[1]["output"] = serde_json::Value::Null;
    assert!(!has_replayable_tool_pair(&malformed, "call_saved"));
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn missing_output_terminal_errors_preserve_structured_metadata() {
    let _guard = jcode_base::storage::lock_test_env();
    for (message, reject_fresh, codes, request_count) in [
        (
            "Invalid request",
            false,
            [Some("service_unavailable_error"), None],
            1,
        ),
        (
            MISSING_TOOL_OUTPUT,
            true,
            [None, Some("service_unavailable_error")],
            2,
        ),
    ] {
        let (events, requests, response_id) = missing_output_scenario_with_codes(
            recovery_history(),
            vec![],
            message,
            reject_fresh,
            codes,
        )
        .await;
        assert_eq!(requests.len(), request_count);
        assert!(response_id.is_none());
        assert_eq!(terminal_errors(&events).len(), 1);
        assert!(
            events.iter().any(|event| matches!(
                event,
                Ok(StreamEvent::Error {
                    provider_code: Some(
                        jcode_message_types::ProviderFailureCode::TemporarilyUnavailable
                    ),
                    retry_after_secs: Some(17),
                    ..
                })
            )),
            "terminal errors must retain provider code and retry-after metadata"
        );
    }
}
