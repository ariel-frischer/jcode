#[test]
fn test_retrieve_pending_message_prefers_pending_interleave_for_editing() {
    let mut app = create_test_app();
    app.is_processing = true;
    app.queue_mode = false; // Enter=interleave, Ctrl+Enter=queue

    for c in "urgent".chars() {
        app.handle_key(KeyCode::Char(c), KeyModifiers::empty())
            .unwrap();
    }
    app.handle_key(KeyCode::Enter, KeyModifiers::empty())
        .unwrap();

    for c in "later".chars() {
        app.handle_key(KeyCode::Char(c), KeyModifiers::empty())
            .unwrap();
    }
    app.handle_key(KeyCode::Enter, KeyModifiers::CONTROL)
        .unwrap();

    assert_eq!(app.interleave_message.as_deref(), Some("urgent"));
    assert_eq!(app.queued_count(), 1);

    app.retrieve_pending_message_for_edit();

    assert_eq!(app.input(), "urgent\n\nlater");
    assert_eq!(app.interleave_message.as_deref(), None);
    assert_eq!(app.queued_count(), 0);
}

#[test]
fn queued_file_mention_stays_compact_when_retrieved_for_editing() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.md"), "private file contents").unwrap();
    let mut app = create_test_app();
    app.session.working_dir = Some(dir.path().to_string_lossy().into_owned());
    app.is_processing = true;
    app.queue_mode = true;
    app.input = "Inspect @notes.md".to_string();
    app.cursor_pos = app.input.len();

    app.handle_key(KeyCode::Enter, KeyModifiers::empty())
        .unwrap();
    assert_eq!(app.queued_messages(), &["Inspect @notes.md".to_string()]);

    app.retrieve_pending_message_for_edit();

    assert_eq!(app.input(), "Inspect @notes.md");
    assert!(!app.input().contains("private file contents"));
}

