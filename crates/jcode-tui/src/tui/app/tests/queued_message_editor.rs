fn queued_editor_selection(
    message_id: &str,
    content: &str,
) -> crate::protocol::QueuedMessageEditorSelection {
    crate::protocol::QueuedMessageEditorSelection {
        message_id: message_id.to_string(),
        content: content.to_string(),
        images: vec![
            ("image/png".to_string(), format!("{message_id}-png")),
            ("image/jpeg".to_string(), format!("{message_id}-jpeg")),
        ],
        older_available: true,
        newer_available: true,
    }
}

fn queued_editor_event(
    navigation_session_id: &str,
    operation_id: &str,
    outcome: crate::protocol::QueuedMessageEditorOutcome,
    selection: Option<crate::protocol::QueuedMessageEditorSelection>,
    placement: crate::protocol::QueuedMessageEditorPlacement,
) -> crate::protocol::ServerEvent {
    crate::protocol::ServerEvent::QueuedMessageEditorResult {
        id: 1,
        navigation_session_id: navigation_session_id.to_string(),
        operation_id: operation_id.to_string(),
        outcome,
        selection,
        placement,
        message: None,
    }
}

fn apply_queued_editor_event(app: &mut App, event: crate::protocol::ServerEvent) {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let mut remote = {
        let _guard = rt.enter();
        crate::tui::backend::RemoteConnection::dummy()
    };
    super::remote::handle_server_event(app, event, &mut remote);
}

#[test]
fn queued_editor_applies_only_matching_session_and_operation_results() {
    let mut app = create_test_app();
    app.input = "current draft".to_string();
    app.pending_images = vec![("image/webp".to_string(), "current-image".to_string())];
    app.remote_queued_message_editor
        .begin("active-navigation", "pending-move");

    for (navigation_session_id, operation_id) in [
        ("foreign-navigation", "pending-move"),
        ("active-navigation", "stale-operation"),
    ] {
        apply_queued_editor_event(
            &mut app,
            queued_editor_event(
                navigation_session_id,
                operation_id,
                crate::protocol::QueuedMessageEditorOutcome::Moved,
                Some(queued_editor_selection("foreign", "must not apply")),
                crate::protocol::QueuedMessageEditorPlacement::Exact,
            ),
        );
        assert_eq!(app.input, "current draft");
        assert_eq!(
            app.pending_images,
            [("image/webp".to_string(), "current-image".to_string())]
        );
    }

    let selected = queued_editor_selection("selected-older", "selected older draft");
    apply_queued_editor_event(
        &mut app,
        queued_editor_event(
            "active-navigation",
            "pending-move",
            crate::protocol::QueuedMessageEditorOutcome::Moved,
            Some(selected.clone()),
            crate::protocol::QueuedMessageEditorPlacement::Exact,
        ),
    );
    assert_eq!(app.input, selected.content);
    assert_eq!(app.pending_images, selected.images);
    assert_eq!(
        app.status_notice(),
        Some("Moved to queued message".to_string())
    );
}

#[test]
fn queued_editor_boundary_and_conflict_preserve_complete_composer_draft() {
    for (outcome, placement, expected_notice) in [
        (
            crate::protocol::QueuedMessageEditorOutcome::Boundary,
            crate::protocol::QueuedMessageEditorPlacement::Exact,
            "Already at the queued message boundary",
        ),
        (
            crate::protocol::QueuedMessageEditorOutcome::Conflict,
            crate::protocol::QueuedMessageEditorPlacement::NotApplied,
            "Queued message changed; your draft was not applied",
        ),
    ] {
        let mut app = create_test_app();
        app.input = "recoverable draft".to_string();
        app.cursor_pos = app.input.len();
        app.pending_images = vec![
            ("image/png".to_string(), "first".to_string()),
            ("image/jpeg".to_string(), "second".to_string()),
        ];
        let original_images = app.pending_images.clone();
        app.remote_queued_message_editor
            .begin("active-navigation", "pending-operation");

        apply_queued_editor_event(
            &mut app,
            queued_editor_event(
                "active-navigation",
                "pending-operation",
                outcome,
                Some(queued_editor_selection("selected", "server-side draft")),
                placement,
            ),
        );

        assert_eq!(app.input, "recoverable draft");
        assert_eq!(app.cursor_pos, app.input.len());
        assert_eq!(app.pending_images, original_images);
        assert_eq!(app.status_notice(), Some(expected_notice.to_string()));
        assert!(app.remote_queued_message_editor.is_active());
    }
}

#[test]
fn queued_editor_surfaces_stale_replay_release_commit_and_delete_distinctly() {
    let cases = [
        (
            crate::protocol::QueuedMessageEditorOutcome::StalePlacement,
            crate::protocol::QueuedMessageEditorPlacement::StaleBestEffort,
            "Updated queued message using stale placement",
        ),
        (
            crate::protocol::QueuedMessageEditorOutcome::Replay,
            crate::protocol::QueuedMessageEditorPlacement::Exact,
            "Replayed queued message editor result",
        ),
        (
            crate::protocol::QueuedMessageEditorOutcome::Released,
            crate::protocol::QueuedMessageEditorPlacement::Exact,
            "Released queued message editor",
        ),
        (
            crate::protocol::QueuedMessageEditorOutcome::Committed,
            crate::protocol::QueuedMessageEditorPlacement::Exact,
            "Updated queued message",
        ),
        (
            crate::protocol::QueuedMessageEditorOutcome::Deleted,
            crate::protocol::QueuedMessageEditorPlacement::Exact,
            "Deleted queued message",
        ),
    ];

    for (outcome, placement, expected_notice) in cases {
        let mut app = create_test_app();
        app.input = "draft".to_string();
        app.pending_images = vec![("image/png".to_string(), "draft-image".to_string())];
        app.remote_queued_message_editor
            .begin("active-navigation", "pending-operation");

        apply_queued_editor_event(
            &mut app,
            queued_editor_event(
                "active-navigation",
                "pending-operation",
                outcome,
                None,
                placement,
            ),
        );

        assert_eq!(app.status_notice(), Some(expected_notice.to_string()));
    }
}

fn queue_local_draft(app: &mut App, content: &str, images: Vec<(String, String)>) {
    app.is_processing = true;
    app.queue_mode = true;
    app.input = content.to_string();
    app.cursor_pos = app.input.len();
    app.pending_images = images;
    app.handle_key(KeyCode::Enter, KeyModifiers::empty())
        .expect("queue local draft");
}

#[test]
fn local_queued_editor_moves_older_and_newer_while_preserving_saved_images() {
    let mut app = create_test_app();
    let oldest_images = vec![("image/png".to_string(), "oldest-image".to_string())];
    let middle_images = vec![("image/jpeg".to_string(), "middle-image".to_string())];
    let newest_images = vec![
        ("image/webp".to_string(), "newest-first".to_string()),
        ("image/png".to_string(), "newest-second".to_string()),
    ];
    queue_local_draft(&mut app, "oldest", oldest_images.clone());
    queue_local_draft(&mut app, "middle", middle_images.clone());
    queue_local_draft(&mut app, "newest", newest_images.clone());

    app.handle_key(KeyCode::Char('q'), KeyModifiers::ALT)
        .expect("recall newest");
    assert_eq!(app.input, "newest");
    assert_eq!(app.pending_images, newest_images);

    app.input = "newest edited".to_string();
    app.pending_images.swap(0, 1);
    let edited_newest_images = app.pending_images.clone();
    app.handle_key(KeyCode::Char('q'), KeyModifiers::ALT)
        .expect("move older");
    assert_eq!(app.input, "middle");
    assert_eq!(app.pending_images, middle_images);

    app.input = "middle edited".to_string();
    app.handle_key(
        KeyCode::Char('q'),
        KeyModifiers::ALT | KeyModifiers::SHIFT,
    )
    .expect("move newer");
    assert_eq!(app.input, "newest edited");
    assert_eq!(app.pending_images, edited_newest_images);

    app.handle_key(KeyCode::Char('q'), KeyModifiers::ALT)
        .expect("move older again");
    assert_eq!(app.input, "middle edited");
    assert_eq!(app.pending_images, middle_images);
    app.handle_key(KeyCode::Char('q'), KeyModifiers::ALT)
        .expect("move to oldest");
    assert_eq!(app.input, "oldest");
    assert_eq!(app.pending_images, oldest_images);
}

#[test]
fn local_queued_editor_boundaries_keep_the_complete_draft_unchanged() {
    let mut app = create_test_app();
    let images = vec![("image/png".to_string(), "only-image".to_string())];
    queue_local_draft(&mut app, "only", images.clone());

    app.handle_key(KeyCode::Char('q'), KeyModifiers::ALT)
        .expect("recall only draft");
    app.input = "only edited".to_string();
    app.cursor_pos = app.input.len();
    let queue_before = app.queued_messages.clone();

    app.handle_key(KeyCode::Char('q'), KeyModifiers::ALT)
        .expect("oldest boundary");
    assert_eq!(app.input, "only edited");
    assert_eq!(app.pending_images, images);
    assert_eq!(app.queued_messages, queue_before);
    assert_eq!(
        app.status_notice(),
        Some("Already editing the oldest queued message".to_string())
    );

    app.handle_key(
        KeyCode::Char('Q'),
        KeyModifiers::ALT | KeyModifiers::SHIFT,
    )
    .expect("newest boundary");
    assert_eq!(app.input, "only edited");
    assert_eq!(app.pending_images, images);
    assert_eq!(app.queued_messages, queue_before);
    assert_eq!(
        app.status_notice(),
        Some("Already editing the newest queued message".to_string())
    );
}

#[test]
fn local_queued_editor_excludes_messages_queued_after_the_snapshot() {
    let mut app = create_test_app();
    queue_local_draft(&mut app, "oldest", Vec::new());
    queue_local_draft(&mut app, "newest", Vec::new());
    app.handle_key(KeyCode::Char('q'), KeyModifiers::ALT)
        .expect("start editor");

    app.queued_messages.push("post-snapshot arrival".to_string());
    app.handle_key(KeyCode::Char('q'), KeyModifiers::ALT)
        .expect("move within snapshot");

    assert_eq!(app.input, "oldest");
    assert_eq!(app.queued_messages, ["post-snapshot arrival"]);
}

#[test]
fn local_queued_editor_treats_images_only_drafts_as_non_empty() {
    let mut app = create_test_app();
    let images = vec![
        ("image/png".to_string(), "first".to_string()),
        ("image/jpeg".to_string(), "second".to_string()),
    ];
    queue_local_draft(&mut app, "", images.clone());
    assert_eq!(app.queued_messages, [""]);

    app.handle_key(KeyCode::Char('q'), KeyModifiers::ALT)
        .expect("recall images-only draft");
    assert!(app.input.is_empty());
    assert_eq!(app.pending_images, images);

    app.handle_key(KeyCode::Enter, KeyModifiers::empty())
        .expect("commit images-only draft");
    assert_eq!(app.queued_messages, [""]);
    assert_eq!(app.queued_message_images, [images]);
    assert_eq!(app.status_notice(), Some("Updated queued message".to_string()));
}

#[test]
fn local_queue_recovery_keeps_interleave_images_aligned() {
    let mut app = create_test_app();
    let images = vec![
        ("image/png".to_string(), "first".to_string()),
        ("image/jpeg".to_string(), "second".to_string()),
    ];
    app.interleave_message = Some("recover me".to_string());
    app.interleave_images = images.clone();

    assert!(super::remote::recover_local_interleave_to_queue_for_test(
        &mut app,
        "test disconnect",
    ));
    assert_eq!(app.queued_messages, ["recover me"]);
    assert_eq!(app.queued_message_images, [images]);
    assert!(app.interleave_images.is_empty());
}
