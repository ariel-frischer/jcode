use jcode_lsp::{LspSessionManager, ServerConfig, ServerRegistry};
use std::collections::BTreeMap;
use std::path::Path;

#[tokio::test]
async fn fake_server_syncs_document_and_returns_versioned_diagnostics() {
    let temp = tempfile::tempdir().expect("tempdir");
    let file = temp.path().join("main.rs");
    let server = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fake_server.py");
    let mut configs = BTreeMap::new();
    configs.insert(
        "fake".into(),
        ServerConfig {
            enabled: true,
            command: "python3".into(),
            args: vec![server.to_string_lossy().into_owned()],
            file_types: vec![".rs".into()],
            language_id: "rust".into(),
            request_timeout_ms: 500,
            startup_timeout_ms: 2_000,
            ..ServerConfig::default()
        },
    );
    let manager = LspSessionManager::with_registry(ServerRegistry::from_servers(configs));

    let session_id = manager
        .start_for_file(temp.path(), &file, None)
        .await
        .expect("start")
        .expect("fake session");
    manager
        .sync_document(&session_id, &file, "error", 7)
        .await
        .expect("sync");
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    let diagnostics = manager
        .diagnostics(&session_id, Some(&jcode_lsp::protocol::file_uri(&file)))
        .await
        .expect("diagnostics");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].version, Some(7));
    assert_eq!(diagnostics[0].message, "fake error");

    manager.disconnect(&session_id).await.expect("cleanup");
    assert!(manager.list().await.is_empty());
}
