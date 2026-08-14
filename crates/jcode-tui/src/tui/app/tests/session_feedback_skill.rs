use std::path::{Path as FsPath, PathBuf};

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

fn copy_directory_unchanged(source: &FsPath, destination: &FsPath) {
    std::fs::create_dir_all(destination).expect("create copied skill directory");
    for entry in std::fs::read_dir(source).expect("read source skill directory") {
        let entry = entry.expect("read source skill entry");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_directory_unchanged(&source_path, &destination_path);
        } else {
            std::fs::copy(&source_path, &destination_path).expect("copy skill file unchanged");
        }
    }
}

fn copy_session_feedback_skill(destination: &FsPath) {
    copy_directory_unchanged(
        &session_feedback_project_root().join(".jcode/skills/session-feedback"),
        destination,
    );
}

fn app_with_working_dir(working_dir: &FsPath) -> App {
    let mut app = create_test_app();
    app.session.working_dir = Some(working_dir.to_string_lossy().into_owned());
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
    let project_root = session_feedback_project_root();
    let mut app = create_test_app();
    let initial_session_messages = app.session.messages.len();
    app.session.working_dir = Some(project_root.to_string_lossy().into_owned());
    app.refresh_skills_snapshot();

    assert_eq!(app.active_skill, None);
    assert_eq!(app.session.messages.len(), initial_session_messages);
    assert!(!app.pending_turn);
    assert!(!app.is_processing);

    let skill =
        std::fs::read_to_string(project_root.join(".jcode/skills/session-feedback/SKILL.md"))
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

#[test]
fn copied_global_session_feedback_skill_is_discoverable_and_forwards_arguments() {
    with_temp_jcode_home(|| {
        let global_skill = crate::storage::jcode_dir()
            .expect("temporary Jcode home")
            .join("skills/session-feedback");
        copy_session_feedback_skill(&global_skill);
        let workspace = tempfile::tempdir().expect("isolated workspace");

        let mut current = app_with_working_dir(workspace.path());
        let initial_session_messages = current.session.messages.len();
        assert!(
            current
                .get_suggestions_for("/session-feed")
                .iter()
                .any(|(command, _)| command == "/session-feedback"),
            "copied global /session-feedback skill was not discoverable"
        );
        current.input = "/session-feedback".to_string();
        current.cursor_pos = current.input.len();
        current.submit_input();
        assert_eq!(current.active_skill.as_deref(), Some("session-feedback"));
        assert_eq!(current.session.messages.len(), initial_session_messages);
        assert!(!current.pending_turn);

        let mut named = app_with_working_dir(workspace.path());
        let visible_session_id = "session-visible_global_01";
        named.input = format!("/session-feedback {visible_session_id}");
        named.cursor_pos = named.input.len();
        named.submit_input();

        assert_eq!(named.active_skill.as_deref(), Some("session-feedback"));
        assert!(matches!(
            named.session.messages.last().map(|message| message.content.as_slice()),
            Some([ContentBlock::Text { text, .. }]) if text == visible_session_id
        ));
        assert!(named.pending_turn);
        assert!(named.is_processing);
    });
}

#[test]
fn project_session_feedback_copy_precedes_global_copy_without_specific_runtime_behavior() {
    with_temp_jcode_home(|| {
        let global_skill = crate::storage::jcode_dir()
            .expect("temporary Jcode home")
            .join("skills/session-feedback");
        copy_session_feedback_skill(&global_skill);

        let workspace = tempfile::tempdir().expect("isolated workspace");
        let project_skill = workspace.path().join(".jcode/skills/session-feedback");
        copy_session_feedback_skill(&project_skill);

        let app = app_with_working_dir(workspace.path());
        let effective = app.current_skills_snapshot();
        let skill = effective
            .get("session-feedback")
            .expect("session-feedback should be loaded from one of the standard skill roots");

        assert!(
            skill.path.starts_with(&project_skill),
            "existing project-local skill precedence should select {project_skill:?}, got {:?}",
            skill.path
        );
        assert_eq!(
            std::fs::read(&skill.path).expect("read effective project skill"),
            std::fs::read(global_skill.join("SKILL.md")).expect("read copied global skill"),
            "both copies must remain byte-for-byte unchanged"
        );
    });
}

#[test]
fn removing_both_session_feedback_copies_preserves_other_commands_and_normal_input() {
    with_temp_jcode_home(|| {
        let global_skill = crate::storage::jcode_dir()
            .expect("temporary Jcode home")
            .join("skills/session-feedback");
        copy_session_feedback_skill(&global_skill);

        let workspace = tempfile::tempdir().expect("isolated workspace");
        let project_skill = workspace.path().join(".jcode/skills/session-feedback");
        copy_session_feedback_skill(&project_skill);

        let mut app = app_with_working_dir(workspace.path());
        assert!(
            app.get_suggestions_for("/session-feed")
                .iter()
                .any(|(command, _)| command == "/session-feedback")
        );

        std::fs::remove_dir_all(&global_skill).expect("remove copied global skill");
        std::fs::remove_dir_all(&project_skill).expect("remove copied project skill");
        app.refresh_skills_snapshot();

        assert!(
            !app.get_suggestions_for("/session-feed")
                .iter()
                .any(|(command, _)| command == "/session-feedback"),
            "removing both copies must remove the session-feedback slash command"
        );
        assert!(
            app.get_suggestions_for("/he")
                .iter()
                .any(|(command, _)| command == "/help"),
            "existing slash commands must remain discoverable"
        );

        app.input = "continue normal session behavior".to_string();
        app.cursor_pos = app.input.len();
        app.submit_input();
        assert!(matches!(
            app.session.messages.last().map(|message| message.content.as_slice()),
            Some([ContentBlock::Text { text, .. }]) if text == "continue normal session behavior"
        ));
        assert_eq!(app.active_skill, None);
        assert!(app.pending_turn);
        assert!(app.is_processing);
    });
}
