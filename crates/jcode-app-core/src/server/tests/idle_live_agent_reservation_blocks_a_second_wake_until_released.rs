#[tokio::test]
async fn idle_live_agent_reservation_blocks_a_second_wake_until_released() {
    // Regression for #1152: the idle check used to drop its try_lock guard
    // before the turn started, so two concurrent wakes could both succeed.
    let provider: Arc<dyn Provider> = Arc::new(StreamingMockProvider::default());
    let agent = test_agent(provider).await;
    let session_id = agent.lock().await.session_id().to_string();
    let sessions = Arc::new(RwLock::new(HashMap::from([(
        session_id.clone(),
        agent.clone(),
    )])));
    let (member_event_tx, _member_event_rx) = mpsc::unbounded_channel();
    let member = attached_swarm_member(&session_id, member_event_tx);
    let swarm_members = Arc::new(RwLock::new(HashMap::from([(session_id.clone(), member)])));

    let first = super::live_turn::idle_live_agent(&session_id, &sessions, &swarm_members).await;
    assert!(first.is_some(), "idle live session should be reservable");

    let second = super::live_turn::idle_live_agent(&session_id, &sessions, &swarm_members).await;
    assert!(
        second.is_none(),
        "second reservation must fail while the first guard is alive"
    );

    drop(first);
    let third = super::live_turn::idle_live_agent(&session_id, &sessions, &swarm_members).await;
    assert!(
        third.is_some(),
        "reservation is available again once released"
    );
}
