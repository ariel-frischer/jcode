use super::*;

/// Test that token usage is tracked
#[tokio::test]
async fn test_token_usage() -> Result<()> {
    let _env = setup_test_env()?;
    let provider = MockProvider::new();

    provider.queue_response(vec![
        StreamEvent::TokenUsage {
            input_tokens: Some(10),
            output_tokens: Some(20),
            cache_read_input_tokens: None,
            cache_creation_input_tokens: None,
            reported_cost_usd: None,
        },
        StreamEvent::TextDelta("Response".to_string()),
        StreamEvent::MessageEnd {
            stop_reason: Some("end_turn".to_string()),
        },
        StreamEvent::SessionId("session-123".to_string()),
    ]);

    let provider: Arc<dyn jcode::provider::Provider> = Arc::new(provider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);

    let response = agent.run_once_capture("Test").await?;
    assert_eq!(response, "Response");

    Ok(())
}
