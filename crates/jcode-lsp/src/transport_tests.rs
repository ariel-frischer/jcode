use super::transport::FrameCodec;

#[test]
fn frame_codec_handles_fragmented_and_multiple_messages() {
    let first = br#"{"jsonrpc":"2.0","method":"one"}"#;
    let second = br#"{"jsonrpc":"2.0","method":"two"}"#;
    let encoded = format!(
        "Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}",
        first.len(),
        String::from_utf8_lossy(first),
        second.len(),
        String::from_utf8_lossy(second)
    )
    .into_bytes();
    let mut codec = FrameCodec::new(1024);
    let split = 10;
    assert!(
        codec
            .push(&encoded[..split])
            .expect("partial header")
            .is_empty()
    );
    let frames = codec.push(&encoded[split..]).expect("complete frames");
    assert_eq!(frames, vec![first.to_vec(), second.to_vec()]);
}

#[tokio::test]
async fn client_correlates_response_and_publishes_notifications() {
    use super::transport::LspClient;
    use serde_json::Value;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::time::{Duration, timeout};

    let (client_io, mut server_io) = tokio::io::duplex(4096);
    let (client_reader, client_writer) = tokio::io::split(client_io);
    let client = LspClient::from_stream(client_reader, client_writer).await;
    let mut notifications = client.subscribe();
    tokio::spawn(async move {
        let mut header = Vec::new();
        loop {
            let mut byte = [0_u8; 1];
            server_io
                .read_exact(&mut byte)
                .await
                .expect("request header");
            header.push(byte[0]);
            if header.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        let header = String::from_utf8(header).expect("header utf8");
        let length = header
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length: "))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .expect("content length");
        let mut body = vec![0_u8; length];
        server_io.read_exact(&mut body).await.expect("request body");
        let request: Value = serde_json::from_slice(&body).expect("request json");
        let id = request["id"].as_u64().expect("request id");
        let response = serde_json::json!({"jsonrpc":"2.0","id":id,"result":{"ok":true}});
        let response = serde_json::to_vec(&response).expect("response json");
        let frame = format!("Content-Length: {}\r\n\r\n", response.len());
        server_io
            .write_all(frame.as_bytes())
            .await
            .expect("response header");
        server_io.write_all(&response).await.expect("response body");
        let notification = serde_json::json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":"file:///main.rs","diagnostics":[]}});
        let notification = serde_json::to_vec(&notification).expect("notification json");
        let frame = format!("Content-Length: {}\r\n\r\n", notification.len());
        server_io
            .write_all(frame.as_bytes())
            .await
            .expect("notification header");
        server_io
            .write_all(&notification)
            .await
            .expect("notification body");
    });

    let result = client
        .request("initialize", serde_json::json!({}), Duration::from_secs(1))
        .await
        .expect("response");
    assert_eq!(result["ok"], true);
    let notification = timeout(Duration::from_secs(1), notifications.recv())
        .await
        .expect("notification timeout")
        .expect("notification");
    assert_eq!(notification.method, "textDocument/publishDiagnostics");
}
