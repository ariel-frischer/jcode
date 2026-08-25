const INLINE_PREVIEW_BYTE_LIMIT: usize = 512 * 1024;

fn wait_for_inline_file_preview_loads(app: &mut App) {
    for _ in 0..200 {
        app.poll_inline_file_preview_loads();
        if app.inline_file_preview_state.pending.is_empty() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("inline file preview load did not complete");
}

struct InlinePreviewFixture {
    _root: tempfile::TempDir,
    session_repository: std::path::PathBuf,
}

impl InlinePreviewFixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("inline preview fixture root");
        let session_repository = root.path().join("session-repository");
        std::fs::create_dir_all(session_repository.join(".git"))
            .expect("create session repository");

        Self {
            _root: root,
            session_repository,
        }
    }

    fn write_bytes(
        &self,
        repository: &std::path::Path,
        relative_path: &str,
        content: impl AsRef<[u8]>,
    ) -> std::path::PathBuf {
        let path = repository.join(relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create fixture file parent");
        }
        std::fs::write(&path, content).expect("write inline preview fixture file");
        path
    }

    fn write_content_samples(&self) {
        self.write_bytes(
            &self.session_repository,
            "samples/markdown.md",
            "# Markdown\n",
        );
        self.write_bytes(
            &self.session_repository,
            "samples/utf8.txt",
            "ordinary UTF-8: café\n",
        );
        self.write_bytes(
            &self.session_repository,
            "samples/invalid-utf8.bin",
            [0xff, 0xfe, 0xfd],
        );
        self.write_bytes(
            &self.session_repository,
            "samples/nul-bearing.txt",
            b"before\0after",
        );
        self.write_bytes(&self.session_repository, "samples/empty.txt", []);
        self.write_bytes(
            &self.session_repository,
            "samples/at-limit.txt",
            vec![b'x'; INLINE_PREVIEW_BYTE_LIMIT],
        );
        self.write_bytes(
            &self.session_repository,
            "samples/over-limit.txt",
            vec![b'x'; INLINE_PREVIEW_BYTE_LIMIT + 1],
        );
    }

    #[cfg(unix)]
    fn symlink_file(
        &self,
        repository: &std::path::Path,
        relative_path: &str,
        target: &std::path::Path,
    ) -> std::io::Result<std::path::PathBuf> {
        let link = repository.join(relative_path);
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::os::unix::fs::symlink(target, &link)?;
        Ok(link)
    }

    #[cfg(windows)]
    fn symlink_file(
        &self,
        repository: &std::path::Path,
        relative_path: &str,
        target: &std::path::Path,
    ) -> std::io::Result<std::path::PathBuf> {
        let link = repository.join(relative_path);
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::os::windows::fs::symlink_file(target, &link)?;
        Ok(link)
    }
}

fn rendered_text_position(
    terminal: &ratatui::Terminal<ratatui::backend::TestBackend>,
    needle: &str,
    character_offset: u16,
) -> Option<(u16, u16)> {
    let buffer = terminal.backend().buffer();
    let area = *buffer.area();
    for row in 0..area.height {
        let mut line = String::new();
        for column in 0..area.width {
            line.push_str(buffer[(column, row)].symbol());
        }
        if let Some(byte) = line.find(needle) {
            return Some((line[..byte].chars().count() as u16 + character_offset, row));
        }
    }
    None
}

fn click_left(app: &mut App, column: u16, row: u16) {
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        app.handle_mouse_event(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::empty(),
        });
    }
}

#[test]
fn inline_preview_fixture_materializes_repository_and_content_variants() {
    let fixture = InlinePreviewFixture::new();
    fixture.write_content_samples();

    assert!(fixture.session_repository.join(".git").is_dir());
    assert_eq!(
        std::fs::metadata(fixture.session_repository.join("samples/at-limit.txt"))
            .expect("at-limit metadata")
            .len(),
        INLINE_PREVIEW_BYTE_LIMIT as u64
    );
    assert_eq!(
        std::fs::metadata(fixture.session_repository.join("samples/over-limit.txt"))
            .expect("over-limit metadata")
            .len(),
        INLINE_PREVIEW_BYTE_LIMIT as u64 + 1
    );
    assert!(
        std::fs::read(fixture.session_repository.join("samples/empty.txt"))
            .expect("read empty sample")
            .is_empty()
    );
    assert!(
        std::str::from_utf8(
            &std::fs::read(fixture.session_repository.join("samples/invalid-utf8.bin"))
                .expect("read invalid UTF-8 sample")
        )
        .is_err()
    );
    assert!(
        std::fs::read(fixture.session_repository.join("samples/nul-bearing.txt"))
            .expect("read NUL-bearing sample")
            .contains(&0)
    );
    assert_eq!(
        std::fs::read_to_string(fixture.session_repository.join("samples/markdown.md"))
            .expect("read Markdown sample"),
        "# Markdown\n"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.session_repository.join("samples/utf8.txt"))
            .expect("read UTF-8 sample"),
        "ordinary UTF-8: café\n"
    );

    #[cfg(unix)]
    {
        let outside = fixture.write_bytes(
            fixture._root.path(),
            "outside.txt",
            "outside repository boundary",
        );
        let link = fixture
            .symlink_file(&fixture.session_repository, "docs/escape.txt", &outside)
            .expect("create fixture symlink");
        assert_eq!(
            std::fs::canonicalize(link).expect("canonicalize symlink"),
            outside
        );
    }
}

#[test]
fn relative_markdown_path_previews_from_session_working_directory() {
    let fixture = InlinePreviewFixture::new();
    fixture.write_bytes(
        &fixture.session_repository,
        "docs/locus-cloud-architecture.md",
        "# Locus Cloud Architecture\n\nSession working directory content.\n",
    );

    let mut app = create_test_app();
    app.session.working_dir = Some(fixture.session_repository.display().to_string());
    app.display_messages = vec![DisplayMessage::assistant(
        "Open 'docs/locus-cloud-architecture.md'",
    )];
    app.bump_display_messages_version();

    assert!(app.try_toggle_inline_file_preview(
        "docs/locus-cloud-architecture.md",
        0,
    ));
    wait_for_inline_file_preview_loads(&mut app);

    let preview = app
        .inline_file_preview_state.loaded
        .values()
        .next()
        .expect("session working directory preview");
    assert_eq!(preview.display_path, "docs/locus-cloud-architecture.md");
    assert!(preview.markdown);
    assert!(preview.content.contains("Session working directory content"));
}

#[test]
fn missing_relative_path_does_not_search_sibling_directories() {
    let fixture = InlinePreviewFixture::new();
    let sibling = fixture._root.path().join("sibling-repository");
    fixture.write_bytes(&sibling, "docs/shared.md", "sibling content must not be used");

    let mut app = create_test_app();
    app.session.working_dir = Some(fixture.session_repository.display().to_string());
    app.display_messages = vec![DisplayMessage::assistant("docs/shared.md")];
    app.bump_display_messages_version();
    assert!(app.try_toggle_inline_file_preview("docs/shared.md", 0));
    wait_for_inline_file_preview_loads(&mut app);
    assert!(app.inline_file_preview_state.loaded.is_empty());
    assert_eq!(app.status_notice(), Some("File is not available: docs/shared.md".to_string()));
}

#[test]
fn line_and_column_suffix_preview_the_underlying_file() {
    let fixture = InlinePreviewFixture::new();
    fixture.write_bytes(&fixture.session_repository, "src/main.rs", "fn main() {}\n");
    let mut app = create_test_app();
    app.session.working_dir = Some(fixture.session_repository.display().to_string());
    app.display_messages = vec![DisplayMessage::assistant("src/main.rs:42:7")];
    app.bump_display_messages_version();

    assert!(app.try_toggle_inline_file_preview("src/main.rs:42:7", 0));
    wait_for_inline_file_preview_loads(&mut app);
    let preview = app.inline_file_preview_state.loaded.values().next().expect("preview");
    assert_eq!(preview.display_path, "src/main.rs");
    assert_eq!(preview.content, "fn main() {}\n");
}

#[test]
fn async_preview_results_stay_with_exact_duplicate_message_owners() {
    let mut app = create_test_app();
    app.display_messages = vec![
        DisplayMessage::assistant("same.txt"),
        DisplayMessage::assistant("same.txt"),
    ];
    app.bump_display_messages_version();

    for message_index in 0..2 {
        assert!(app.start_inline_file_preview_load_with(
            "same.txt".to_string(),
            message_index,
            move || {
                Ok(crate::tui::InlineFilePreview {
                    display_path: "same.txt".to_string(),
                    content: format!("owner {message_index}"),
                    markdown: false,
                })
            },
        ));
    }
    wait_for_inline_file_preview_loads(&mut app);

    assert_eq!(app.inline_file_preview_state.loaded.len(), 2);
    assert_eq!(
        app.inline_file_preview_state.loaded
            .get(&(0, app.display_messages[0].stable_cache_hash()))
            .map(|preview| preview.content.as_str()),
        Some("owner 0")
    );
    assert_eq!(
        app.inline_file_preview_state.loaded
            .get(&(1, app.display_messages[1].stable_cache_hash()))
            .map(|preview| preview.content.as_str()),
        Some("owner 1")
    );
}

#[test]
fn stale_async_preview_result_is_discarded_after_message_replacement() {
    let mut app = create_test_app();
    app.display_messages = vec![DisplayMessage::assistant("slow.txt")];
    app.bump_display_messages_version();
    let (release_sender, release_receiver) = std::sync::mpsc::channel();

    assert!(app.start_inline_file_preview_load_with(
        "slow.txt".to_string(),
        0,
        move || {
            release_receiver.recv().expect("release delayed preview");
            Ok(crate::tui::InlineFilePreview {
                display_path: "slow.txt".to_string(),
                content: "stale".to_string(),
                markdown: false,
            })
        },
    ));
    assert_eq!(app.inline_file_preview_state.pending.len(), 1);

    app.display_messages[0] = DisplayMessage::assistant("replacement.txt");
    app.bump_display_messages_version();
    release_sender.send(()).expect("release worker");
    wait_for_inline_file_preview_loads(&mut app);

    assert!(app.inline_file_preview_state.loaded.is_empty());
}

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

    let (path_col, path_row) = rendered_text_position(&terminal, "docs/guide.md", 3)
        .expect("path must be visible");
    click_left(&mut app, path_col, path_row);
    wait_for_inline_file_preview_loads(&mut app);

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

    let (path_col, path_row) = rendered_text_position(&terminal, "docs/guide.md", 3)
        .expect("path remains visible when expanded");
    click_left(&mut app, path_col, path_row);
    let collapsed_again = render_and_snap(&app, &mut terminal);
    assert!(!collapsed_again.contains("Inline file · docs/guide.md"));
    assert!(!collapsed_again.contains("Preview Heading"));
}

#[test]
fn test_click_on_absolute_file_path_followed_by_period_opens_inline_preview() {
    let _render_lock = scroll_render_test_lock();
    let repository = tempfile::tempdir().expect("repository tempdir");
    let file = repository.path().join("how-my-dev-workflow-works.md");
    std::fs::write(&file, "# Workflow\n\nWritten file content.\n").expect("write file");

    let mut app = create_test_app();
    app.display_messages = vec![DisplayMessage::assistant(format!(
        "Written to {}.",
        file.display()
    ))];
    app.bump_display_messages_version();

    let backend = ratatui::backend::TestBackend::new(120, 20);
    let mut terminal = ratatui::Terminal::new(backend).expect("create test terminal");
    render_and_snap(&app, &mut terminal);
    let buf = terminal.backend().buffer();
    let area = *buf.area();
    let (column, row) = (0..area.height)
        .find_map(|row| {
            let mut line = String::new();
            for column in 0..area.width {
                line.push_str(buf[(column, row)].symbol());
            }
            let byte = line.find(file.to_str()?)?;
            Some((line[..byte].chars().count() as u16 + 8, row))
        })
        .expect("absolute file path must be visible");

    let opened = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
    let opened_for_closure = opened.clone();
    assert!(app.try_open_link_at_with(column, row, |target| {
        *opened_for_closure.lock().unwrap() = Some(target.to_string());
        Ok::<(), &'static str>(())
    }));
    wait_for_inline_file_preview_loads(&mut app);

    assert_eq!(*opened.lock().unwrap(), None);
    let preview = app
        .inline_file_preview_state.loaded
        .values()
        .next()
        .expect("absolute local file should open in the inline preview");
    assert_eq!(preview.display_path, file.to_string_lossy());
    assert_eq!(preview.content, "# Workflow\n\nWritten file content.\n");
}

#[test]
fn test_clicking_html_file_uses_resolved_external_opener_by_default() {
    let _render_lock = scroll_render_test_lock();
    let repository = tempfile::tempdir().expect("repository tempdir");
    let html = repository.path().join("report.html");
    std::fs::write(&html, "<html><body>report</body></html>").expect("write html report");

    let mut app = create_test_app();
    app.session.working_dir = Some(repository.path().to_string_lossy().into_owned());
    app.display_messages = vec![DisplayMessage::assistant("Open report.html")];
    app.bump_display_messages_version();

    let backend = ratatui::backend::TestBackend::new(90, 20);
    let mut terminal = ratatui::Terminal::new(backend).expect("create test terminal");
    render_and_snap(&app, &mut terminal);
    let buf = terminal.backend().buffer();
    let area = *buf.area();
    let (column, row) = (0..area.height)
        .find_map(|row| {
            let mut line = String::new();
            for column in 0..area.width {
                line.push_str(buf[(column, row)].symbol());
            }
            let byte = line.find("report.html")?;
            Some((line[..byte].chars().count() as u16 + 2, row))
        })
        .expect("HTML path must be visible");

    let opened = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
    let opened_for_closure = opened.clone();
    let handled = app.try_open_link_at_with(column, row, |target| {
        *opened_for_closure.lock().unwrap() = Some(target.to_string());
        Ok::<(), &'static str>(())
    });

    assert!(handled);
    assert_eq!(
        *opened.lock().unwrap(),
        Some(html.to_string_lossy().into_owned()),
        "HTML file opening must resolve against the session working directory"
    );
    assert!(app.inline_file_preview_state.loaded.is_empty());
    assert_eq!(
        app.status_notice(),
        Some(format!("Opened file: {}", html.display()))
    );
}

#[test]
fn test_clicking_html_file_can_be_configured_to_use_inline_preview() {
    let _render_lock = scroll_render_test_lock();
    let repository = tempfile::tempdir().expect("repository tempdir");
    let html = repository.path().join("report.html");
    std::fs::write(&html, "<html><body>report</body></html>").expect("write html report");

    let mut app = create_test_app();
    app.session.working_dir = Some(repository.path().to_string_lossy().into_owned());
    app.display_messages = vec![DisplayMessage::assistant("Open report.html")];
    app.bump_display_messages_version();

    let backend = ratatui::backend::TestBackend::new(90, 20);
    let mut terminal = ratatui::Terminal::new(backend).expect("create test terminal");
    render_and_snap(&app, &mut terminal);
    let buf = terminal.backend().buffer();
    let area = *buf.area();
    let (column, row) = (0..area.height)
        .find_map(|row| {
            let mut line = String::new();
            for column in 0..area.width {
                line.push_str(buf[(column, row)].symbol());
            }
            let byte = line.find("report.html")?;
            Some((line[..byte].chars().count() as u16 + 2, row))
        })
        .expect("HTML path must be visible");

    let opened = std::sync::Arc::new(std::sync::Mutex::new(false));
    let opened_for_closure = opened.clone();
    let handled = app.try_open_link_at_with_mode(
        column,
        row,
        crate::config::HtmlFileOpenMode::Inline,
        |_| {
            *opened_for_closure.lock().unwrap() = true;
            Ok::<(), &'static str>(())
        },
    );
    wait_for_inline_file_preview_loads(&mut app);

    assert!(handled);
    assert!(!*opened.lock().unwrap());
    assert_eq!(app.inline_file_preview_state.loaded.len(), 1);
}

#[test]
fn test_relative_file_preview_falls_back_to_process_working_directory() {
    let _render_lock = scroll_render_test_lock();
    let mut app = create_test_app();
    app.display_messages = vec![DisplayMessage::assistant("`Cargo.toml`")];
    app.bump_display_messages_version();
    let opened = app.try_toggle_inline_file_preview("Cargo.toml", 0);
    wait_for_inline_file_preview_loads(&mut app);

    assert!(opened, "relative paths should resolve without a session working directory");
    assert_eq!(app.inline_file_preview_state.loaded.len(), 1);
}

#[test]
fn test_click_on_relative_file_uses_process_working_directory_without_session_cwd() {
    let _render_lock = scroll_render_test_lock();
    let mut app = create_test_app();
    app.display_messages = vec![DisplayMessage::assistant("`Cargo.toml`")];
    app.bump_display_messages_version();

    let backend = ratatui::backend::TestBackend::new(90, 20);
    let mut terminal = ratatui::Terminal::new(backend).expect("create test terminal");
    render_and_snap(&app, &mut terminal);

    let buf = terminal.backend().buffer();
    let area = *buf.area();
    let (column, row) = (0..area.height)
        .find_map(|row| {
            let mut line = String::new();
            for column in 0..area.width {
                line.push_str(buf[(column, row)].symbol());
            }
            let byte = line.find("Cargo.toml")?;
            Some((line[..byte].chars().count() as u16 + 3, row))
        })
        .expect("relative file must be visible");

    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        app.handle_mouse_event(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::empty(),
        });
    }
    wait_for_inline_file_preview_loads(&mut app);

    assert_eq!(app.inline_file_preview_state.loaded.len(), 1);
}

#[test]
fn test_click_on_home_relative_file_path_toggles_inline_preview() {
    let _render_lock = scroll_render_test_lock();
    let home = dirs::home_dir().expect("home directory");
    let home_file_dir = tempfile::tempdir_in(&home).expect("home-relative tempdir");
    let home_relative_dir = home_file_dir
        .path()
        .strip_prefix(&home)
        .expect("tempdir must be under home");
    let target = format!("~/{}", home_relative_dir.join("home-relative.txt").display());
    std::fs::write(home_file_dir.path().join("home-relative.txt"), "home-relative content")
        .expect("write home-relative file");

    let mut app = create_test_app();
    app.display_messages = vec![DisplayMessage::assistant(format!("Open `{target}`"))];
    app.bump_display_messages_version();
    app.scroll_offset = 0;
    app.auto_scroll_paused = false;
    app.is_processing = false;
    app.status = ProcessingStatus::Idle;

    let backend = ratatui::backend::TestBackend::new(100, 20);
    let mut terminal = ratatui::Terminal::new(backend).expect("create test terminal");
    render_and_snap(&app, &mut terminal);
    let buf = terminal.backend().buffer();
    let area = *buf.area();
    let (column, row) = (0..area.height)
        .find_map(|row| {
            let mut line = String::new();
            for column in 0..area.width {
                line.push_str(buf[(column, row)].symbol());
            }
            let byte = line.find(&target)?;
            Some((line[..byte].chars().count() as u16 + 1, row))
        })
        .expect("home-relative path must be visible");

    assert!(app.try_open_link_at(column, row));
    wait_for_inline_file_preview_loads(&mut app);
    let preview = app
        .inline_file_preview_state.loaded
        .values()
        .next()
        .expect("home-relative file preview");
    assert_eq!(preview.display_path, target);
    assert_eq!(preview.content, "home-relative content");
}

#[test]
fn test_clicking_file_mentions_in_user_and_prior_messages_uses_session_cwd() {
    let _render_lock = scroll_render_test_lock();
    let repository = tempfile::tempdir().expect("repository tempdir");
    std::fs::write(repository.path().join("current.txt"), "current mention content")
        .expect("write current file");
    std::fs::write(repository.path().join("prior.txt"), "prior mention content")
        .expect("write prior file");
    std::fs::write(repository.path().join("system.txt"), "system mention content")
        .expect("write system file");

    let mut app = create_test_app();
    app.session.working_dir = Some(repository.path().to_string_lossy().into_owned());
    app.display_messages = vec![
        DisplayMessage::user("open @current.txt"),
        DisplayMessage::assistant("Earlier reference: @prior.txt"),
        DisplayMessage::system("System reference: @system.txt"),
    ];
    app.bump_display_messages_version();
    app.scroll_offset = 0;
    app.auto_scroll_paused = false;
    app.is_processing = false;
    app.status = ProcessingStatus::Idle;

    let backend = ratatui::backend::TestBackend::new(100, 36);
    let mut terminal = ratatui::Terminal::new(backend).expect("create test terminal");
    render_and_snap(&app, &mut terminal);

    let locate = |terminal: &ratatui::Terminal<ratatui::backend::TestBackend>, needle: &str| {
        let buf = terminal.backend().buffer();
        let area = *buf.area();
        for row in 0..area.height {
            let mut line = String::new();
            for column in 0..area.width {
                line.push_str(buf[(column, row)].symbol());
            }
            if let Some(byte) = line.find(needle) {
                return Some((line[..byte].chars().count() as u16 + 1, row));
            }
        }
        None
    };
    let click = |app: &mut App, column: u16, row: u16| {
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            app.handle_mouse_event(MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::empty(),
            });
        }
    };

    let (column, row) = locate(&terminal, "@current.txt").expect("current mention visible");
    click(&mut app, column, row);
    wait_for_inline_file_preview_loads(&mut app);
    let current = render_and_snap(&app, &mut terminal);
    assert!(current.contains("Inline file · current.txt"));
    assert!(current.contains("current mention content"));

    let (column, row) = locate(&terminal, "@prior.txt").expect("prior mention visible");
    click(&mut app, column, row);
    wait_for_inline_file_preview_loads(&mut app);
    let prior = render_and_snap(&app, &mut terminal);
    assert!(prior.contains("Inline file · prior.txt"));
    assert!(prior.contains("prior mention content"));

    let (column, row) = locate(&terminal, "@system.txt").expect("system mention visible");
    click(&mut app, column, row);
    wait_for_inline_file_preview_loads(&mut app);
    let system = render_and_snap(&app, &mut terminal);
    assert!(system.contains("Inline file · system.txt"));
    assert!(system.contains("system mention content"));
    assert_eq!(app.inline_file_preview_state.loaded.len(), 3);
}

#[test]
fn test_clicking_invalid_file_mention_is_consumed_locally() {
    let _render_lock = scroll_render_test_lock();
    let repository = tempfile::tempdir().expect("repository tempdir");
    let mut app = create_test_app();
    app.session.working_dir = Some(repository.path().to_string_lossy().into_owned());
    app.display_messages = vec![DisplayMessage::assistant("Missing: @not-here.txt")];
    app.bump_display_messages_version();
    app.scroll_offset = 0;
    app.auto_scroll_paused = false;
    app.is_processing = false;
    app.status = ProcessingStatus::Idle;

    let backend = ratatui::backend::TestBackend::new(90, 20);
    let mut terminal = ratatui::Terminal::new(backend).expect("create test terminal");
    render_and_snap(&app, &mut terminal);
    let buf = terminal.backend().buffer();
    let area = *buf.area();
    let (column, row) = (0..area.height)
        .find_map(|row| {
            let mut line = String::new();
            for column in 0..area.width {
                line.push_str(buf[(column, row)].symbol());
            }
            let byte = line.find("@not-here.txt")?;
            Some((line[..byte].chars().count() as u16 + 1, row))
        })
        .expect("invalid mention visible");

    assert!(app.try_open_link_at(column, row));
    wait_for_inline_file_preview_loads(&mut app);
    assert!(app.inline_file_preview_state.loaded.is_empty());
    assert!(
        app.status_notice
            .as_ref()
            .is_some_and(|(notice, _)| notice == "File is not available: not-here.txt")
    );
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
    wait_for_inline_file_preview_loads(&mut app);

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
    wait_for_inline_file_preview_loads(&mut app);

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
        app.inline_file_preview_state.loaded.is_empty(),
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
    wait_for_inline_file_preview_loads(&mut app);

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
        !app.inline_file_preview_state.loaded.is_empty(),
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
    wait_for_inline_file_preview_loads(&mut app);
    assert!(app.inline_file_preview_state.loaded.is_empty());
    assert!(
        app.status_notice()
            .is_some_and(|notice| notice.contains("too large"))
    );

    assert!(app.try_toggle_inline_file_preview("binary.bin", 0));
    wait_for_inline_file_preview_loads(&mut app);
    assert!(app.inline_file_preview_state.loaded.is_empty());
    assert!(
        app.status_notice()
            .is_some_and(|notice| notice.contains("not readable text"))
    );
}

#[test]
fn clearing_transcript_releases_inline_file_preview_contents() {
    let mut app = create_test_app();
    app.push_display_message(DisplayMessage::user("src/main.rs"));
    let message_hash = app.display_messages[0].stable_cache_hash();
    app.inline_file_preview_state.loaded.insert(
        (0, message_hash),
        crate::tui::InlineFilePreview {
            display_path: "src/main.rs".to_string(),
            content: "fn main() {}".to_string(),
            markdown: false,
        },
    );
    let version = app.inline_file_preview_state.version;

    app.clear_display_messages();

    assert!(app.inline_file_preview_state.loaded.is_empty());
    assert_ne!(app.inline_file_preview_state.version, version);
}
