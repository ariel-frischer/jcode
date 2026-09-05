use super::*;
use crate::message::{ContentBlock, Message, Role, StreamEvent, ToolDefinition};
use crate::provider::{EventStream, Provider};
use jcode_session_types::memory_usage::AttemptCoverage;

#[derive(Clone)]
struct FixtureProvider;

#[async_trait::async_trait]
impl Provider for FixtureProvider {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        system: &str,
        resume: Option<&str>,
    ) -> Result<EventStream> {
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, Role::User);
        assert!(
            matches!(&messages[0].content[0], ContentBlock::Text { text, cache_control: None } if text == "private-input")
        );
        assert!(tools.is_empty());
        assert_eq!(system, "private-system");
        assert!(resume.is_none());
        let usage = || StreamEvent::TokenUsage {
            input_tokens: Some(10),
            output_tokens: Some(2),
            cache_read_input_tokens: Some(3),
            cache_creation_input_tokens: None,
            reported_cost_usd: None,
        };
        Ok(Box::pin(futures::stream::iter(vec![
            Ok(usage()),
            Ok(StreamEvent::RetryRollback { attempt: 1, max: 2 }),
            Ok(usage()),
            Ok(usage()),
            Ok(StreamEvent::TextDelta("answer".into())),
            Ok(StreamEvent::MessageEnd { stop_reason: None }),
        ])))
    }
    fn name(&self) -> &str {
        "fixture"
    }
    fn model(&self) -> String {
        "fixture-model".into()
    }
    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(self.clone())
    }
}

#[tokio::test]
async fn generic_usage_retries_are_segments_not_claimed_physical_attempts() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let mut sidecar = Sidecar::with_openai_model("unused", None).with_observation_sender(tx);
    sidecar.backend = SidecarBackend::Provider;
    sidecar.provider = Some(Arc::new(FixtureProvider));
    assert_eq!(
        sidecar
            .complete("private-system", "private-input")
            .await
            .unwrap(),
        "answer"
    );
    let first = rx.try_recv().expect("exposed interrupted segment");
    let second = rx.try_recv().expect("exposed resumed segment");
    assert_ne!(first.request_id, second.request_id);
    assert_eq!(first.context.operation_id, second.context.operation_id);
    assert_eq!(first.outcome, RequestOutcome::Error);
    assert_eq!(second.outcome, RequestOutcome::Success);
    for record in [first, second] {
        assert_eq!(record.attempt_coverage, AttemptCoverage::ProviderCallOnly);
        assert_eq!(record.provider, "fixture");
        assert_eq!(record.model, "fixture-model");
        assert_eq!(record.usage.input_tokens, Some(10));
        assert_eq!(record.usage.output_tokens, Some(2));
        assert_eq!(record.usage.reasoning_tokens, None);
        assert_eq!(record.auth_class, AuthClass::Unknown);
        record.validate().unwrap();
    }
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn claude_http_error_and_success_preserve_usage_and_auth() {
    use super::attempt_tests::fixture;
    for (status, auth) in [
        ("403 Forbidden", AuthClass::Oauth),
        ("200 OK", AuthClass::ApiKey),
    ] {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let sidecar =
            Sidecar::with_claude_model("claude-haiku-4-5-20251001").with_observation_sender(tx);
        let body = r#"{"content":[{"type":"text","text":"PRIVATE_TEXT"}],"stop_reason":"end_turn","usage":{"input_tokens":10,"cache_read_input_tokens":3,"cache_creation_input_tokens":2,"output_tokens":4}}"#;
        let (url, server) = fixture(status, body, false).await;
        let builder = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .post(url)
            .json(&serde_json::json!({}));
        let result = sidecar.send_claude_request(builder, auth).await;
        assert_eq!(result.is_ok(), status == "200 OK");
        server.await.unwrap();
        let record = rx.try_recv().expect("Claude actual send observation");
        assert_eq!(record.auth_class, auth);
        assert_eq!(record.usage.input_tokens, Some(15));
        assert_eq!(record.usage.total_tokens().unwrap(), Some(19));
        assert_eq!(
            record.outcome,
            if status == "200 OK" {
                RequestOutcome::Success
            } else {
                RequestOutcome::Error
            }
        );
        assert!(!serde_json::to_string(&record).unwrap().contains("PRIVATE_"));
    }
}
