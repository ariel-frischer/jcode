use std::path::PathBuf;

fn session_feedback_project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical project root")
}

fn app_with_project_session_feedback_skill() -> App {
    let project_root = session_feedback_project_root();
    assert!(
        project_root
            .join(".jcode/skills/session-feedback/SKILL.md")
            .is_file(),
        "the real project-local session-feedback skill must exist"
    );

    let mut app = create_test_app();
    app.session.working_dir = Some(project_root.to_string_lossy().into_owned());
    app.refresh_skills_snapshot();
    app
}

#[test]
fn session_feedback_skill_is_discoverable_and_bare_invocation_selects_current_session() {
    let mut app = app_with_project_session_feedback_skill();
    let session_id = app.session.id.clone();
    let initial_session_messages = app.session.messages.len();

    let suggestions = app.get_suggestions_for("/session-feed");
    assert!(
        suggestions
            .iter()
            .any(|(command, _)| command == "/session-feedback"),
        "project-local /session-feedback skill was not discoverable"
    );

    app.input = "/session-feedback".to_string();
    app.cursor_pos = app.input.len();
    app.submit_input();

    assert_eq!(app.active_skill.as_deref(), Some("session-feedback"));
    assert_eq!(app.session.id, session_id);
    assert_eq!(
        app.session.messages.len(),
        initial_session_messages,
        "bare invocation must not invent or forward a named-session argument"
    );
    assert!(!app.pending_turn);
    assert!(!app.is_processing);
}

#[test]
fn session_feedback_skill_forwards_visible_session_id_unchanged() {
    let mut app = app_with_project_session_feedback_skill();
    let visible_session_id = "session-visible_01";
    app.input = format!("/session-feedback {visible_session_id}");
    app.cursor_pos = app.input.len();

    app.submit_input();

    assert_eq!(app.active_skill.as_deref(), Some("session-feedback"));
    let submitted = app
        .session
        .messages
        .last()
        .expect("named-session invocation should submit a skill prompt");
    assert!(matches!(
        submitted.content.as_slice(),
        [ContentBlock::Text { text, .. }] if text == visible_session_id
    ));
    assert!(app.pending_turn);
    assert!(app.is_processing);
}

#[test]
fn session_feedback_discovery_is_inert_and_declares_no_automatic_trigger() {
    let app = app_with_project_session_feedback_skill();

    assert_eq!(app.active_skill, None);
    assert!(app.session.messages.is_empty());
    assert!(!app.pending_turn);
    assert!(!app.is_processing);

    let skill = std::fs::read_to_string(
        session_feedback_project_root().join(".jcode/skills/session-feedback/SKILL.md"),
    )
    .expect("read real project-local session-feedback skill");
    let frontmatter = skill
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .map(|(frontmatter, _)| frontmatter)
        .expect("session-feedback skill must have YAML frontmatter");

    for forbidden_key in [
        "trigger:",
        "triggers:",
        "hook:",
        "hooks:",
        "session_end:",
        "pre_close:",
        "background:",
        "automatic:",
    ] {
        assert!(
            !frontmatter.lines().any(|line| {
                line.trim_start()
                    .to_ascii_lowercase()
                    .starts_with(forbidden_key)
            }),
            "session-feedback must remain human-triggered; found `{forbidden_key}` in frontmatter"
        );
    }
}
