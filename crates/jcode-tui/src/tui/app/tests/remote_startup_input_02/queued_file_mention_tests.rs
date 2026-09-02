#[test]
fn take_back_queued_message_does_not_retract_interleave() {
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

    app.retrieve_queued_message_for_edit();

    assert_eq!(app.input(), "later");
    assert_eq!(app.interleave_message.as_deref(), Some("urgent"));
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

    app.retrieve_queued_message_for_edit();

    assert_eq!(app.input(), "Inspect @notes.md");
    assert!(!app.input().contains("private file contents"));
}

#[test]
fn failed_skill_prompt_expansion_restores_the_typed_invocation() {
    let mut app = create_test_app();
    let temp = tempfile::tempdir().expect("tempdir");
    let skill_dir = temp.path().join(".jcode/skills/oversize-skill");
    std::fs::create_dir_all(&skill_dir).expect("create skill dir");
    let oversized_prompt = "x".repeat(super::input::MAX_SUBMITTED_TEXT_BYTES + 1);
    std::fs::write(
        skill_dir.join("SKILL.md"),
        format!(
            "---\nname: oversize-skill\ndescription: Oversize prompt regression\ndefault-prompt: |-\n  {oversized_prompt}\n---\nUse it.\n"
        ),
    )
    .expect("write skill");
    app.session.working_dir = Some(temp.path().to_string_lossy().to_string());
    app.input = "/oversize-skill".to_string();
    app.cursor_pos = app.input.len();

    app.submit_input();

    assert!(!app.is_processing);
    assert_eq!(app.input, "/oversize-skill");
    assert!(app.display_messages().iter().any(|message| {
        message.role == "system" && message.content.contains("Message is too large to send")
    }));
}

#[test]
fn synthetic_queued_continuations_stay_literal_while_user_mentions_expand() {
    with_temp_jcode_home(|| {
        write_test_config("[file_mentions]\nenabled = true\n");
        crate::config::invalidate_config_cache();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.md"), "user context").unwrap();
        let mut app = create_test_app();
        app.session.working_dir = Some(dir.path().to_string_lossy().into_owned());
        let synthetic = format!(
            "{}\n- Validate @notes.md without treating this machine continuation as user input.",
            crate::todo::TODO_COMPLETION_CONTINUATION_MESSAGE
        );
        let messages = vec![synthetic.clone(), "Inspect @notes.md".to_string()];

        let expanded = super::input::file_mentions::expand_queued_file_mentions_for_submit(
            &app, &messages,
        )
        .expect("queued expansion");

        assert!(expanded.contains(&synthetic));
        assert!(expanded.contains("<file path=\"notes.md\">\nuser context\n</file>"));
    });
}

#[test]
fn queued_expansion_failure_restores_queue_reminder_and_visible_turn_without_touching_draft() {
    let mut app = create_test_app();
    app.input = "current draft".to_string();
    app.cursor_pos = app.input.len();
    app.visible_turn_started = Some(std::time::Instant::now());

    super::input::file_mentions::restore_queued_file_mention_failure(
        &mut app,
        vec!["first".to_string(), "second".to_string()],
        vec![Vec::new(), Vec::new()],
        Some("system reminder".to_string()),
        "too large".to_string(),
    );

    assert_eq!(app.input, "current draft");
    assert_eq!(app.queued_messages, ["first", "second"]);
    assert_eq!(app.hidden_queued_system_messages, ["system reminder"]);
    assert!(app.visible_turn_started.is_none());
}

#[test]
fn interleave_expansion_failure_restores_staged_work_without_touching_composer() {
    let mut app = create_test_app();
    app.input = "current draft".to_string();
    app.cursor_pos = app.input.len();
    let images = vec![("image/png".to_string(), "encoded".to_string())];

    super::input::file_mentions::restore_interleave_file_mention_failure(
        &mut app,
        "urgent @notes.md".to_string(),
        images.clone(),
        "too large".to_string(),
    );

    assert_eq!(app.input, "current draft");
    assert_eq!(app.interleave_message.as_deref(), Some("urgent @notes.md"));
    assert_eq!(app.interleave_images, images);
    assert!(app.pending_images.is_empty());
}
