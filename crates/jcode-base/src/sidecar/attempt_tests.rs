use super::*;
use jcode_session_types::memory_usage::{
    MemoryOperationKind, MemoryRequestObservation, RequestOutcome,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

fn observed_sidecar() -> (Sidecar, mpsc::Receiver<MemoryRequestObservation>) {
    let (tx, rx) = mpsc::channel(32);
    let mut sidecar = Sidecar::with_openai_model("gpt-5.6-luna", Some("xhigh".into()));
    sidecar.client = reqwest::Client::builder().no_proxy().build().unwrap();
    sidecar.observation_tx = Some(tx);
    (
        sidecar.with_memory_operation(Some("session-a"), MemoryOperationKind::Rerank),
        rx,
    )
}

pub(super) async fn fixture(
    status: &str,
    body: &str,
    split: bool,
) -> (String, tokio::task::JoinHandle<serde_json::Value>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/responses", listener.local_addr().unwrap());
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        let header_end;
        loop {
            let mut chunk = [0; 1024];
            let n = stream.read(&mut chunk).await.unwrap();
            assert!(n > 0 && bytes.len() < 16384);
            bytes.extend_from_slice(&chunk[..n]);
            if let Some(pos) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                header_end = pos + 4;
                break;
            }
        }
        let headers = String::from_utf8_lossy(&bytes[..header_end]);
        let length: usize = headers
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().unwrap())
            })
            .unwrap();
        while bytes.len() < header_end + length {
            let mut chunk = [0; 1024];
            let n = stream.read(&mut chunk).await.unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&chunk[..n]);
        }
        let request = serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
        if split {
            for chunk in reply.as_bytes().chunks(7) {
                if let Err(error) = stream.write_all(chunk).await {
                    assert!(matches!(
                        error.kind(),
                        std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
                    ));
                    break;
                }
                tokio::task::yield_now().await;
            }
        } else {
            stream.write_all(reply.as_bytes()).await.unwrap();
        }
        request
    });
    (url, task)
}

async fn send(sidecar: &Sidecar, url: &str, streaming: bool, model: &str) -> bool {
    sidecar
        .complete_openai_with_model(
            url,
            "PRIVATE_FAKE_TOKEN",
            None,
            streaming,
            "PRIVATE_PROMPT",
            "PRIVATE_MEMORY",
            model,
            Some("xhigh"),
        )
        .await
        .is_ok()
}

#[tokio::test]
async fn physical_attempts_reconcile_votes_retry_fallback_and_payload() {
    let (sidecar, mut rx) = observed_sidecar();
    let mut ids = std::collections::HashSet::new();
    let mut total = 0;
    for (status, model) in [
        ("500 Error", "gpt-5.6-luna"),
        ("200 OK", "gpt-5.6-luna"),
        ("200 OK", "gpt-5.4"),
        ("200 OK", "gpt-5.6-luna"),
    ] {
        let body = r#"{"status":"completed","usage":{"input_tokens":10,"output_tokens":4,"output_tokens_details":{"reasoning_tokens":3}},"output":[]}"#;
        let (url, server) = fixture(status, body, false).await;
        assert_eq!(
            send(&sidecar.clone(), &url, false, model).await,
            status == "200 OK"
        );
        let payload = server.await.unwrap();
        assert_eq!(
            payload,
            build_openai_request(
                model,
                "PRIVATE_PROMPT",
                "PRIVATE_MEMORY",
                false,
                Some("xhigh")
            )
        );
        assert!(payload.get("max_output_tokens").is_none());
        let record = rx
            .try_recv()
            .expect("each physical send must produce one observation");
        assert!(ids.insert(record.request_id.clone()));
        assert_eq!(record.model, model);
        assert_eq!(record.context.session_id.as_deref(), Some("session-a"));
        assert_eq!(record.context.operation_kind, MemoryOperationKind::Rerank);
        record.validate().unwrap();
        total += record.usage.total_tokens().unwrap().unwrap_or(0);
        let json = serde_json::to_string(&record).unwrap();
        assert!(!json.contains("PRIVATE_"));
    }
    assert_eq!(ids.len(), 4);
    assert_eq!(total, 56);
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn split_duplicate_truncated_and_failed_streams_preserve_usage() {
    for (terminal, expected, success) in [
        ("response.completed", RequestOutcome::Success, true),
        ("response.incomplete", RequestOutcome::Incomplete, true),
        ("response.failed", RequestOutcome::Error, false),
        ("", RequestOutcome::Incomplete, true),
    ] {
        let (sidecar, mut rx) = observed_sidecar();
        let prefix = "data: not-json\n\ndata: {\"type\":\"response.created\",\"response\":{\"usage\":{\"input_tokens\":7}}}\n\n";
        let event = format!(
            "data: {{\"type\":\"{terminal}\",\"response\":{{\"usage\":{{\"input_tokens\":7,\"output_tokens\":2}}}}}}\n\n"
        );
        let body = if terminal.is_empty() {
            format!("{prefix}data: {{\"type\":")
        } else {
            format!("{prefix}{event}{event}")
        };
        let (url, server) = fixture("200 OK", &body, true).await;
        assert_eq!(send(&sidecar, &url, true, "gpt-5.6-luna").await, success);
        server.await.unwrap();
        let record = rx.try_recv().expect("stream attempt observation");
        assert_eq!(record.usage.input_tokens, Some(7));
        assert_eq!(record.outcome, expected);
        assert!(rx.try_recv().is_err());
    }
}

#[tokio::test]
async fn cancellation_finalizes_once_but_preflight_and_unpolled_work_do_not() {
    let (mut sidecar, mut rx) = observed_sidecar();
    sidecar.backend = SidecarBackend::Claude;
    assert!(sidecar.complete("private", "private").await.is_err());
    assert!(rx.try_recv().is_err());
    sidecar.backend = SidecarBackend::OpenAI;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let unpolled = send(&sidecar, &url, false, "gpt-5.6-luna");
    drop(unpolled);
    assert!(rx.try_recv().is_err());
    let task = tokio::spawn(async move { send(&sidecar, &url, false, "gpt-5.6-luna").await });
    let (_socket, _) = listener.accept().await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let record = rx
        .try_recv()
        .expect("cancelled sent request remains visible");
    assert_eq!(record.outcome, RequestOutcome::Cancelled);
    assert_eq!(record.usage.input_tokens, None);
    assert!(rx.try_recv().is_err());
}
