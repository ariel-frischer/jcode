use super::*;

#[test]
fn session_handoff_ready_restores_destination_startup_context_before_resume() {
    ensure_test_jcode_home_if_unset();
    let destination = "session_handoff_context_test";
    crate::client_input::save_startup_queued_message_for_session(
        destination,
        "continue from the handoff summary".to_string(),
    );

    let mut app = create_test_app();
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let _guard = rt.enter();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();

    app.handle_server_event(
        ServerEvent::SessionHandoffReady {
            id: 1,
            source_session_id: app.session.id.clone(),
            new_session_id: destination.to_string(),
            new_session_name: "oak".to_string(),
            auto_start: true,
        },
        &mut remote,
    );

    assert!(app.input.is_empty());
    assert!(!app.submit_input_on_startup);
    assert_eq!(app.queued_messages, ["continue from the handoff summary"]);
    assert!(
        remote.resume_in_flight(),
        "SessionHandoffReady must arm the resume barrier before the next tick consumes the target"
    );
    assert!(
        app.display_messages().is_empty(),
        "handoff reminder should wait for destination history rather than appearing in the source transcript"
    );
}

#[test]
fn catchup_resume_tick_sends_request_and_tracks_in_flight_state() {
    use tokio::io::AsyncBufReadExt;

    let mut app = create_test_app();
    app.queue_catchup_resume(
        "session_otter_1785728596263_80eb5ad6012a1864".to_string(),
        Some("source-session".to_string()),
        Some((1, 2)),
        true,
    );

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let request = rt.block_on(async {
        let mut remote = crate::tui::backend::RemoteConnection::dummy();
        let peer = remote
            .take_dummy_peer()
            .expect("dummy remote should retain peer stream");
        let (reader, _writer) = peer.into_split();
        let mut reader = tokio::io::BufReader::new(reader);

        assert!(handle_tick(&mut app, &mut remote).await);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .expect("resume request should be readable by peer");
        serde_json::from_str::<crate::protocol::Request>(&line)
            .expect("resume request should deserialize")
    });

    match request {
        crate::protocol::Request::ResumeSession { session_id, .. } => {
            assert_eq!(session_id, "session_otter_1785728596263_80eb5ad6012a1864");
        }
        other => panic!("expected ResumeSession request, got {other:?}"),
    }
    assert!(app.pending_catchup_resume.is_none());
    assert_eq!(
        app.in_flight_catchup_resume
            .as_ref()
            .map(|request| request.target_session_id.as_str()),
        Some("session_otter_1785728596263_80eb5ad6012a1864")
    );
    assert_eq!(
        app.catchup_return_stack.last().map(String::as_str),
        Some("source-session")
    );
}

#[test]
fn catchup_resume_tick_reports_send_failure_without_stranding_state() {
    let mut app = create_test_app();
    app.queue_catchup_resume("unreachable-session".to_string(), None, None, false);

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let needs_redraw = rt.block_on(async {
        let mut remote = crate::tui::backend::RemoteConnection::dummy();
        drop(remote.take_dummy_peer().expect("dummy peer"));
        handle_tick(&mut app, &mut remote).await
    });

    assert!(needs_redraw);
    assert!(app.pending_catchup_resume.is_none());
    assert!(app.in_flight_catchup_resume.is_none());
    assert!(app.display_messages().iter().any(|message| {
        message
            .content
            .contains("Failed to switch Catch Up session")
    }));
}
