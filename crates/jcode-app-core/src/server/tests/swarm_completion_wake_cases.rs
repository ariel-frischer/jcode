use super::super::live_turn::LiveTurnSwarmContext;
use super::super::swarm_completion_wake::dispatch_member_completion;
use super::*;

struct Fixture {
    _environment: EnvGuard,
    _home: tempfile::TempDir,
    agent: Arc<Mutex<Agent>>,
    session_id: String,
    sessions: super::super::SessionAgents,
    queues: SessionInterruptQueues,
    swarm: LiveTurnSwarmContext,
    events: mpsc::UnboundedReceiver<ServerEvent>,
}

impl Fixture {
    async fn new() -> Self {
        let home = tempfile::tempdir().expect("isolated runtime");
        let environment = configure_test_env(&home);
        crate::config::invalidate_config_cache();
        let provider = Arc::new(StreamingMockProvider::default());
        provider.queue_response(vec![
            StreamEvent::TextDelta("Completion processed.".to_string()),
            StreamEvent::MessageEnd { stop_reason: None },
        ]);
        provider.queue_response(vec![
            StreamEvent::TextDelta("Remaining completion processed.".to_string()),
            StreamEvent::MessageEnd { stop_reason: None },
        ]);
        let agent = test_agent(provider).await;
        let guard = agent.lock().await;
        let session_id = guard.session_id().to_string();
        let queue = guard.soft_interrupt_queue();
        drop(guard);
        let sessions = Arc::new(RwLock::new(HashMap::from([(
            session_id.clone(),
            agent.clone(),
        )])));
        let queues = Arc::new(RwLock::new(HashMap::from([(session_id.clone(), queue)])));
        let (event_tx, events) = mpsc::unbounded_channel();
        let mut owner = attached_swarm_member(&session_id, event_tx.clone());
        owner.swarm_id = Some(session_id.clone());
        let mut finished = attached_swarm_member("finished-worker", event_tx.clone());
        finished.swarm_id = Some(session_id.clone());
        finished.report_back_to_session_id = Some(session_id.clone());
        let mut busy = attached_swarm_member("busy-worker", event_tx);
        busy.swarm_id = Some(session_id.clone());
        busy.report_back_to_session_id = Some(session_id.clone());
        busy.status = "running".to_string();
        let members = Arc::new(RwLock::new(HashMap::from([
            (session_id.clone(), owner),
            (finished.session_id.clone(), finished),
            (busy.session_id.clone(), busy),
        ])));
        let (swarms, history, counter, event_tx) = empty_swarm_status_state();
        let swarm = LiveTurnSwarmContext::new(&members, &swarms, &history, &counter, &event_tx);
        Self {
            _environment: environment,
            _home: home,
            agent,
            session_id,
            sessions,
            queues,
            swarm,
            events,
        }
    }

    fn completion(&self) -> crate::bus::SwarmMemberCompleted {
        crate::bus::SwarmMemberCompleted {
            owner_session_id: self.session_id.clone(),
            worker_session_id: "finished-worker".to_string(),
            swarm_id: self.session_id.clone(),
            notification: "First worker result".to_string(),
        }
    }

    async fn dispatch(&self, event: &crate::bus::SwarmMemberCompleted) {
        dispatch_member_completion(event, &self.sessions, &self.queues, self.swarm.clone()).await;
    }

    async fn done(&mut self) {
        timeout(Duration::from_secs(2), async {
            while let Some(event) = self.events.recv().await {
                if matches!(event, ServerEvent::Done { .. }) {
                    return;
                }
            }
            panic!("event stream closed");
        })
        .await
        .expect("completion wake should finish");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        crate::config::invalidate_config_cache();
    }
}

#[tokio::test]
async fn swarm_completion_wake_routes_status_through_the_production_bus_monitor() {
    let _lock = crate::storage::lock_test_env();
    let _enabled = ScopedEnvVar::set("JCODE_SWARM_COMPLETION_WAKE", "true");
    let _internal = ScopedEnvVar::set("JCODE_WAKE_MODE", "internal");
    let mut fixture = Fixture::new().await;
    fixture
        .swarm
        .members
        .write()
        .await
        .get_mut("finished-worker")
        .unwrap()
        .status = "running".to_string();
    let mut monitor = Box::pin(Server::monitor_bus(
        super::super::FileTouchService::new(),
        fixture.swarm.members.clone(),
        fixture.swarm.swarms_by_id.clone(),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(RwLock::new(HashMap::new())),
        fixture.sessions.clone(),
        fixture.queues.clone(),
        fixture.swarm.event_history.clone(),
        fixture.swarm.event_counter.clone(),
        fixture.swarm.event_tx.clone(),
    ));
    // Poll once to establish the real bus subscription before publishing.
    assert!(futures::poll!(&mut monitor).is_pending());
    let monitor = tokio::spawn(monitor);
    for _ in 0..2 {
        super::super::update_member_status_with_report_tldr(
            "finished-worker",
            "ready",
            None,
            Some("Bus pipeline result".to_string()),
            None,
            &fixture.swarm.members,
            &fixture.swarm.swarms_by_id,
            Some(&fixture.swarm.event_history),
            Some(&fixture.swarm.event_counter),
            Some(&fixture.swarm.event_tx),
        )
        .await;
    }
    fixture.done().await;
    monitor.abort();
    assert!(monitor.await.unwrap_err().is_cancelled());
    assert_eq!(
        fixture.swarm.members.read().await["busy-worker"].status,
        "running"
    );
    let agent = fixture.agent.lock().await;
    assert_eq!(
        agent
            .messages()
            .iter()
            .filter(|message| message.role == Role::User
                && message.content_preview().contains("Bus pipeline result"))
            .count(),
        1
    );
}

#[tokio::test]
async fn swarm_completion_wake_is_incremental_without_an_explicit_wait() {
    let _lock = crate::storage::lock_test_env();
    let _enabled = ScopedEnvVar::set("JCODE_SWARM_COMPLETION_WAKE", "true");
    let _internal = ScopedEnvVar::set("JCODE_WAKE_MODE", "internal");
    let mut fixture = Fixture::new().await;
    fixture.dispatch(&fixture.completion()).await;
    fixture.done().await;
    assert_eq!(
        fixture.swarm.members.read().await["busy-worker"].status,
        "running"
    );
    let agent = fixture.agent.lock().await;
    assert_eq!(
        agent
            .messages()
            .iter()
            .filter(|message| message.role == Role::User
                && message.content_preview().contains("First worker result"))
            .count(),
        1
    );
}

#[tokio::test]
async fn swarm_completion_wake_disabled_and_unrelated_owners_do_not_run() {
    let _lock = crate::storage::lock_test_env();
    let _enabled = ScopedEnvVar::set("JCODE_SWARM_COMPLETION_WAKE", "false");
    let _internal = ScopedEnvVar::set("JCODE_WAKE_MODE", "internal");
    let fixture = Fixture::new().await;
    let before = fixture.agent.lock().await.messages().len();
    fixture.dispatch(&fixture.completion()).await;
    crate::env::set_var("JCODE_SWARM_COMPLETION_WAKE", "true");
    crate::config::invalidate_config_cache();
    let mut wrong_owner = fixture.completion();
    wrong_owner.owner_session_id = "unrelated-parent".to_string();
    fixture.dispatch(&wrong_owner).await;
    let mut wrong_swarm = fixture.completion();
    wrong_swarm.swarm_id = "unrelated-swarm".to_string();
    fixture.dispatch(&wrong_swarm).await;
    let mut missing_worker = fixture.completion();
    missing_worker.worker_session_id = "missing-worker".to_string();
    fixture.dispatch(&missing_worker).await;
    let agent = fixture.agent.lock().await;
    assert_eq!(agent.messages().len(), before);
    assert!(!agent.has_soft_interrupts());
}

#[tokio::test]
async fn swarm_completion_wake_does_not_replay_consumed_or_canceled_queue_items() {
    let _lock = crate::storage::lock_test_env();
    let _enabled = ScopedEnvVar::set("JCODE_SWARM_COMPLETION_WAKE", "true");
    let _internal = ScopedEnvVar::set("JCODE_WAKE_MODE", "internal");
    let fixture = Fixture::new().await;
    let reservation = fixture.agent.clone().lock_owned().await;
    let before = reservation.messages().len();
    fixture.dispatch(&fixture.completion()).await;
    let queue = reservation.soft_interrupt_queue();
    assert_eq!(queue.lock().expect("queue").len(), 1);
    // Register the handoff's lock waiter before releasing the active turn.
    tokio::task::yield_now().await;
    queue.lock().expect("queue").clear();
    reservation.persist_soft_interrupt_snapshot();
    drop(reservation);
    let agent = fixture.agent.lock().await;
    assert_eq!(agent.messages().len(), before);
    assert!(!agent.has_soft_interrupts());
}

#[tokio::test]
async fn swarm_completion_wake_revalidates_detachment_and_feature_disable() {
    let _lock = crate::storage::lock_test_env();
    let _enabled = ScopedEnvVar::set("JCODE_SWARM_COMPLETION_WAKE", "true");
    let _internal = ScopedEnvVar::set("JCODE_WAKE_MODE", "internal");
    for detach in [false, true] {
        crate::env::set_var("JCODE_SWARM_COMPLETION_WAKE", "true");
        let mut fixture = Fixture::new().await;
        let reservation = fixture.agent.clone().lock_owned().await;
        let before = reservation.messages().len();
        fixture.dispatch(&fixture.completion()).await;
        tokio::task::yield_now().await;
        if detach {
            fixture.events.close();
        } else {
            crate::env::set_var("JCODE_SWARM_COMPLETION_WAKE", "false");
        }
        crate::config::invalidate_config_cache();
        drop(reservation);
        let agent = fixture.agent.lock().await;
        assert_eq!(agent.messages().len(), before);
    }
}

#[tokio::test]
async fn swarm_completion_wake_preserves_external_execution_ownership() {
    let _lock = crate::storage::lock_test_env();
    let _enabled = ScopedEnvVar::set("JCODE_SWARM_COMPLETION_WAKE", "true");
    let _external = ScopedEnvVar::set("JCODE_WAKE_MODE", "external");
    let mut fixture = Fixture::new().await;
    let before = fixture.agent.lock().await.messages().len();
    fixture.dispatch(&fixture.completion()).await;
    let event = fixture.events.recv().await.expect("external wake event");
    assert!(
        matches!(event, ServerEvent::WakeRequested { session_id, reason, .. }
        if session_id == fixture.session_id && reason == "swarm_member_completed")
    );
    let agent = fixture.agent.lock().await;
    assert_eq!(agent.messages().len(), before);
    assert!(!agent.has_soft_interrupts());
}

#[tokio::test]
async fn swarm_completion_wake_respects_explicit_await_wake_false() {
    let _lock = crate::storage::lock_test_env();
    let _enabled = ScopedEnvVar::set("JCODE_SWARM_COMPLETION_WAKE", "true");
    let fixture = Fixture::new().await;
    let before = fixture.agent.lock().await.messages().len();
    let event = crate::bus::SwarmAwaitCompleted {
        session_id: fixture.session_id.clone(),
        completed: true,
        summary: "Done".to_string(),
        notification: "Await result".to_string(),
        notify: false,
        wake: false,
    };
    super::super::background_tasks::dispatch_swarm_await_completion(
        &event,
        &fixture.sessions,
        &fixture.queues,
        &fixture.swarm.members,
        &fixture.swarm.swarms_by_id,
        &fixture.swarm.event_history,
        &fixture.swarm.event_counter,
        &fixture.swarm.event_tx,
    )
    .await;
    let agent = fixture.agent.lock().await;
    assert_eq!(agent.messages().len(), before);
    assert!(!agent.has_soft_interrupts());
}

#[tokio::test]
async fn swarm_completion_wake_coalesces_busy_completions_without_duplicate_messages() {
    let _lock = crate::storage::lock_test_env();
    let _enabled = ScopedEnvVar::set("JCODE_SWARM_COMPLETION_WAKE", "true");
    let _internal = ScopedEnvVar::set("JCODE_WAKE_MODE", "internal");
    let mut fixture = Fixture::new().await;
    let reservation = fixture.agent.clone().lock_owned().await;
    fixture.dispatch(&fixture.completion()).await;
    let mut second = fixture.completion();
    second.worker_session_id = "busy-worker".to_string();
    second.notification = "Second worker result".to_string();
    fixture.dispatch(&second).await;
    assert_eq!(reservation.soft_interrupt_count(), 2);
    tokio::task::yield_now().await;
    drop(reservation);
    fixture.done().await;
    let agent = fixture.agent.lock().await;
    for result in ["First worker result", "Second worker result"] {
        assert_eq!(
            agent
                .messages()
                .iter()
                .filter(|message| message.role == Role::User
                    && message.content_preview().contains(result))
                .count(),
            1
        );
    }
    assert!(!agent.has_soft_interrupts());
}

#[tokio::test]
async fn swarm_completion_wake_restores_busy_unregistered_queue_fallback() {
    let _lock = crate::storage::lock_test_env();
    let _enabled = ScopedEnvVar::set("JCODE_SWARM_COMPLETION_WAKE", "true");
    let _internal = ScopedEnvVar::set("JCODE_WAKE_MODE", "internal");
    let mut fixture = Fixture::new().await;
    let reservation = fixture.agent.clone().lock_owned().await;
    fixture.queues.write().await.remove(&fixture.session_id);
    fixture.dispatch(&fixture.completion()).await;
    assert!(!reservation.has_soft_interrupts());
    tokio::task::yield_now().await;
    drop(reservation);
    fixture.done().await;
    let agent = fixture.agent.lock().await;
    assert_eq!(
        agent
            .messages()
            .iter()
            .filter(|message| message.role == Role::User
                && message.content_preview().contains("First worker result"))
            .count(),
        1
    );
    assert!(!agent.has_soft_interrupts());
}
