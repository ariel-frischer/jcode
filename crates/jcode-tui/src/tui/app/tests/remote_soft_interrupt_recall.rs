fn remote_alt_q_request(app: &mut App) -> crate::protocol::Request {
    use tokio::io::AsyncBufReadExt;

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let mut remote = crate::tui::backend::RemoteConnection::dummy();
        let peer = remote
            .take_dummy_peer()
            .expect("dummy remote should retain peer stream");
        let (reader, _writer) = peer.into_split();
        let mut reader = tokio::io::BufReader::new(reader);

        app.handle_remote_key(KeyCode::Char('q'), KeyModifiers::ALT, &mut remote)
            .await
            .expect("Alt+q should be handled");

        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(1), reader.read_line(&mut line))
            .await
            .expect("Alt+q should send a recall request")
            .expect("recall request should be readable by peer");
        serde_json::from_str(&line).expect("recall request should deserialize")
    })
}

fn recall_operation_id(request: crate::protocol::Request) -> String {
    let crate::protocol::Request::RecallSoftInterrupt { operation_id, .. } = request else {
        panic!("expected RecallSoftInterrupt request, got {request:?}");
    };
    operation_id
}

#[test]
fn remote_soft_interrupt_recall_requires_an_empty_text_and_image_composer() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");

    for with_image in [false, true] {
        let mut app = create_test_app();
        app.pending_soft_interrupts = vec!["server pending".to_string()];
        if with_image {
            app.pending_images = vec![("image/png".to_string(), "cG5n".to_string())];
        } else {
            app.input = "current draft".to_string();
            app.cursor_pos = app.input.len();
        }
        let original_input = app.input.clone();
        let original_images = app.pending_images.clone();
        let original_pending = app.pending_soft_interrupts.clone();

        rt.block_on(async {
            let mut remote = crate::tui::backend::RemoteConnection::dummy();
            app.handle_remote_key(KeyCode::Char('q'), KeyModifiers::ALT, &mut remote)
                .await
                .expect("Alt+q should be handled");
            assert_eq!(remote.next_request_id_for_test(), 1);
        });

        assert_eq!(app.input, original_input);
        assert_eq!(app.pending_images, original_images);
        assert_eq!(app.pending_soft_interrupts, original_pending);
    }
}

#[test]
fn remote_soft_interrupt_recall_sends_only_one_request_while_pending() {
    let mut app = create_test_app();
    app.pending_soft_interrupts = vec!["server pending".to_string()];
    let rt = tokio::runtime::Runtime::new().expect("runtime");

    rt.block_on(async {
        let mut remote = crate::tui::backend::RemoteConnection::dummy();
        app.handle_remote_key(KeyCode::Char('q'), KeyModifiers::ALT, &mut remote)
            .await
            .expect("first Alt+q should be handled");
        assert_eq!(remote.next_request_id_for_test(), 2);

        app.handle_remote_key(KeyCode::Char('q'), KeyModifiers::ALT, &mut remote)
            .await
            .expect("repeated Alt+q should be handled");
        assert_eq!(remote.next_request_id_for_test(), 2);
    });

    assert!(app.input.is_empty());
    assert!(app.pending_images.is_empty());
    assert_eq!(app.pending_soft_interrupts, ["server pending"]);
}

#[test]
fn remote_soft_interrupt_recall_matching_result_applies_exact_payload_once() {
    let mut app = create_test_app();
    app.pending_soft_interrupts = vec!["server pending".to_string()];
    let operation_id = recall_operation_id(remote_alt_q_request(&mut app));
    let message = crate::protocol::RecallableSoftInterrupt {
        content: "edit this queued message".to_string(),
        images: vec![
            ("image/png".to_string(), "cG5n".to_string()),
            ("image/jpeg".to_string(), "anBlZw==".to_string()),
        ],
    };
    let event = crate::protocol::ServerEvent::SoftInterruptRecalled {
        id: 1,
        operation_id,
        message: Some(message.clone()),
    };
    let mut remote = crate::tui::backend::RemoteConnection::dummy();

    super::remote::handle_server_event(&mut app, event.clone(), &mut remote);
    assert_eq!(app.input, message.content);
    assert_eq!(app.cursor_pos, app.input.len());
    assert_eq!(app.pending_images, message.images);

    app.input.clear();
    app.cursor_pos = 0;
    app.pending_images.clear();
    super::remote::handle_server_event(&mut app, event, &mut remote);
    assert!(app.input.is_empty());
    assert!(app.pending_images.is_empty());
}

#[test]
fn remote_soft_interrupt_recall_failed_stale_and_disconnect_preserve_state() {
    let mut failed_app = create_test_app();
    failed_app.pending_soft_interrupts = vec!["server pending".to_string()];
    let operation_id = recall_operation_id(remote_alt_q_request(&mut failed_app));
    let original_pending = failed_app.pending_soft_interrupts.clone();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();

    super::remote::handle_server_event(
        &mut failed_app,
        crate::protocol::ServerEvent::SoftInterruptRecalled {
            id: 1,
            operation_id: operation_id.clone(),
            message: None,
        },
        &mut remote,
    );
    assert!(failed_app.input.is_empty());
    assert!(failed_app.pending_images.is_empty());
    assert_eq!(failed_app.pending_soft_interrupts, original_pending);

    let mut stale_app = create_test_app();
    stale_app.pending_soft_interrupts = vec!["server pending".to_string()];
    let _expected_operation = recall_operation_id(remote_alt_q_request(&mut stale_app));
    let stale_pending = stale_app.pending_soft_interrupts.clone();
    super::remote::handle_server_event(
        &mut stale_app,
        crate::protocol::ServerEvent::SoftInterruptRecalled {
            id: 99,
            operation_id: "different-operation".to_string(),
            message: Some(crate::protocol::RecallableSoftInterrupt {
                content: "must not apply".to_string(),
                images: vec![("image/png".to_string(), "bad".to_string())],
            }),
        },
        &mut remote,
    );
    assert!(stale_app.input.is_empty());
    assert!(stale_app.pending_images.is_empty());
    assert_eq!(stale_app.pending_soft_interrupts, stale_pending);

    let mut disconnected_app = create_test_app();
    disconnected_app.pending_soft_interrupts = vec!["server pending".to_string()];
    let _operation_id = recall_operation_id(remote_alt_q_request(&mut disconnected_app));
    let disconnected_pending = disconnected_app.pending_soft_interrupts.clone();
    let mut state = super::remote::RemoteRunState::default();
    super::remote::handle_disconnect(
        &mut disconnected_app,
        &mut state,
        Some(crate::tui::backend::RemoteDisconnectReason::PeerClosed),
    );
    assert!(disconnected_app.input.is_empty());
    assert!(disconnected_app.pending_images.is_empty());
    assert_eq!(
        disconnected_app.pending_soft_interrupts,
        disconnected_pending
    );
}

#[test]
fn remote_soft_interrupt_recall_preserves_immediate_local_queue_recall() {
    let mut app = create_test_app();
    app.queued_messages = vec!["first".to_string(), "second".to_string()];
    app.pending_soft_interrupts = vec!["server pending".to_string()];
    let rt = tokio::runtime::Runtime::new().expect("runtime");

    rt.block_on(async {
        let mut remote = crate::tui::backend::RemoteConnection::dummy();
        app.handle_remote_key(KeyCode::Char('q'), KeyModifiers::ALT, &mut remote)
            .await
            .expect("Alt+q should recall local queue first");
        assert_eq!(remote.next_request_id_for_test(), 1);
    });

    assert_eq!(app.input, "second");
    assert_eq!(app.queued_messages, ["first"]);
    assert_eq!(app.pending_soft_interrupts, ["server pending"]);
}
