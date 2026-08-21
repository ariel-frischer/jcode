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
