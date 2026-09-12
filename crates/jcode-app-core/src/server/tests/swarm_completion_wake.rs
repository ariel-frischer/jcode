use super::*;

#[tokio::test]
async fn swarm_completion_wake_emits_once_per_owned_terminal_transition() {
    let _env_lock = crate::storage::lock_test_env();
    let mut bus = crate::bus::Bus::global().subscribe();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let mut owner = attached_swarm_member("completion-owner", event_tx.clone());
    owner.swarm_id = Some("completion-test".to_string());
    let mut worker = attached_swarm_member("completion-worker", event_tx);
    worker.swarm_id = owner.swarm_id.clone();
    worker.report_back_to_session_id = Some(owner.session_id.clone());
    worker.status = "running".to_string();
    let members = Arc::new(RwLock::new(HashMap::from([
        (owner.session_id.clone(), owner),
        (worker.session_id.clone(), worker),
    ])));
    let (swarms, history, counter, events) = empty_swarm_status_state();
    for status in ["ready", "ready", "completed"] {
        super::super::swarm::update_member_status_with_report(
            "completion-worker",
            status,
            None,
            Some("Verified result".to_string()),
            &members,
            &swarms,
            Some(&history),
            Some(&counter),
            Some(&events),
        )
        .await;
    }
    let mut completions = Vec::new();
    while let Ok(event) = bus.try_recv() {
        if let crate::bus::BusEvent::SwarmMemberCompleted(event) = event {
            if event.worker_session_id == "completion-worker" {
                completions.push(event);
            }
        }
    }
    assert_eq!(
        completions.len(),
        1,
        "a completion signal must not depend on an explicit wait"
    );
    assert_eq!(completions[0].owner_session_id, "completion-owner");
    assert!(completions[0].notification.contains("Verified result"));
}

#[tokio::test]
async fn swarm_completion_wake_rechecks_queue_after_busy_turn_releases_agent() {
    let _env_lock = crate::storage::lock_test_env();
    let _enabled = ScopedEnvVar::set("JCODE_SWARM_COMPLETION_WAKE", "true");
    let _wake_mode = ScopedEnvVar::set("JCODE_WAKE_MODE", "internal");
    let home = tempfile::tempdir().expect("isolated runtime");
    let _environment = configure_test_env(&home);
    crate::config::invalidate_config_cache();
    let provider = Arc::new(StreamingMockProvider::default());
    provider.queue_response(vec![
        StreamEvent::TextDelta("Completion processed.".to_string()),
        StreamEvent::MessageEnd { stop_reason: None },
    ]);
    let agent = test_agent(provider).await;
    let reservation = agent.clone().lock_owned().await;
    let session_id = reservation.session_id().to_string();
    let queue = reservation.soft_interrupt_queue();
    let sessions = Arc::new(RwLock::new(HashMap::from([(
        session_id.clone(),
        agent.clone(),
    )])));
    let queues = Arc::new(RwLock::new(HashMap::from([(
        session_id.clone(),
        queue.clone(),
    )])));
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let members = Arc::new(RwLock::new(HashMap::from([(
        session_id.clone(),
        attached_swarm_member(&session_id, event_tx),
    )])));
    let (swarms, history, counter, events) = empty_swarm_status_state();
    let completion = crate::bus::SwarmAwaitCompleted {
        session_id: session_id.clone(),
        completed: true,
        summary: "Worker finished".to_string(),
        notification: "Final worker result".to_string(),
        notify: false,
        wake: true,
    };
    super::super::background_tasks::dispatch_swarm_await_completion(
        &completion,
        &sessions,
        &queues,
        &members,
        &swarms,
        &history,
        &counter,
        &events,
    )
    .await;
    assert_eq!(queue.lock().expect("queue").len(), 1);
    // This represents the end-of-turn window after the final queue drain but
    // before releasing the agent reservation. No further model output occurs.
    drop(reservation);
    timeout(Duration::from_secs(2), async {
        while let Some(event) = event_rx.recv().await {
            if matches!(event, ServerEvent::Done { .. }) {
                return;
            }
        }
        panic!("event stream closed before completion wake");
    })
    .await
    .expect("completion must wake after the busy reservation is released");
    let agent = agent.lock().await;
    assert!(queue.lock().expect("queue").is_empty());
    assert_eq!(
        agent
            .messages()
            .iter()
            .filter(|m| m.role == Role::User && m.content_preview().contains("Final worker result"))
            .count(),
        1
    );
}
