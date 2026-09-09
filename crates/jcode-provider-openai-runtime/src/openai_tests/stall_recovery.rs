// Real native-provider requests to a loopback-only SSE fixture. No live credentials.
async fn stall_sse_scenario(
    enabled: bool,
    buffered_progress: bool,
    recover: bool,
) -> (Vec<Result<StreamEvent>>, usize) {
    stall_sse_scenario_with_idle(enabled, buffered_progress, recover, "180").await
}

async fn stall_sse_scenario_with_idle(
    enabled: bool,
    buffered_progress: bool,
    recover: bool,
    idle_secs: &str,
) -> (Vec<Result<StreamEvent>>, usize) {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    let _lock = jcode_base::storage::lock_test_env();
    let home = tempfile::tempdir().unwrap();
    let _home = EnvVarGuard::set_path("HOME", home.path());
    let _jcode_home = EnvVarGuard::set_path("JCODE_HOME", home.path());
    let _codex_home = EnvVarGuard::set_path("CODEX_HOME", home.path());
    let _config_home = EnvVarGuard::set_path("XDG_CONFIG_HOME", home.path());
    let _route = EnvVarGuard::remove("JCODE_RUNTIME_PROVIDER");
    let _model = EnvVarGuard::set("JCODE_OPENAI_MODEL", "gpt-5.6-sol");
    let _enabled = EnvVarGuard::set(
        "JCODE_OPENAI_STALL_RECOVERY",
        if enabled { "true" } else { "false" },
    );
    let _timeout = EnvVarGuard::set("JCODE_OPENAI_STALL_TIMEOUT_SECS", "1");
    let _idle = EnvVarGuard::set("JCODE_STREAM_IDLE_TIMEOUT_SECS", idle_secs);
    jcode_base::provider::populate_account_models(vec!["gpt-5.6-sol".into()]);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let _base = EnvVarGuard::set("JCODE_OPENAI_API_BASE", &format!("http://{address}"));
    // This test crate depends on jcode-base, where the production config cache
    // throttle is 500ms. Force the fixture's per-test environment overrides to
    // be observed even when adjacent serial tests ran moments earlier.
    jcode_base::config::invalidate_config_cache();
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = Arc::clone(&count);
    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let attempt = observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            tokio::spawn(async move {
                let mut stream = BufReader::new(stream);
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    if stream.read_line(&mut line).await.unwrap() == 0 {
                        return;
                    }
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse::<usize>().unwrap();
                    }
                }
                stream.read_exact(&mut vec![0; length]).await.unwrap();
                let mut stream = stream.into_inner();
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").await.unwrap();
                if recover && attempt > 0 {
                    let _ = stream.write_all(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"recovered\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"done\",\"output\":[]}}\n\n").await;
                    return;
                }
                if recover {
                    let _ = stream.write_all(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"discarded\"}\n\n").await;
                }
                if buffered_progress {
                    let _ = stream.write_all(b"data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"item\",\"type\":\"function_call\",\"call_id\":\"call\",\"name\":\"read\",\"arguments\":\"\"}}\n\n").await;
                }
                for _ in 0..16 {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    let frame = if buffered_progress {
                        b"data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"item\",\"delta\":\" \"}\n\n".as_slice()
                    } else {
                        b": keepalive\n\ndata: {\"type\":\"response.in_progress\"}\n\n".as_slice()
                    };
                    if stream.write_all(frame).await.is_err() {
                        return;
                    }
                }
                if buffered_progress {
                    let _ = stream.write_all(b"data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"item\",\"arguments\":\"{}\"}\n\n").await;
                }
                let _ = stream.write_all(b"data: {\"type\":\"response.completed\",\"response\":{\"id\":\"done\",\"output\":[]}}\n\n").await;
            });
        }
    });
    let provider = OpenAIProvider::new(CodexCredentials {
        access_token: "loopback-only".into(),
        refresh_token: String::new(),
        id_token: None,
        account_id: None,
        expires_at: None,
    });
    *provider.transport_mode.write().await = OpenAITransportMode::HTTPS;
    assert!(
        OpenAIProvider::responses_url(&*provider.credentials.read().await)
            .starts_with(&format!("http://{address}/"))
    );
    let events = tokio::time::timeout(Duration::from_secs(20), async {
        provider
            .complete(&[], &[], "offline test", None)
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await
    })
    .await;
    server.abort();
    let _ = server.await;
    (
        events.expect("bounded offline provider completion"),
        count.load(std::sync::atomic::Ordering::SeqCst),
    )
}

#[tokio::test]
async fn stall_recovery_retries_https_heartbeat_only_stream_and_rolls_back() {
    let (events, attempts) = stall_sse_scenario(true, false, true).await;
    assert_eq!(attempts, 2);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Ok(StreamEvent::RetryRollback { attempt: 2, max: 3 })))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Ok(StreamEvent::TextDelta(text)) if text == "recovered"))
    );
    assert!(!events.iter().any(Result::is_err));
}

#[tokio::test]
async fn stall_recovery_bounds_https_attempts_at_existing_cap() {
    let (events, attempts) = stall_sse_scenario(true, false, false).await;
    assert_eq!(attempts, 3);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Err(error) if error.to_string().contains("timed out")))
    );
}

#[tokio::test]
async fn stall_recovery_disabled_preserves_legacy_https_wait() {
    let (events, attempts) = stall_sse_scenario(false, false, false).await;
    assert_eq!(attempts, 1);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Ok(StreamEvent::MessageEnd { .. })))
    );
    assert!(!events.iter().any(Result::is_err));
}

#[tokio::test]
async fn stall_recovery_preserves_buffered_https_tool_argument_progress() {
    let (events, attempts) = stall_sse_scenario(true, true, false).await;
    assert_eq!(
        attempts, 1,
        "ongoing buffered tool arguments must refresh the deadline"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Ok(StreamEvent::ToolUseEnd)))
    );
    assert!(!events.iter().any(Result::is_err));
}

#[tokio::test]
async fn stall_recovery_buffered_progress_preserves_existing_idle_deadline() {
    for enabled in [false, true] {
        let (events, attempts) = stall_sse_scenario_with_idle(enabled, true, false, "1").await;
        assert_eq!(
            attempts, 3,
            "buffered progress must not extend the old event idle deadline (enabled={enabled})"
        );
        assert!(events.iter().any(Result::is_err));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, Ok(StreamEvent::MessageEnd { .. })))
        );
    }
}
