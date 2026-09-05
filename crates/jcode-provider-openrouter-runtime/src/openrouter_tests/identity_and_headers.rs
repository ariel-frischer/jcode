/// A named OpenAI-compatible profile keeps the stable machine-facing
/// `Provider::name()` and surfaces its identity through `display_name()`.
///
/// Issue #691 proposed returning `profile_id` from `name()`. That would regress
/// the contract documented on the trait and settled in #329: billing, routing,
/// and provider-class matching key off `name()`, so it must stay constant for a
/// provider class, while user-visible labels come from `display_name()`. This
/// pins both halves so the split cannot be undone by accident.
#[test]
fn named_openai_compatible_provider_keeps_stable_name_and_profile_display_name() {
    let _lock = ENV_LOCK.lock();
    let _namespace = EnvVarGuard::remove("JCODE_OPENROUTER_CACHE_NAMESPACE");

    let profile = jcode_base::config::NamedProviderConfig {
        base_url: "https://llm.example.com/v1".to_string(),
        auth: jcode_base::config::NamedProviderAuth::None,
        default_model: Some("example-model".to_string()),
        ..Default::default()
    };

    let provider = OpenRouterProvider::new_named_openai_compatible("example-compat", &profile)
        .expect("named profile should initialize");

    // Machine-facing identity: stable per provider class.
    assert_eq!(
        Provider::name(&provider),
        "openrouter",
        "billing/routing key off name(); it must not become the profile id"
    );
    // User-facing identity: the profile the user configured.
    assert_eq!(provider.runtime_display_name(), "example-compat");
    assert_eq!(Provider::display_name(&provider), "example-compat");
}

/// Issue #1167: OpenCode Go/Zen require a stable per-conversation
/// `x-opencode-session` header; other OpenAI-compatible hosts must not get it.
#[test]
fn opencode_session_header_only_for_opencode_hosts() {
    assert!(is_opencode_api_base("https://opencode.ai/zen/go/v1"));
    assert!(is_opencode_api_base("https://opencode.ai/zen/v1"));
    assert!(is_opencode_api_base("https://api.opencode.ai/v1"));
    assert!(!is_opencode_api_base("https://openrouter.ai/api/v1"));
    assert!(!is_opencode_api_base("https://api.deepseek.com/v1"));
    assert!(!is_opencode_api_base("not a url"));

    let client = reqwest::Client::new();
    let req = apply_opencode_session_header(
        client.post("https://opencode.ai/zen/go/v1/chat/completions"),
        "https://opencode.ai/zen/go/v1",
        "conv-123",
    )
    .build()
    .unwrap();
    assert_eq!(
        req.headers()
            .get(OPENCODE_SESSION_HEADER)
            .and_then(|v| v.to_str().ok()),
        Some("conv-123")
    );

    let req = apply_opencode_session_header(
        client.post("https://openrouter.ai/api/v1/chat/completions"),
        "https://openrouter.ai/api/v1",
        "conv-123",
    )
    .build()
    .unwrap();
    assert!(req.headers().get(OPENCODE_SESSION_HEADER).is_none());
}

#[test]
fn opencode_session_ids_are_uuids_and_unique() {
    let a = new_conversation_id();
    let b = new_conversation_id();
    assert_ne!(a, b);
    assert!(uuid::Uuid::parse_str(&a).is_ok());
}

/// Wire-level check for issue #1167: a real `chat/completions` request whose
/// api_base host is `opencode.ai` carries `x-opencode-session`, and a
/// request to another host does not. The DNS override points the hostname at
/// a local listener, so the full stream path (including retries) is exercised.
fn spawn_header_capturing_server() -> (std::net::SocketAddr, std::sync::mpsc::Receiver<String>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("read timeout");
        let mut buf = vec![0u8; 65536];
        let n = stream.read(&mut buf).unwrap_or(0);
        let _ = tx.send(String::from_utf8_lossy(&buf[..n]).to_string());
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(response.as_bytes());
    });
    (addr, rx)
}

fn captured_request_for_host(host: &str, conversation_id: &str) -> String {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    rt.block_on(async {
        let (addr, rx) = spawn_header_capturing_server();
        let client = reqwest::Client::builder()
            .resolve(host, addr)
            .build()
            .expect("client");
        let api_base = format!("http://{host}:{}/zen/go/v1", addr.port());
        let (tx, mut events) = tokio::sync::mpsc::channel::<anyhow::Result<StreamEvent>>(64);
        super::openrouter_sse_stream::run_stream_with_retries(
            client,
            api_base,
            ProviderAuth::None {
                label: "test".to_string(),
            },
            false,
            conversation_id.to_string(),
            serde_json::json!({"model": "m", "messages": [], "stream": true}),
            tx,
            Arc::new(Mutex::new(None)),
            "m".to_string(),
        )
        .await;
        while events.recv().await.is_some() {}
        rx.recv_timeout(Duration::from_secs(5))
            .expect("server captured request")
    })
}

#[test]
fn opencode_session_header_is_sent_on_the_wire_only_to_opencode_hosts() {
    let raw = captured_request_for_host("opencode.ai", "conv-wire-1167").to_ascii_lowercase();
    assert!(
        raw.contains("x-opencode-session: conv-wire-1167"),
        "opencode.ai request lacked the header:\n{raw}"
    );

    let raw = captured_request_for_host("example.test", "conv-wire-1167").to_ascii_lowercase();
    assert!(
        !raw.contains("x-opencode-session"),
        "non-opencode host received the header:\n{raw}"
    );
}
