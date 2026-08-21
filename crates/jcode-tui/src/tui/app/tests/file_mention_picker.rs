#[test]
fn at_file_suggestions_use_session_cwd_ignore_vendor_content_and_accept_selection() {
    let _env_lock = crate::storage::lock_test_env();
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(temp.path().join("src")).expect("src directory");
    std::fs::create_dir_all(temp.path().join("node_modules/pkg")).expect("vendor directory");
    std::fs::write(temp.path().join("src/main.rs"), "fn main() {}").expect("source file");
    std::fs::write(temp.path().join("node_modules/pkg/index.js"), "").expect("vendor file");

    let mut app = create_test_app();
    app.session.working_dir = Some(temp.path().to_string_lossy().into_owned());
    app.input = "Explain @".to_string();
    app.cursor_pos = app.input.len();

    let suggestions = wait_for_file_mention_suggestions(&mut app);
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
fn tab_completes_an_active_file_mention_without_submitting() {
    let _env_lock = crate::storage::lock_test_env();
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(temp.path().join("README.md"), "readme").expect("readme");

    let mut app = create_test_app();
    app.session.working_dir = Some(temp.path().to_string_lossy().into_owned());
    app.input = "Explain @README".to_owned();
    app.cursor_pos = app.input.len();

    let suggestions = wait_for_file_mention_suggestions(&mut app);
    assert_eq!(suggestions[0].0, "Explain @README.md");
    app.handle_key(
        crossterm::event::KeyCode::Tab,
        crossterm::event::KeyModifiers::empty(),
    )
    .expect("complete file mention");

    assert_eq!(app.input, "Explain @README.md");
    assert!(app.queued_messages.is_empty());
    assert!(!app.queue_mode);
}

#[test]
fn repeated_tab_cycles_through_file_mention_suggestions() {
    let _env_lock = crate::storage::lock_test_env();
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(temp.path().join("alpha.txt"), "").expect("alpha");
    std::fs::write(temp.path().join("beta.txt"), "").expect("beta");

    let mut app = create_test_app();
    app.session.working_dir = Some(temp.path().to_string_lossy().into_owned());
    app.input = "@".to_owned();
    app.cursor_pos = 1;
    let suggestions = wait_for_file_mention_suggestions(&mut app);
    assert!(suggestions.iter().any(|(value, _)| value == "@alpha.txt"));
    assert!(suggestions.iter().any(|(value, _)| value == "@beta.txt"));

    app.handle_key(
        crossterm::event::KeyCode::Tab,
        crossterm::event::KeyModifiers::empty(),
    )
    .expect("complete first file mention");
    assert_eq!(app.input, "@alpha.txt");

    app.handle_key(
        crossterm::event::KeyCode::Tab,
        crossterm::event::KeyModifiers::empty(),
    )
    .expect("cycle to second file mention");
    assert_eq!(app.input, "@beta.txt");
}

#[test]
fn file_mention_discovery_falls_back_to_the_launch_cwd() {
    let _env_lock = crate::storage::lock_test_env();
    let mut app = create_test_app();
    app.session.working_dir = None;
    app.input = "@".to_owned();
    app.cursor_pos = 1;

    let _ = app.command_suggestions();
    let request = app
        .file_mention_discovery
        .borrow()
        .as_ref()
        .expect("file mention discovery")
        .request
        .clone();

    assert_eq!(request.root, std::env::current_dir().expect("launch cwd"));
}

#[test]
fn file_mention_discovery_prioritizes_files_directly_in_the_root() {
    let _env_lock = crate::storage::lock_test_env();
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(temp.path().join("nested")).expect("nested directory");
    for index in 0..64 {
        std::fs::write(
            temp.path().join(format!("nested/fixture-{index:02}.txt")),
            "",
        )
        .expect("nested fixture");
    }
    std::fs::write(temp.path().join("root-file.txt"), "").expect("root file");

    let (receiver, _cancel) =
        super::state_ui_input_helpers::start_file_mention_discovery_for_test(
            temp.path().to_path_buf(),
            String::new(),
            Vec::new(),
            11,
        );
    let first = receiver
        .recv_timeout(std::time::Duration::from_millis(100))
        .expect("first file batch");

    assert!(
        first.candidates.iter().any(|candidate| candidate.path == "root-file.txt"),
        "root-level file missing from first batch: {:?}",
        first.candidates
    );
}

#[test]
fn file_mentions_default_enabled_and_can_be_disabled_without_scanning() {
    assert!(jcode_config_types::FileMentionsConfig::default().enabled);
    let legacy = crate::config::Config::default();
    assert!(legacy.file_mentions.enabled);

    with_temp_jcode_home(|| {
        write_test_config("[file_mentions]\nenabled = false\n");
        crate::config::invalidate_config_cache();

        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join("README.md"), "").expect("readme");
        let mut app = create_test_app();
        app.session.working_dir = Some(temp.path().to_string_lossy().into_owned());
        app.input = "@".to_owned();
        app.cursor_pos = 1;

        assert!(app.command_suggestions().is_empty());
        assert!(!app.poll_file_mention_discovery());
    });
}

#[test]
fn submitted_file_mention_keeps_compact_display_and_expands_model_context() {
    with_temp_jcode_home(|| {
        write_test_config("[file_mentions]\nenabled = true\n");
        crate::config::invalidate_config_cache();

        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(temp.path().join("docs")).expect("docs directory");
        std::fs::write(temp.path().join("docs/context.md"), "full context\n")
            .expect("context file");

        let mut app = create_test_app();
        app.session.working_dir = Some(temp.path().to_string_lossy().into_owned());
        app.input = "Explain @docs/context.md".to_owned();
        app.cursor_pos = app.input.len();

        app.submit_input();

        let displayed = app
            .display_messages()
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .expect("displayed user message");
        assert_eq!(displayed.content, "Explain @docs/context.md");
        let submitted = app.session.messages.last().expect("submitted message");
        assert!(matches!(
            submitted.content.as_slice(),
            [ContentBlock::Text { text, .. }]
                if text == "Explain <file path=\"docs/context.md\">\nfull context\n\n</file>"
        ));
    });
}

#[test]
fn stale_file_mention_generations_are_discarded() {
    let _env_lock = crate::storage::lock_test_env();
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(temp.path().join("old-name.txt"), "").expect("old file");
    std::fs::write(temp.path().join("new-name.txt"), "").expect("new file");

    let mut app = create_test_app();
    app.session.working_dir = Some(temp.path().to_string_lossy().into_owned());
    app.input = "@old".to_owned();
    app.cursor_pos = app.input.len();
    let _ = app.command_suggestions();
    app.input = "@new".to_owned();
    app.cursor_pos = app.input.len();

    let suggestions = wait_for_file_mention_suggestions(&mut app);
    assert!(
        suggestions.iter().any(|(value, _)| value == "@new-name.txt"),
        "new query suggestions: {suggestions:?}"
    );
    assert!(suggestions
        .iter()
        .all(|(value, _)| !value.contains("old-name")));
}

#[test]
fn file_mention_discovery_is_batched_and_input_stays_within_budget() {
    let _env_lock = crate::storage::lock_test_env();
    use std::time::{Duration, Instant};

    let sizes = [32, 256, 1024];
    for size in sizes {
        let temp = tempfile::tempdir().expect("tempdir");
        for index in 0..size {
            std::fs::write(temp.path().join(format!("fixture-{index:04}.txt")), "")
                .expect("fixture file");
        }

        let started = Instant::now();
        let (receiver, _cancel) = super::state_ui_input_helpers::start_file_mention_discovery_for_test(
            temp.path().to_path_buf(),
            String::new(),
            Vec::new(),
            7,
        );
        let first = receiver
            .recv_timeout(Duration::from_millis(100))
            .expect("first bounded file batch");
        let first_batch_elapsed = started.elapsed();
        assert!(first_batch_elapsed < Duration::from_millis(100));
        assert!(first.candidates.len() <= 32);
        assert!(!first.done || size <= 32);

        let mut app = create_test_app();
        app.session.working_dir = Some(temp.path().to_string_lossy().into_owned());
        app.input = "@".to_owned();
        app.cursor_pos = 1;
        let input_started = Instant::now();
        let _ = app.command_suggestions();
        let input_elapsed = input_started.elapsed();
        assert!(
            input_elapsed < Duration::from_millis(50),
            "input request exceeded 50 ms for fixture size {size}"
        );
        eprintln!(
            "file mention benchmark: files={size} first_batch_ms={:.3} input_ms={:.3} batch={} done={}",
            first_batch_elapsed.as_secs_f64() * 1000.0,
            input_elapsed.as_secs_f64() * 1000.0,
            first.candidates.len(),
            first.done,
        );
    }
}

fn wait_for_file_mention_suggestions(app: &mut App) -> Vec<(String, &'static str)> {
    use std::time::{Duration, Instant};

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let suggestions = app.command_suggestions();
        if !suggestions.is_empty() || Instant::now() >= deadline {
            return suggestions;
        }
        app.poll_file_mention_discovery();
        std::thread::sleep(Duration::from_millis(1));
    }
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

        let suggestions = wait_for_file_mention_suggestions(&mut app);
        assert!(suggestions.iter().any(|(value, _)| value == "@README.md"));
        assert!(!suggestions
            .iter()
            .any(|(value, _)| value.contains("private-notes")));
    });
}
