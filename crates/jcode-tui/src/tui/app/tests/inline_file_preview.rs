#[test]
fn test_click_on_relative_markdown_path_toggles_inline_preview() {
    let _render_lock = scroll_render_test_lock();
    let repository = tempfile::tempdir().expect("repository tempdir");
    let docs = repository.path().join("docs");
    std::fs::create_dir_all(&docs).expect("create docs directory");
    std::fs::write(
        docs.join("guide.md"),
        "# Preview Heading\n\n- first item\n- second item\n\nA final paragraph.\n",
    )
    .expect("write preview markdown");

    let mut app = create_test_app();
    app.session.working_dir = Some(repository.path().to_string_lossy().into_owned());
    app.display_messages = vec![
        DisplayMessage::user("show me the guide"),
        DisplayMessage::assistant("- `docs/guide.md`"),
    ];
    app.bump_display_messages_version();
    app.scroll_offset = 0;
    app.auto_scroll_paused = false;
    app.is_processing = false;
    app.status = ProcessingStatus::Idle;

    let backend = ratatui::backend::TestBackend::new(90, 32);
    let mut terminal = ratatui::Terminal::new(backend).expect("create test terminal");

    let collapsed = render_and_snap(&app, &mut terminal);
    assert!(collapsed.contains("docs/guide.md"));
    assert!(!collapsed.contains("Preview Heading"));

    let locate_path = |terminal: &ratatui::Terminal<ratatui::backend::TestBackend>| {
        let buf = terminal.backend().buffer();
        let area = *buf.area();
        for row in 0..area.height {
            let mut line = String::new();
            for col in 0..area.width {
                line.push_str(buf[(col, row)].symbol());
            }
            if let Some(byte) = line.find("docs/guide.md") {
                return Some((line[..byte].chars().count() as u16 + 3, row));
            }
        }
        None
    };
    let click = |app: &mut App, col: u16, row: u16| {
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: col,
            row,
            modifiers: KeyModifiers::empty(),
        });
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: col,
            row,
            modifiers: KeyModifiers::empty(),
        });
    };

    let (path_col, path_row) = locate_path(&terminal).expect("path must be visible");
    click(&mut app, path_col, path_row);

    let expanded = render_and_snap(&app, &mut terminal);
    assert!(
        expanded.contains("Inline file · docs/guide.md"),
        "expanded preview must have an inline header:\n{expanded}"
    );
    assert!(
        expanded.contains("Preview Heading"),
        "expanded preview must render markdown content:\n{expanded}"
    );
    assert!(expanded.contains("• first item"));

    let (path_col, path_row) = locate_path(&terminal).expect("path remains visible when expanded");
    click(&mut app, path_col, path_row);
    let collapsed_again = render_and_snap(&app, &mut terminal);
    assert!(!collapsed_again.contains("Inline file · docs/guide.md"));
    assert!(!collapsed_again.contains("Preview Heading"));
}

#[test]
fn test_expanded_inline_file_preview_participates_in_chat_scroll() {
    let _render_lock = scroll_render_test_lock();
    let repository = tempfile::tempdir().expect("repository tempdir");
    let docs = repository.path().join("docs");
    std::fs::create_dir_all(&docs).expect("create docs directory");
    let long_markdown = (1..=80)
        .map(|line| format!("- scrollable preview line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(docs.join("long.md"), long_markdown).expect("write long markdown");

    let mut app = create_test_app();
    app.session.working_dir = Some(repository.path().to_string_lossy().into_owned());
    app.display_messages = vec![
        DisplayMessage::user("show the long file"),
        DisplayMessage::assistant("`docs/long.md`"),
    ];
    app.bump_display_messages_version();
    assert!(app.try_toggle_inline_file_preview("docs/long.md", 1));

    let backend = ratatui::backend::TestBackend::new(72, 18);
    let mut terminal = ratatui::Terminal::new(backend).expect("create test terminal");
    let first = render_and_snap(&app, &mut terminal);
    let max_scroll = crate::tui::ui::last_max_scroll();
    assert!(max_scroll > 40, "long preview must extend normal chat scroll");

    assert!(app.scroll_up(8), "chat scroll must move through the preview");
    let second = render_and_snap(&app, &mut terminal);
    assert_ne!(first, second, "scrolling must reveal different preview rows");
    assert!(app.auto_scroll_paused);
}

#[test]
fn test_clicking_visible_inline_file_body_collapses_preview() {
    let _render_lock = scroll_render_test_lock();
    let repository = tempfile::tempdir().expect("repository tempdir");
    let docs = repository.path().join("docs");
    std::fs::create_dir_all(&docs).expect("create docs directory");
    let long_markdown = (1..=80)
        .map(|line| format!("- dismissible preview line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(docs.join("long.md"), long_markdown).expect("write long markdown");

    let mut app = create_test_app();
    app.session.working_dir = Some(repository.path().to_string_lossy().into_owned());
    app.display_messages = vec![DisplayMessage::assistant("`docs/long.md`")];
    app.bump_display_messages_version();
    assert!(app.try_toggle_inline_file_preview("docs/long.md", 0));

    let backend = ratatui::backend::TestBackend::new(72, 18);
    let mut terminal = ratatui::Terminal::new(backend).expect("create test terminal");
    render_and_snap(&app, &mut terminal);
    assert!(app.scroll_up(12), "scrolling should expose the preview body");
    render_and_snap(&app, &mut terminal);

    let body_position = {
        let buf = terminal.backend().buffer();
        let area = *buf.area();
        (0..area.height).find_map(|row| {
            let mut line = String::new();
            for col in 0..area.width {
                line.push_str(buf[(col, row)].symbol());
            }
            line.contains("dismissible preview line ")
                .then_some((area.left() + 3, row))
        })
    }
    .expect("a visible preview body row");

    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        app.handle_mouse_event(MouseEvent {
            kind,
            column: body_position.0,
            row: body_position.1,
            modifiers: KeyModifiers::empty(),
        });
    }

    assert!(
        app.inline_file_previews.is_empty(),
        "clicking a visible preview body row should remove the preview state"
    );
    let collapsed = render_and_snap(&app, &mut terminal);
    assert!(
        !collapsed.contains("Inline file · docs/long.md"),
        "clicking a visible preview body row should collapse it:\n{collapsed}"
    );
}

#[test]
fn test_dragging_over_inline_file_body_keeps_preview_open_for_copying() {
    let _render_lock = scroll_render_test_lock();
    let repository = tempfile::tempdir().expect("repository tempdir");
    let docs = repository.path().join("docs");
    std::fs::create_dir_all(&docs).expect("create docs directory");
    let long_markdown = (1..=80)
        .map(|line| format!("- drag-protected preview line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(docs.join("long.md"), long_markdown).expect("write long markdown");

    let mut app = create_test_app();
    app.session.working_dir = Some(repository.path().to_string_lossy().into_owned());
    app.display_messages = vec![DisplayMessage::assistant("`docs/long.md`")];
    app.bump_display_messages_version();
    assert!(app.try_toggle_inline_file_preview("docs/long.md", 0));

    let backend = ratatui::backend::TestBackend::new(72, 18);
    let mut terminal = ratatui::Terminal::new(backend).expect("create test terminal");
    render_and_snap(&app, &mut terminal);
    assert!(app.scroll_up(12), "scrolling should expose the preview body");
    render_and_snap(&app, &mut terminal);

    let body_rows = {
        let buf = terminal.backend().buffer();
        let area = *buf.area();
        (0..area.height)
            .filter(|row| {
                let mut line = String::new();
                for col in 0..area.width {
                    line.push_str(buf[(col, *row)].symbol());
                }
                line.contains("drag-protected preview line ")
            })
            .collect::<Vec<_>>()
    };
    assert!(body_rows.len() >= 2, "a copy drag needs two visible preview rows");
    let column = terminal.backend().buffer().area().left() + 3;
    let start_row = body_rows[0];
    let end_row = body_rows[1];

    app.handle_mouse_event(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row: start_row,
        modifiers: KeyModifiers::empty(),
    });
    app.handle_mouse_event(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column,
        row: end_row,
        modifiers: KeyModifiers::empty(),
    });
    app.handle_mouse_event(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column,
        row: end_row,
        modifiers: KeyModifiers::empty(),
    });

    assert!(
        !app.inline_file_previews.is_empty(),
        "dragging to copy preview text must not collapse the preview"
    );
}

#[test]
fn test_inline_file_preview_rejects_oversized_and_binary_files_safely() {
    let repository = tempfile::tempdir().expect("repository tempdir");
    std::fs::write(repository.path().join("large.txt"), vec![b'x'; 512 * 1024 + 1])
        .expect("write oversized file");
    std::fs::write(repository.path().join("binary.bin"), [0xff, 0xfe, 0xfd])
        .expect("write binary file");

    let mut app = create_test_app();
    app.session.working_dir = Some(repository.path().to_string_lossy().into_owned());
    app.display_messages = vec![DisplayMessage::assistant("large.txt binary.bin")];
    app.bump_display_messages_version();

    assert!(app.try_toggle_inline_file_preview("large.txt", 0));
    assert!(app.inline_file_previews.is_empty());
    assert!(
        app.status_notice()
            .is_some_and(|notice| notice.contains("too large"))
    );

    assert!(app.try_toggle_inline_file_preview("binary.bin", 0));
    assert!(app.inline_file_previews.is_empty());
    assert!(
        app.status_notice()
            .is_some_and(|notice| notice.contains("not readable text"))
    );
}
