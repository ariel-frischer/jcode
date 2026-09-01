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
