#[tokio::test]
async fn wake_turn_holds_reservation_until_terminal_status_is_published() {
    // Greptile review on #1166: releasing the guard before the terminal status
    // write let a newer wake's `running` be overwritten by this turn's `ready`.
    let provider = Arc::new(StreamingMockProvider::default());
    provider.queue_response(vec![
        StreamEvent::TextDelta("done".to_string()),
        StreamEvent::MessageEnd { stop_reason: None },
    ]);
    let provider_dyn: Arc<dyn Provider> = provider.clone();
    let agent = test_agent(provider_dyn).await;
    let session_id = agent.lock().await.session_id().to_string();
    let sessions = Arc::new(RwLock::new(HashMap::from([(
        session_id.clone(),
        agent.clone(),
    )])));
    let (member_event_tx, mut member_event_rx) = mpsc::unbounded_channel();
    let member = attached_swarm_member(&session_id, member_event_tx);
    let swarm_members = Arc::new(RwLock::new(HashMap::from([(session_id.clone(), member)])));
    let (swarms_by_id, event_history, event_counter, swarm_event_tx) = empty_swarm_status_state();
    let ctx = super::live_turn::LiveTurnSwarmContext::new(
        &swarm_members,
        &swarms_by_id,
        &event_history,
        &event_counter,
        &swarm_event_tx,
    );

    let started = super::live_turn::run_live_turn_if_idle(
        &session_id,
        "first wake",
        None,
        &sessions,
        ctx.clone(),
    )
    .await;
    assert!(started);

    // Wait for the terminal Done fanout.
    timeout(Duration::from_secs(2), async {
        loop {
            match member_event_rx.recv().await {
                Some(ServerEvent::Done { .. }) => return,
                Some(_) => continue,
                None => panic!("member stream closed"),
            }
        }
    })
    .await
    .expect("wake turn should finish");

    // Whenever a second reservation succeeds, the first turn must already have
    // published its terminal status: the guard outlives the status update.
    let reacquired = timeout(Duration::from_secs(2), async {
        loop {
            if let Some(guard) =
                super::live_turn::idle_live_agent(&session_id, &sessions, &swarm_members).await
            {
                return guard;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("reservation should be released after the turn");
    let status = swarm_members
        .read()
        .await
        .get(&session_id)
        .map(|m| m.status.clone());
    assert_eq!(status.as_deref(), Some("ready"));
    drop(reacquired);
}
