//! Adapter for the existing generic stream contract. Exposed retry segments are
//! not proof of physical transport-attempt completeness or resolved auth/model.
use super::{Attempt, AuthClass, MemoryOperationKind, RequestOutcome, Sidecar};
use crate::message::{ContentBlock, Message, Role, StreamEvent};
use anyhow::{Context, Result};
use futures::StreamExt;

pub(super) async fn complete(sidecar: &Sidecar, system: &str, prompt: &str) -> Result<String> {
    let provider = sidecar.provider.as_ref().context(
        "No active provider registered for sidecar; memory features require a logged-in provider",
    )?;
    let bound = if sidecar.memory_context.is_none() {
        sidecar
            .clone()
            .with_memory_operation(None, MemoryOperationKind::Unattributed)
    } else {
        sidecar.clone()
    };
    let model = provider.model();
    let new_attempt = || {
        let mut attempt = Attempt::new(&bound, provider.name(), &model, None, AuthClass::Unknown);
        attempt.provider_call_only();
        attempt
    };
    // Identical messages/options to Provider::complete_simple. No new inference.
    let messages = vec![Message {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: prompt.to_string(),
            cache_control: None,
        }],
        timestamp: None,
        tool_duration_ms: None,
    }];
    let mut attempt = Some(new_attempt());
    let result: Result<String> = async {
        let mut stream = provider.complete(&messages, &[], system, None).await?;
        let mut text = String::new();
        while let Some(event) = stream.next().await {
            match event? {
                StreamEvent::RetryRollback { .. } => {
                    if let Some(mut previous) = attempt.take() {
                        previous.finish(true);
                    }
                    // A retry announcement is not a sent request. Only subsequent
                    // content/usage starts another explicitly partial segment.
                }
                StreamEvent::TokenUsage {
                    input_tokens,
                    output_tokens,
                    cache_read_input_tokens,
                    cache_creation_input_tokens,
                    ..
                } => {
                    let current = attempt.get_or_insert_with(new_attempt);
                    current.usage.generic(
                        input_tokens,
                        output_tokens,
                        cache_read_input_tokens,
                        cache_creation_input_tokens,
                        provider.name(),
                    );
                }
                StreamEvent::TextDelta(delta) => {
                    attempt.get_or_insert_with(new_attempt);
                    // Preserve complete_simple's text semantics across retries.
                    text.push_str(&delta);
                }
                StreamEvent::MessageEnd { .. } => {
                    let current = attempt.get_or_insert_with(new_attempt);
                    current.usage.outcome = RequestOutcome::Success;
                }
                _ => {}
            }
        }
        Ok(text)
    }
    .await;
    if let Some(current) = attempt.as_mut() {
        current.finish(result.is_err());
    }
    result.context("Sidecar completion via active provider failed")
}
