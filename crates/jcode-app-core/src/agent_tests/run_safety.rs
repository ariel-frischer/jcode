use super::*;
use crate::tool::{Tool, ToolContext};
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn agent_safety_state_is_opt_in_and_non_persisted() {
    let provider: Arc<dyn Provider> = Arc::new(DelayedProvider {
        open_delay: Duration::ZERO,
        first_event_delay: Duration::ZERO,
    });
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);
    assert!(agent.run_safety_stop_reason().is_none());

    let policy = crate::agent::run_safety::EffectiveRunSafetyPolicy {
        max_turns: std::num::NonZeroU64::new(1),
        max_tool_steps: None,
        token_budget: None,
        deadline: None,
        sources: Default::default(),
        usage_baseline: Default::default(),
    };
    agent.install_run_safety(crate::agent::run_safety::RunSafetyController::new(policy));
    assert!(agent.run_safety_controller().is_some());
    agent.run_safety_complete_turn();
    assert_eq!(
        agent.run_safety_stop_reason(),
        Some(crate::agent::run_safety::RunStopReason::MaxTurnsExceeded)
    );
}

#[tokio::test]
async fn capture_and_streaming_paths_stop_before_next_turn_when_bound_is_reached() {
    let provider: Arc<dyn Provider> = Arc::new(DelayedProvider {
        open_delay: Duration::ZERO,
        first_event_delay: Duration::ZERO,
    });
    let registry = Registry::new(provider.clone()).await;
    let mut capture_agent = Agent::new(provider.clone(), registry.clone());
    let policy = crate::agent::run_safety::EffectiveRunSafetyPolicy {
        max_turns: std::num::NonZeroU64::new(1),
        max_tool_steps: None,
        token_budget: None,
        deadline: None,
        sources: Default::default(),
        usage_baseline: Default::default(),
    };
    capture_agent.install_run_safety(crate::agent::run_safety::RunSafetyController::new(
        policy.clone(),
    ));
    capture_agent.run_safety_complete_turn();
    assert_eq!(
        capture_agent
            .run_once_capture("should not call provider")
            .await
            .unwrap(),
        ""
    );

    let mut streaming_agent = Agent::new(provider, registry);
    streaming_agent.install_run_safety(crate::agent::run_safety::RunSafetyController::new(policy));
    streaming_agent.run_safety_complete_turn();
    let (tx, mut rx) = tokio_mpsc::unbounded_channel();
    streaming_agent
        .run_once_streaming_mpsc("should not call provider", Vec::new(), None, tx)
        .await
        .unwrap();
    assert!(rx.try_recv().is_err());
}

struct CountingTool {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for CountingTool {
    fn name(&self) -> &str {
        "counting_tool"
    }

    fn description(&self) -> &str {
        "Counts test executions."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "intent": {"type": "string"}
            },
            "required": ["intent"]
        })
    }

    async fn execute(&self, _input: serde_json::Value, _ctx: ToolContext) -> Result<ToolOutput> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ToolOutput::new("counted"))
    }
}

struct TwoToolCallProvider {
    requests: Arc<AtomicUsize>,
}

#[async_trait]
impl Provider for TwoToolCallProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = tokio_mpsc::channel::<Result<StreamEvent>>(16);
        tokio::spawn(async move {
            for (id, intent) in [("call-1", "first"), ("call-2", "second")] {
                let _ = tx
                    .send(Ok(StreamEvent::ToolUseStart {
                        id: id.to_string(),
                        name: "counting_tool".to_string(),
                    }))
                    .await;
                let _ = tx
                    .send(Ok(StreamEvent::ToolInputDelta(format!(
                        "{{\"intent\":\"{intent}\"}}"
                    ))))
                    .await;
                let _ = tx.send(Ok(StreamEvent::ToolUseEnd)).await;
            }
            let _ = tx
                .send(Ok(StreamEvent::MessageEnd {
                    stop_reason: Some("tool_use".to_string()),
                }))
                .await;
        });
        Ok(Box::pin(ReceiverStream::new(rx)))
    }

    fn name(&self) -> &str {
        "two-tool-call"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self {
            requests: self.requests.clone(),
        })
    }
}

struct UsageAndToolProvider;

#[async_trait]
impl Provider for UsageAndToolProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        let (tx, rx) = tokio_mpsc::channel::<Result<StreamEvent>>(16);
        tokio::spawn(async move {
            let _ = tx
                .send(Ok(StreamEvent::TokenUsage {
                    input_tokens: Some(10),
                    output_tokens: Some(0),
                    cache_read_input_tokens: None,
                    cache_creation_input_tokens: None,
                    reported_cost_usd: None,
                }))
                .await;
            let _ = tx
                .send(Ok(StreamEvent::ToolUseStart {
                    id: "call-usage".to_string(),
                    name: "counting_tool".to_string(),
                }))
                .await;
            let _ = tx
                .send(Ok(StreamEvent::ToolInputDelta(
                    r#"{"intent":"after-budget"}"#.to_string(),
                )))
                .await;
            let _ = tx.send(Ok(StreamEvent::ToolUseEnd)).await;
            let _ = tx
                .send(Ok(StreamEvent::MessageEnd {
                    stop_reason: Some("tool_use".to_string()),
                }))
                .await;
        });
        Ok(Box::pin(ReceiverStream::new(rx)))
    }

    fn name(&self) -> &str {
        "usage-and-tool"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self)
    }
}

fn safety_test_policy(
    max_tool_steps: Option<std::num::NonZeroU64>,
    token_budget: Option<std::num::NonZeroU64>,
) -> crate::agent::run_safety::EffectiveRunSafetyPolicy {
    crate::agent::run_safety::EffectiveRunSafetyPolicy {
        max_turns: None,
        max_tool_steps,
        token_budget,
        deadline: None,
        sources: Default::default(),
        usage_baseline: Default::default(),
    }
}

fn agent_has_tool_result(agent: &Agent, tool_use_id: &str) -> bool {
    agent.session.messages.iter().any(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::ToolResult { tool_use_id: id, .. } if id == tool_use_id
            )
        })
    })
}

#[tokio::test]
async fn capture_tool_step_bound_stops_before_next_provider_request_and_repairs_history() {
    let executions = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(AtomicUsize::new(0));
    let provider: Arc<dyn Provider> = Arc::new(TwoToolCallProvider {
        requests: requests.clone(),
    });
    let registry = Registry::new(provider.clone()).await;
    registry
        .register(
            "counting_tool".to_string(),
            Arc::new(CountingTool {
                calls: executions.clone(),
            }),
        )
        .await;
    let mut agent = Agent::new(provider, registry);
    agent.install_run_safety(crate::agent::run_safety::RunSafetyController::new(
        safety_test_policy(std::num::NonZeroU64::new(1), None),
    ));

    agent
        .run_once_capture("execute the tools")
        .await
        .expect("bounded capture run should complete");

    assert_eq!(executions.load(Ordering::SeqCst), 1);
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    assert_eq!(
        agent.run_safety_stop_reason(),
        Some(crate::agent::run_safety::RunStopReason::MaxToolStepsExceeded)
    );
    assert!(agent_has_tool_result(&agent, "call-2"));
}

#[tokio::test]
async fn capture_token_budget_repairs_history_after_provider_tool_calls() {
    let provider: Arc<dyn Provider> = Arc::new(UsageAndToolProvider);
    let registry = Registry::new(provider.clone()).await;
    registry
        .register(
            "counting_tool".to_string(),
            Arc::new(CountingTool {
                calls: Arc::new(AtomicUsize::new(0)),
            }),
        )
        .await;
    let mut agent = Agent::new(provider, registry);
    agent.install_run_safety(crate::agent::run_safety::RunSafetyController::new(
        safety_test_policy(None, std::num::NonZeroU64::new(10)),
    ));

    agent
        .run_once_capture("spend the budget")
        .await
        .expect("bounded capture run should complete");

    assert_eq!(
        agent.run_safety_stop_reason(),
        Some(crate::agent::run_safety::RunStopReason::TokenBudgetExceeded)
    );
    assert!(agent_has_tool_result(&agent, "call-usage"));
}

#[tokio::test]
async fn streaming_token_budget_stops_before_local_tool_execution() {
    let executions = Arc::new(AtomicUsize::new(0));
    let provider: Arc<dyn Provider> = Arc::new(UsageAndToolProvider);
    let registry = Registry::new(provider.clone()).await;
    registry
        .register(
            "counting_tool".to_string(),
            Arc::new(CountingTool {
                calls: executions.clone(),
            }),
        )
        .await;
    let mut agent = Agent::new(provider, registry);
    agent.install_run_safety(crate::agent::run_safety::RunSafetyController::new(
        safety_test_policy(None, std::num::NonZeroU64::new(10)),
    ));
    let (tx, mut rx) = tokio_mpsc::unbounded_channel();

    agent
        .run_once_streaming_mpsc("spend the budget", Vec::new(), None, tx)
        .await
        .expect("bounded streaming run should complete");
    while rx.try_recv().is_ok() {}

    assert_eq!(executions.load(Ordering::SeqCst), 0);
    assert_eq!(
        agent.run_safety_stop_reason(),
        Some(crate::agent::run_safety::RunStopReason::TokenBudgetExceeded)
    );
}
