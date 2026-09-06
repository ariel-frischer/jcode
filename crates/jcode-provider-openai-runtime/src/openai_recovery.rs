use super::*;

#[derive(Debug)]
pub(crate) enum OpenAIStreamFailure {
    FallbackToHttps(anyhow::Error),
    Other(anyhow::Error),
    Terminal(StreamEvent),
}

impl From<anyhow::Error> for OpenAIStreamFailure {
    fn from(err: anyhow::Error) -> Self {
        Self::Other(err)
    }
}

pub(crate) enum ContinuationDisposition {
    Finished,
    Fresh { recovering: bool },
}

#[expect(
    clippy::too_many_arguments,
    reason = "keeps continuation disposition beside its transport and saved-history safety inputs"
)]
pub(crate) async fn handle_persistent_ws_result(
    continuation_result: PersistentWsResult,
    saw_output: bool,
    saved_result_ids: &HashSet<String>,
    input: &[Value],
    persistent_ws: &Arc<Mutex<Option<PersistentWsState>>>,
    tx: &mpsc::Sender<Result<StreamEvent>>,
    model_for_transport: &str,
    websocket_cooldowns: &Arc<RwLock<HashMap<String, Instant>>>,
    websocket_failure_streaks: &Arc<RwLock<HashMap<String, u32>>>,
) -> ContinuationDisposition {
    let mut recovering_missing_tool_output = false;
    match continuation_result {
        PersistentWsResult::MissingToolOutput { call_id, mut event } => {
            *persistent_ws.lock().await = None;
            let blocked_reason = if saw_output {
                Some("response output was already emitted")
            } else if !saved_result_ids.contains(&call_id)
                || !has_replayable_tool_pair(input, &call_id)
            {
                Some("a matching saved tool call and result are unavailable in replayable history")
            } else {
                None
            };
            if let Some(reason) = blocked_reason {
                if let StreamEvent::Error { message, .. } = &mut event {
                    message.push_str(&format!(
                        " Automatic recovery stopped because {reason}. No tools were rerun. Inspect the saved conversation before resuming."
                    ));
                }
                if tx.send(Ok(event)).await.is_err() {
                    jcode_base::logging::info(
                        "OpenAI recovery consumer disconnected before terminal error delivery",
                    );
                }
                return ContinuationDisposition::Finished;
            }
            // The original full request is immutable and has no
            // previous_response_id. Replay saved results, never
            // execute tools or reconstruct their outputs here.
            recovering_missing_tool_output = true;
            emit_status_detail(tx, "recovering websocket response chain").await;
            log_openai_stream_lifecycle(
                jcode_base::logging::LogLevel::Warn,
                "persistent_state_reset",
                vec![
                    ("model", model_for_transport.to_string()),
                    ("reason", "missing_tool_output_recovery".to_string()),
                ],
            );
        }
        PersistentWsResult::Terminal(event) => {
            *persistent_ws.lock().await = None;
            if tx.send(Ok(event)).await.is_err() {
                jcode_base::logging::info(
                    "OpenAI consumer disconnected before terminal error delivery",
                );
            }
            return ContinuationDisposition::Finished;
        }
        PersistentWsResult::Success => {
            log_openai_stream_lifecycle(
                jcode_base::logging::LogLevel::Info,
                "persistent_reuse_success",
                vec![
                    ("model", model_for_transport.to_string()),
                    ("transport", "websocket".to_string()),
                ],
            );
            record_websocket_success(
                websocket_cooldowns,
                websocket_failure_streaks,
                model_for_transport,
            )
            .await;
            return ContinuationDisposition::Finished;
        }
        PersistentWsResult::NotAvailable => {
            log_openai_stream_lifecycle(
                jcode_base::logging::LogLevel::Info,
                "persistent_reuse_unavailable",
                vec![
                    ("model", model_for_transport.to_string()),
                    ("transport", "websocket".to_string()),
                ],
            );
            jcode_base::logging::info(
                "No persistent WS connection available; using fresh connection",
            );
        }
        PersistentWsResult::Failed(err) => {
            log_openai_stream_lifecycle(
                jcode_base::logging::LogLevel::Warn,
                "persistent_reuse_failed",
                vec![
                    ("model", model_for_transport.to_string()),
                    ("transport", "websocket".to_string()),
                    ("error", err.clone()),
                ],
            );
            jcode_base::logging::warn(&format!(
                "Persistent WS continuation failed: {}; using fresh connection",
                err
            ));
            if saw_output {
                // The failed continuation already streamed
                // partial output; the fresh connection below
                // replays the response from the top, so roll
                // the partial output back on the consumer.
                if tx
                    .send(Ok(StreamEvent::RetryRollback {
                        attempt: 1,
                        max: MAX_RETRIES,
                    }))
                    .await
                    .is_err()
                {
                    jcode_base::logging::info(
                        "OpenAI consumer disconnected before retry rollback delivery",
                    );
                }
            }
            let mut guard = persistent_ws.lock().await;
            *guard = None;
            log_openai_stream_lifecycle(
                jcode_base::logging::LogLevel::Warn,
                "persistent_state_reset",
                vec![
                    ("model", model_for_transport.to_string()),
                    ("reason", "persistent_reuse_failed".to_string()),
                ],
            );
        }
    }
    ContinuationDisposition::Fresh {
        recovering: recovering_missing_tool_output,
    }
}

/// Recognize only this specific Responses API rejection, not invalid requests
/// generally. Keep it separate from transport retryability.
pub(crate) fn missing_tool_output_call_id(message: &str) -> Option<&str> {
    let (prefix, suffix) = message.split_once("No tool output found for function call ")?;
    if !matches!(prefix, "" | "invalid_request_error: ") {
        return None;
    }
    let call_id = suffix.strip_suffix('.').unwrap_or(suffix);
    (!call_id.is_empty()
        && call_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')))
    .then_some(call_id)
}

/// The request builder can inject missing-result placeholders. Only real saved
/// call/result pairs may authorize recovery, never those generated placeholders.
pub(crate) fn saved_tool_result_ids(messages: &[ChatMessage]) -> HashSet<String> {
    use jcode_message_types::{ContentBlock, Role, sanitize_tool_id};
    let calls: HashSet<&str> = messages
        .iter()
        .filter(|message| message.role == Role::Assistant)
        .flat_map(|message| &message.content)
        .filter_map(|block| match block {
            ContentBlock::ToolUse { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    messages
        .iter()
        .filter(|message| message.role == Role::User)
        .flat_map(|message| &message.content)
        .filter_map(|block| match block {
            ContentBlock::ToolResult { tool_use_id, .. }
                if calls.contains(tool_use_id.as_str()) =>
            {
                Some(sanitize_tool_id(tool_use_id))
            }
            _ => None,
        })
        .collect()
}

pub(crate) fn has_replayable_tool_pair(input: &[Value], call_id: &str) -> bool {
    let mut calls = 0;
    let mut outputs = 0;
    for item in input {
        if item.get("call_id").and_then(Value::as_str) != Some(call_id) {
            continue;
        }
        match item.get("type").and_then(Value::as_str) {
            Some("function_call") => calls += 1,
            Some("function_call_output") if item.get("output").is_some_and(Value::is_string) => {
                outputs += 1;
            }
            _ => {}
        }
    }
    calls == 1 && outputs == 1
}

pub(crate) async fn finish_failed_recovery(
    failure: OpenAIStreamFailure,
    tx: &mpsc::Sender<Result<StreamEvent>>,
) {
    const ADVICE: &str = "Automatic recovery stopped after one fresh request. No tools were rerun. Inspect the saved conversation before resuming.";
    let event = match failure {
        OpenAIStreamFailure::Terminal(mut event) => {
            if let StreamEvent::Error { message, .. } = &mut event {
                *message = format!("OpenAI full-history recovery failed: {message}. {ADVICE}");
            }
            Ok(event)
        }
        OpenAIStreamFailure::Other(error) | OpenAIStreamFailure::FallbackToHttps(error) => Err(
            anyhow::anyhow!("OpenAI full-history recovery failed: {error:#}. {ADVICE}"),
        ),
    };
    if tx.send(event).await.is_err() {
        jcode_base::logging::info("OpenAI recovery consumer disconnected before failure delivery");
    }
}
