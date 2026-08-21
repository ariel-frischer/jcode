#[test]
fn at_file_suggestions_use_session_cwd_ignore_vendor_content_and_accept_selection() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(temp.path().join("src")).expect("src directory");
    std::fs::create_dir_all(temp.path().join("node_modules/pkg")).expect("vendor directory");
    std::fs::write(temp.path().join("src/main.rs"), "fn main() {}").expect("source file");
    std::fs::write(temp.path().join("node_modules/pkg/index.js"), "").expect("vendor file");

    let mut app = create_test_app();
    app.session.working_dir = Some(temp.path().to_string_lossy().into_owned());
    app.input = "Explain @".to_string();
    app.cursor_pos = app.input.len();

    let suggestions = app.command_suggestions();
    assert!(
        suggestions.iter().any(|(value, _)| value == "Explain @src/"),
        "expected src directory suggestion, got {suggestions:?}"
    );
    assert!(
        suggestions
            .iter()
            .all(|(value, _)| !value.contains("node_modules")),
        "vendor content leaked into @ suggestions: {suggestions:?}"
    );

    app.handle_key(
        crossterm::event::KeyCode::Down,
        crossterm::event::KeyModifiers::empty(),
    )
    .expect("navigate suggestions");
    app.handle_key(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::empty(),
    )
    .expect("accept suggestion");
    assert!(app.input.starts_with("Explain @"));
    assert!(app.input.ends_with('/') || app.input.ends_with("main.rs"));
}

#[test]
fn at_file_suggestions_include_profile_specific_ignore_patterns() {
    with_temp_jcode_home(|| {
        write_test_config(
            "[profiles.review]\nfile_mentions_ignore = [\"private-notes/\"]\n",
        );
        crate::config::invalidate_config_cache();

        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(temp.path().join("private-notes"))
            .expect("private directory");
        std::fs::write(temp.path().join("private-notes/todo.md"), "")
            .expect("private file");
        std::fs::write(temp.path().join("README.md"), "").expect("readme");

        let mut app = create_test_app();
        app.session.working_dir = Some(temp.path().to_string_lossy().into_owned());
        app.session.profile_name = Some("review".to_owned());
        app.input = "@".to_owned();
        app.cursor_pos = 1;

        let suggestions = app.command_suggestions();
        assert!(suggestions.iter().any(|(value, _)| value == "@README.md"));
        assert!(!suggestions
            .iter()
            .any(|(value, _)| value.contains("private-notes")));
    });
}
