use super::*;

#[test]
fn persisted_metadata_reads_large_transcripts_from_bounded_windows() {
    let home = ScopedJcodeHome::new("bounded-metadata");
    let sessions = home.path.join("sessions");
    std::fs::create_dir_all(&sessions).expect("create sessions directory");
    let path = sessions.join("session_large.json");
    let mut file = std::fs::File::create(&path).expect("create large session");
    write!(
        file,
        "{{\"id\":\"session_large\",\"title\":\"Generated title\",\"messages\":[\""
    )
    .unwrap();
    for _ in 0..(2 * 1024) {
        file.write_all(&[b'x'; 1024]).unwrap();
    }
    write!(
        file,
        "\"],\"working_dir\":\"/workspace/large\",\"custom_title\":\"Pinned title\"}}"
    )
    .unwrap();
    drop(file);

    let metadata = BridgeState::resolve_session_metadata("session_large").expect("metadata");
    assert_eq!(metadata.working_dir.as_deref(), Some("/workspace/large"));
    assert_eq!(metadata.title.as_deref(), Some("Generated title"));
    assert_eq!(metadata.custom_title.as_deref(), Some("Pinned title"));
    assert_eq!(metadata.display_title().as_deref(), Some("Pinned title"));
}

#[test]
fn persisted_metadata_prefers_canonical_trailer_fields() {
    let home = ScopedJcodeHome::new("metadata-precedence");
    let sessions = home.path.join("sessions");
    std::fs::create_dir_all(&sessions).expect("create sessions directory");
    std::fs::write(
        sessions.join("session_precedence.json"),
        r#"{"working_dir":"/stale/header","title":"Generated title","messages":[],"custom_title":"Pinned title","working_dir":"/canonical/trailer"}"#,
    )
    .expect("write session record");

    let metadata = BridgeState::resolve_session_metadata("session_precedence").expect("metadata");
    assert_eq!(metadata.working_dir.as_deref(), Some("/canonical/trailer"));
    assert_eq!(metadata.display_title().as_deref(), Some("Pinned title"));
}

#[test]
fn compact_index_preserves_activity_order_and_nullable_metadata() {
    let home = ScopedJcodeHome::new("metadata-index-compatibility");
    assert!(BridgeState::recent_session_index_entries().is_empty());
    let connection = Connection::open(home.path.join("session-metadata-v1.sqlite3")).unwrap();
    connection
        .execute(
            "INSERT INTO recent_sessions (
                 session_id, working_dir, generated_title, custom_title,
                 todo_title, updated_at_ms, last_active_at_ms
             ) VALUES ('older', NULL, NULL, NULL, NULL, 10, NULL)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO recent_sessions (
                 session_id, working_dir, generated_title, custom_title,
                 todo_title, updated_at_ms, last_active_at_ms
             ) VALUES ('active', '/workspace', 'Generated', 'Custom', NULL, 20, 30)",
            [],
        )
        .unwrap();

    let event = only_reply_event(
        BridgeState::default()
            .api_request_to_legacy(&json!({"req": "list_sessions", "id": 1, "limit": 2})),
    );
    let ApiEvent::Sessions { sessions } = event else {
        panic!("expected sessions reply, got {event:?}");
    };
    assert_eq!(
        sessions
            .iter()
            .map(|session| session.session_id.as_str())
            .collect::<Vec<_>>(),
        ["active", "older"]
    );
    assert_eq!(sessions[0].working_dir.as_deref(), Some("/workspace"));
    assert_eq!(sessions[0].title.as_deref(), Some("Custom"));
    assert_eq!(sessions[1].working_dir, None);
    assert_eq!(sessions[1].title, None);
}
