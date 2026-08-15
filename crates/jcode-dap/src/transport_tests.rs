use super::transport::{DapClient, FrameCodec};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, split};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

fn frame(body: &str) -> Vec<u8> {
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body).into_bytes()
}

#[test]
fn codec_handles_fragmented_and_multiple_frames() {
    let first = frame(r#"{"seq":1}"#);
    let second = frame(r#"{"seq":2}"#);
    let mut codec = FrameCodec::default();
    assert!(codec.push(&first[..8]).expect("partial header").is_empty());
    let mut bytes = first[8..].to_vec();
    bytes.extend(second);
    let messages = codec.push(&bytes).expect("complete frames");
    assert_eq!(
        messages,
        vec![br#"{"seq":1}"#.to_vec(), br#"{"seq":2}"#.to_vec()]
    );
}

#[test]
fn codec_skips_malformed_json_and_recovers_to_a_later_valid_frame() {
    let malformed = frame("not-json");
    let valid = frame(r#"{"seq":3}"#);
    let mut codec = FrameCodec::default();
    let messages = codec.push(&[malformed, valid].concat()).expect("recover");
    assert_eq!(messages, vec![br#"{"seq":3}"#.to_vec()]);
    assert!(codec.malformed_messages() >= 1);
}

#[test]
fn codec_rejects_oversized_frames() {
    let mut codec = FrameCodec::new(4);
    let error = codec.push(&frame("12345")).expect_err("size bound");
    assert!(error.to_string().contains("maximum"));
}

#[tokio::test]
async fn client_correlates_responses_and_cancels_a_hung_request() {
    let (client_stream, adapter_stream) = tokio::io::duplex(4096);
    let (client_reader, client_writer) = split(client_stream);
    let client = DapClient::from_stream(client_reader, client_writer).await;
    let (mut adapter_reader, mut adapter_writer) = split(adapter_stream);
    let (seen_tx, seen_rx) = oneshot::channel();
    tokio::spawn(async move {
        let mut codec = FrameCodec::default();
        let mut bytes = [0_u8; 1024];
        let mut seen_tx = Some(seen_tx);
        loop {
            let count = adapter_reader.read(&mut bytes).await.expect("request read");
            if count == 0 {
                return;
            }
            for payload in codec.push(&bytes[..count]).expect("request frame") {
                let request: serde_json::Value =
                    serde_json::from_slice(&payload).expect("request json");
                if request["command"] == "threads" {
                    let response = serde_json::json!({
                        "seq": 2,
                        "type": "response",
                        "request_seq": request["seq"],
                        "success": true,
                        "command": "threads",
                        "body": {"threads": []}
                    });
                    adapter_writer
                        .write_all(&frame(&response.to_string()))
                        .await
                        .expect("response write");
                    let _ = seen_tx.take().expect("seen sender").send(());
                }
            }
        }
    });

    let response = client
        .request(
            "threads",
            serde_json::json!({}),
            Duration::from_secs(1),
            None,
        )
        .await
        .expect("threads response");
    assert!(response.success);
    seen_rx.await.expect("adapter saw request");

    let cancellation = CancellationToken::new();
    let pending = client.request(
        "hang",
        serde_json::json!({}),
        Duration::from_secs(10),
        Some(cancellation.clone()),
    );
    cancellation.cancel();
    let error = pending.await.expect_err("cancelled request");
    assert!(error.to_string().contains("cancel"));
}

#[tokio::test]
async fn client_times_out_a_request_when_the_adapter_does_not_reply() {
    let (client_stream, _adapter_stream) = tokio::io::duplex(4096);
    let (client_reader, client_writer) = split(client_stream);
    let client = DapClient::from_stream(client_reader, client_writer).await;
    let error = client
        .request(
            "hang",
            serde_json::json!({}),
            Duration::from_millis(20),
            None,
        )
        .await
        .expect_err("request timeout");
    assert!(error.to_string().contains("timeout"));
}

#[tokio::test]
async fn client_reports_adapter_exit_to_pending_requests() {
    let (client_stream, adapter_stream) = tokio::io::duplex(4096);
    let (client_reader, client_writer) = split(client_stream);
    let client = DapClient::from_stream(client_reader, client_writer).await;
    drop(adapter_stream);
    tokio::time::sleep(Duration::from_millis(10)).await;
    let error = client
        .request(
            "after_exit",
            serde_json::json!({}),
            Duration::from_millis(50),
            None,
        )
        .await
        .expect_err("adapter exit");
    assert!(error.to_string().contains("closed") || error.to_string().contains("exited"));
}

#[tokio::test]
async fn client_connects_to_an_existing_tcp_adapter() {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let (mut reader, mut writer) = split(stream);
        let mut codec = FrameCodec::default();
        let mut bytes = [0_u8; 1024];
        let count = reader.read(&mut bytes).await.expect("request read");
        let payload = codec
            .push(&bytes[..count])
            .expect("request frame")
            .pop()
            .expect("request");
        let request: serde_json::Value = serde_json::from_slice(&payload).expect("request json");
        let response = serde_json::json!({
            "seq": 2,
            "type": "response",
            "request_seq": request["seq"],
            "success": true,
            "command": request["command"],
            "body": {"connected": true}
        });
        writer
            .write_all(&frame(&response.to_string()))
            .await
            .expect("response write");
    });
    let client = DapClient::connect_tcp("127.0.0.1", address.port(), Duration::from_secs(1))
        .await
        .expect("tcp connection");
    let response = client
        .request(
            "threads",
            serde_json::json!({}),
            Duration::from_secs(1),
            None,
        )
        .await
        .expect("tcp response");
    assert_eq!(response.body.expect("body")["connected"], true);
}

#[tokio::test]
async fn client_spawns_an_adapter_that_listens_on_tcp() {
    let fixture =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fake_adapter.py");
    let args = vec![
        fixture.to_string_lossy().into_owned(),
        "--tcp".to_owned(),
        "${port}".to_owned(),
    ];
    let client = DapClient::spawn_tcp_listen(
        "python3",
        &args,
        std::path::Path::new("."),
        Duration::from_secs(2),
    )
    .await
    .expect("listener adapter");
    let response = client
        .request(
            "threads",
            serde_json::json!({}),
            Duration::from_secs(1),
            None,
        )
        .await
        .expect("tcp listener response");
    assert_eq!(response.body.expect("body")["threads"][0]["name"], "fake");
    client.dispose().await;
}

#[cfg(unix)]
#[tokio::test]
async fn client_connects_to_an_existing_unix_socket_adapter() {
    let socket_path =
        std::env::temp_dir().join(format!("jcode-dap-test-{}.sock", uuid::Uuid::new_v4()));
    let listener = tokio::net::UnixListener::bind(&socket_path).expect("unix listener");
    let server_path = socket_path.clone();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let (mut reader, mut writer) = split(stream);
        let mut codec = FrameCodec::default();
        let mut bytes = [0_u8; 1024];
        let count = reader.read(&mut bytes).await.expect("request read");
        let payload = codec
            .push(&bytes[..count])
            .expect("request frame")
            .pop()
            .expect("request");
        let request: serde_json::Value = serde_json::from_slice(&payload).expect("request json");
        let response = serde_json::json!({
            "seq": 2,
            "type": "response",
            "request_seq": request["seq"],
            "success": true,
            "command": request["command"],
            "body": {"connected": true}
        });
        writer
            .write_all(&frame(&response.to_string()))
            .await
            .expect("response write");
        let _ = std::fs::remove_file(server_path);
    });
    let client = DapClient::connect_unix(&socket_path, Duration::from_secs(1))
        .await
        .expect("unix connection");
    let response = client
        .request(
            "threads",
            serde_json::json!({}),
            Duration::from_secs(1),
            None,
        )
        .await
        .expect("unix response");
    assert_eq!(response.body.expect("body")["connected"], true);
}
