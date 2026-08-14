const SESSION_LIBRARIAN_CURRENT_PROMPT: &str =
    "Invoke session_librarian exactly once for the current session and report the result.";

fn write_session_librarian_skill(project_root: &std::path::Path) {
    let skill_dir = project_root.join(".jcode/skills/session-librarian");
    std::fs::create_dir_all(&skill_dir).expect("create session librarian skill dir");
    std::fs::write(
        skill_dir.join("SKILL.md"),
        format!(
            "---\nname: session-librarian\ndescription: Summarize one canonical session\n\
             default-prompt: {SESSION_LIBRARIAN_CURRENT_PROMPT}\n---\n\
             When invoked without an argument, submit this prompt exactly once:\n\
             `{SESSION_LIBRARIAN_CURRENT_PROMPT}`\n\
             When invoked with a trailing persisted session identifier, preserve it exactly and \
             pass it as `session_librarian.session_id`, then report the result.\n"
        ),
    )
    .expect("write session librarian skill");
}

fn submitted_text(app: &App) -> Option<&str> {
    app.session.messages.last().and_then(|message| {
        let [ContentBlock::Text { text, .. }] = message.content.as_slice() else {
            return None;
        };
        Some(text.as_str())
    })
}

#[test]
fn project_overlay_discovers_session_librarian_skill() {
    let mut app = create_test_app();
    let project = tempfile::tempdir().expect("tempdir");
    write_session_librarian_skill(project.path());
    app.session.working_dir = Some(project.path().to_string_lossy().into_owned());

    let skills = app.current_skills_snapshot();
    let skill = skills
        .get("session-librarian")
        .expect("project-local session librarian skill should be discovered");

    assert!(skill.path.starts_with(project.path()));
    assert!(skill.content.contains("session_librarian.session_id"));
}

#[test]
fn bare_session_librarian_invocation_submits_current_session_prompt_once() {
    let mut app = create_test_app();
    let project = tempfile::tempdir().expect("tempdir");
    write_session_librarian_skill(project.path());
    app.session.working_dir = Some(project.path().to_string_lossy().into_owned());
    let messages_before = app.session.messages.len();
    app.input = "/session-librarian".to_owned();
    app.cursor_pos = app.input.len();

    app.submit_input();

    assert_eq!(app.active_skill.as_deref(), Some("session-librarian"));
    assert!(app.is_processing, "bare activation must start exactly one turn");
    assert_eq!(app.session.messages.len(), messages_before + 1);
    assert_eq!(submitted_text(&app), Some(SESSION_LIBRARIAN_CURRENT_PROMPT));
}

#[test]
fn session_librarian_preserves_and_forwards_explicit_session_id() {
    let mut app = create_test_app();
    let project = tempfile::tempdir().expect("tempdir");
    write_session_librarian_skill(project.path());
    app.session.working_dir = Some(project.path().to_string_lossy().into_owned());
    let session_id = "persisted-session_2026-08-14";
    app.input = format!("/session-librarian {session_id}");
    app.cursor_pos = app.input.len();

    app.submit_input();

    assert_eq!(app.active_skill.as_deref(), Some("session-librarian"));
    assert!(app.is_processing, "explicit activation must start one turn");
    assert_eq!(submitted_text(&app), Some(session_id));
    let active_skill = app
        .current_skills_snapshot()
        .get("session-librarian")
        .expect("active skill remains discoverable")
        .clone();
    assert!(active_skill.content.contains("session_librarian.session_id"));
}

#[test]
fn unused_session_does_not_activate_session_librarian_or_start_a_turn() {
    let mut app = create_test_app();
    let project = tempfile::tempdir().expect("tempdir");
    write_session_librarian_skill(project.path());
    app.session.working_dir = Some(project.path().to_string_lossy().into_owned());
    let messages_before = app.session.messages.len();

    let skills = app.current_skills_snapshot();
    assert!(skills.get("session-librarian").is_some());
    assert!(app.active_skill.is_none());
    assert!(!app.is_processing);
    assert_eq!(app.session.messages.len(), messages_before);
}
