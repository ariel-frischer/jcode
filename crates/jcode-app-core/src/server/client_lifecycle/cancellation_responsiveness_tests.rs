#[tokio::test]
async fn resume_working_dir_snapshot_uses_member_metadata_while_agent_is_busy() {
    let provider: Arc<dyn Provider> = Arc::new(PanicOnForkProvider {
        forked: Arc::new(AtomicBool::new(false)),
    });
    let registry = Registry::new(Arc::clone(&provider)).await;
    let session_id = "session_busy_resume_snapshot";
    let mut session = crate::session::Session::create_with_id(session_id.to_string(), None, None);
    session.working_dir = Some("/workspace/agent-root".to_string());
    let agent = Arc::new(Mutex::new(Agent::new_with_session(
        provider, registry, session, None,
    )));

    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let now = Instant::now();
    let members = Arc::new(RwLock::new(HashMap::from([(
        session_id.to_string(),
        SwarmMember {
            session_id: session_id.to_string(),
            event_tx,
            event_txs: HashMap::new(),
            working_dir: Some("/workspace/agent-root".into()),
            swarm_id: None,
            swarm_enabled: false,
            status: "running".to_string(),
            detail: None,
            task_label: None,
            friendly_name: None,
            report_back_to_session_id: None,
            latest_completion_report: None,
            role: "agent".to_string(),
            joined_at: now,
            last_status_change: now,
            is_headless: false,
            output_tail: None,
            todo_progress: None,
            todo_items: Vec::new(),
            runtime: crate::protocol::SwarmMemberRuntime::default(),
        },
    )])));
    let _busy_agent_lock = agent.lock().await;

    let working_dir = tokio::time::timeout(
        Duration::from_millis(100),
        resolve_resume_working_dir(&agent, session_id, &members),
    )
    .await
    .expect("resume metadata lookup must not wait for the busy Agent");
    assert_eq!(working_dir.as_deref(), Some("/workspace/agent-root"));
}

#[tokio::test]
async fn busy_attached_message_can_be_cancelled_before_agent_lock() {
    let forked = Arc::new(AtomicBool::new(false));
    let provider: Arc<dyn Provider> = Arc::new(PanicOnForkProvider {
        forked: Arc::clone(&forked),
    });
    let registry = Registry::new(Arc::clone(&provider)).await;
    let agent = Arc::new(Mutex::new(Agent::new(provider, registry)));
    let _busy_agent_lock = agent.lock().await;

    let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel::<ServerEvent>();
    let (processing_done_tx, mut processing_done_rx) = mpsc::unbounded_channel();
    let mut client_is_processing = false;
    let mut processing_message_id = None;
    let mut processing_session_id = None;
    let mut processing_task = None;
    let mut processing_control = None;
    let swarm_members = Arc::new(RwLock::new(HashMap::new()));
    let swarms_by_id = Arc::new(RwLock::new(HashMap::new()));
    let event_history = Arc::new(RwLock::new(std::collections::VecDeque::new()));
    let event_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (swarm_event_tx, _) = broadcast::channel(8);
    let queue = Arc::new(std::sync::Mutex::new(Vec::new()));
    let source_stop = InterruptSignal::new();
    let control = SessionControlHandle::new(
        "session_busy_message_dispatch",
        queue,
        InterruptSignal::new(),
        source_stop.clone(),
    );
    let target_stop = InterruptSignal::new();
    let switched_control = SessionControlHandle::new(
        "session_newly_selected_target",
        Arc::new(std::sync::Mutex::new(Vec::new())),
        InterruptSignal::new(),
        target_stop.clone(),
    );
    let swarm = SwarmStatusRefs {
        members: &swarm_members,
        swarms_by_id: &swarms_by_id,
        event_history: &event_history,
        event_counter: &event_counter,
        event_tx: &swarm_event_tx,
    };

    tokio::time::timeout(
        Duration::from_millis(100),
        start_processing_message(
            ProcessingMessage {
                id: 900,
                content: "queued while the attached session is busy".to_string(),
                images: Vec::new(),
                system_reminder: None,
                active_skill: None,
                run_safety: None,
            },
            "session_busy_message_dispatch",
            &mut ProcessingState {
                client_is_processing: &mut client_is_processing,
                message_id: &mut processing_message_id,
                session_id: &mut processing_session_id,
                task: &mut processing_task,
                processing_control: &mut processing_control,
            },
            &control,
            &agent,
            &client_event_tx,
            &processing_done_tx,
            Vec::new(),
            &swarm,
        ),
    )
    .await
    .expect("message dispatch must not wait for the busy Agent mutex");
    assert!(client_is_processing);
    assert_eq!(processing_message_id, Some(900));
    assert!(processing_task.is_some());

    tokio::time::timeout(
        Duration::from_secs(3),
        cancel_processing_message(
            &mut ProcessingState {
                client_is_processing: &mut client_is_processing,
                message_id: &mut processing_message_id,
                session_id: &mut processing_session_id,
                task: &mut processing_task,
                processing_control: &mut processing_control,
            },
            &switched_control,
            &client_event_tx,
            &swarm,
            Some(901),
            None,
        ),
    )
    .await
    .expect("Cancel must remain responsive while the message waits for Agent");

    assert!(source_stop.epoch() > 0);
    assert_eq!(target_stop.epoch(), 0);
    assert!(!client_is_processing);
    assert!(processing_task.is_none());
    assert_eq!(processing_message_id, None);
    assert!(processing_done_rx.try_recv().is_err());
    assert!(matches!(
        client_event_rx.try_recv(),
        Ok(ServerEvent::TurnStopped {
            reason: crate::protocol::TurnStopReason::Interrupted,
            ..
        })
    ));
    assert!(matches!(
        client_event_rx.try_recv(),
        Ok(ServerEvent::Interrupted)
    ));
    assert!(!forked.load(Ordering::SeqCst));
}

#[tokio::test]
async fn cancel_without_local_task_still_signals_session_control() {
    let soft_interrupt_queue = Arc::new(std::sync::Mutex::new(Vec::new()));
    let stop_signal = InterruptSignal::new();
    let control = SessionControlHandle::cancel_only(
        "session_detached_cancel",
        soft_interrupt_queue,
        stop_signal.clone(),
    );
    // The point of this path is a turn this connection does not own (attach
    // after reload, server-initiated turn). Without a registered active turn
    // the cancel is a deliberate no-op, because arming the signal with nothing
    // running only kills the *next* message.
    let _active_turn = crate::turn_cancel_registry::register_active_turn(
        "session_detached_cancel",
        InterruptSignal::new(),
    );
    let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel::<ServerEvent>();
    let swarm_members = Arc::new(RwLock::new(HashMap::new()));
    let swarms_by_id = Arc::new(RwLock::new(HashMap::new()));
    let event_history = Arc::new(RwLock::new(std::collections::VecDeque::new()));
    let event_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (swarm_event_tx, _) = broadcast::channel(8);
    let mut client_is_processing = true;
    let mut message_id = Some(99);
    let mut session_id = Some("session_detached_cancel".to_string());
    let mut task = None;
    let mut processing_control = None;

    cancel_processing_message(
        &mut ProcessingState {
            client_is_processing: &mut client_is_processing,
            message_id: &mut message_id,
            session_id: &mut session_id,
            task: &mut task,
            processing_control: &mut processing_control,
        },
        &control,
        &client_event_tx,
        &SwarmStatusRefs {
            members: &swarm_members,
            swarms_by_id: &swarms_by_id,
            event_history: &event_history,
            event_counter: &event_counter,
            event_tx: &swarm_event_tx,
        },
        Some(99),
        None,
    )
    .await;

    assert!(stop_signal.is_set());
    assert!(!client_is_processing);
    assert!(message_id.is_none());
    assert!(session_id.is_none());
    assert!(matches!(
        client_event_rx.recv().await,
        Some(ServerEvent::TurnStopped {
            reason: crate::protocol::TurnStopReason::Interrupted,
            ..
        })
    ));
    assert!(matches!(
        client_event_rx.recv().await,
        Some(ServerEvent::Interrupted)
    ));
    assert!(matches!(
        client_event_rx.recv().await,
        Some(ServerEvent::Done { id: 99 })
    ));
}
