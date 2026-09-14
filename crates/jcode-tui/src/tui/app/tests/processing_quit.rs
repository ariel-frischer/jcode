#[test]
fn test_expired_ctrl_c_confirmation_does_not_quit_while_processing() {
    let mut app = create_test_app();
    app.is_processing = true;
    app.quit_pending = Some(std::time::Instant::now() - std::time::Duration::from_secs(3));

    app.handle_key(KeyCode::Char('c'), KeyModifiers::CONTROL)
        .unwrap();

    assert!(!app.should_quit);
    assert!(app.cancel_requested);
    assert!(app.quit_pending.unwrap().elapsed() < std::time::Duration::from_secs(1));
}

#[test]
fn test_remote_expired_ctrl_c_confirmation_keeps_interrupting() {
    let mut app = create_test_app();
    app.is_processing = true;
    app.quit_pending = Some(std::time::Instant::now() - std::time::Duration::from_secs(3));
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();

    rt.block_on(app.handle_remote_key(KeyCode::Char('c'), KeyModifiers::CONTROL, &mut remote))
        .unwrap();

    assert!(!app.should_quit);
    assert!(app.is_processing);
    assert!(app.quit_pending.unwrap().elapsed() < std::time::Duration::from_secs(1));
}

#[test]
fn test_repeated_ctrl_c_quits_while_processing_without_cancel_ack() {
    let mut app = create_test_app();
    app.is_processing = true;

    app.handle_key(KeyCode::Char('c'), KeyModifiers::CONTROL)
        .unwrap();
    assert!(app.quit_pending.is_some());
    assert!(!app.should_quit);

    app.handle_key(KeyCode::Char('c'), KeyModifiers::CONTROL)
        .unwrap();
    assert!(app.should_quit);
}

#[test]
fn test_remote_repeated_ctrl_c_quits_without_waiting_for_cancel_ack() {
    let mut app = create_test_app();
    app.is_processing = true;
    app.status = ProcessingStatus::Streaming;
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();

    rt.block_on(app.handle_remote_key(KeyCode::Char('c'), KeyModifiers::CONTROL, &mut remote))
        .unwrap();
    assert!(app.quit_pending.is_some());
    assert!(!app.should_quit);

    rt.block_on(app.handle_remote_key(KeyCode::Char('c'), KeyModifiers::CONTROL, &mut remote))
        .unwrap();
    assert!(app.should_quit);
}
