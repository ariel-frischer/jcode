//! Native OpenAI HTTPS/SSE response consumption.

use super::*;

/// Stream the response from OpenAI API
pub(crate) async fn stream_response(
    client: Client,
    credentials: Arc<RwLock<CodexCredentials>>,
    request: Value,
    initial_status_detail: String,
    tx: mpsc::Sender<Result<StreamEvent>>,
) -> Result<(), OpenAIStreamFailure> {
    use jcode_message_types::ConnectionPhase;
    let request_model = openai_request_model(&request);
    let stream_started_at = Instant::now();
    log_openai_stream_lifecycle(
        jcode_base::logging::LogLevel::Info,
        "https_request_start",
        vec![
            ("model", request_model.clone()),
            ("transport", "https".to_string()),
        ],
    );
    let usage_snapshot = jcode_base::usage::get_openai_usage_sync();
    jcode_base::logging::info(&format!(
        "OpenAI limit diag: starting fresh HTTPS request usage=({})",
        usage_snapshot.diagnostic_fields()
    ));
    emit_status_detail(&tx, initial_status_detail).await;
    emit_connection_phase(&tx, ConnectionPhase::Authenticating).await;
    let access_token = openai_access_token(&credentials).await?;
    let creds = credentials.read().await;
    let is_chatgpt_mode = !creds.refresh_token.is_empty() || creds.id_token.is_some();
    let url = OpenAIProvider::responses_url(&creds);
    let account_id = creds.account_id.clone();
    drop(creds);

    let mut builder = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json");

    if is_chatgpt_mode {
        builder = builder.header("originator", ORIGINATOR);
        if let Some(account_id) = account_id.as_ref() {
            builder = builder.header("chatgpt-account-id", account_id);
        }
    }

    emit_connection_phase(&tx, ConnectionPhase::SendingRequest).await;
    let connect_start = std::time::Instant::now();
    let idle_timeout = effective_https_idle_timeout(&request);

    let response = jcode_provider_core::transport::send_with_initial_response_timeout(
        builder.json(&request),
        idle_timeout,
    )
    .await
    .context("Failed to send request to OpenAI API")
    .map_err(OpenAIStreamFailure::Other)?;

    let connect_ms = connect_start.elapsed().as_millis();
    jcode_base::logging::info(&format!(
        "HTTP connection established in {}ms (status={})",
        connect_ms,
        response.status()
    ));
    log_openai_stream_lifecycle(
        jcode_base::logging::LogLevel::Info,
        "https_connected",
        vec![
            ("model", request_model.clone()),
            ("status", response.status().as_u16().to_string()),
            ("connect_ms", connect_ms.to_string()),
        ],
    );
    if response.status().is_success() && usage_snapshot.exhausted() {
        jcode_base::logging::warn(&format!(
            "OpenAI limit diag: fresh HTTPS request accepted while local usage indicates exhausted usage=({})",
            usage_snapshot.diagnostic_fields()
        ));
    }

    if !response.status().is_success() {
        let status = response.status();
        let retry_after = jcode_provider_core::retry_after::retry_after(response.headers());

        let body = jcode_base::util::http_error_body(response, "HTTP error").await;
        log_openai_stream_lifecycle(
            jcode_base::logging::LogLevel::Warn,
            "https_http_error",
            vec![
                ("model", request_model.clone()),
                ("status", status.as_u16().to_string()),
                (
                    "retry_after_secs",
                    retry_after
                        .map(|hint| hint.remaining().as_secs().to_string())
                        .unwrap_or_else(|| "none".to_string()),
                ),
                ("body", body.clone()),
                (
                    "elapsed_ms",
                    stream_started_at.elapsed().as_millis().to_string(),
                ),
            ],
        );

        if let Some(reason) = classify_unavailable_model_error(status, &body)
            && let Some(model_name) = request.get("model").and_then(|m| m.as_str())
        {
            jcode_base::provider::record_model_unavailable_for_account(model_name, &reason);
            jcode_base::logging::warn(&format!(
                "Recorded OpenAI model '{}' as unavailable: {}",
                model_name, reason
            ));
        }

        // Check if we need to refresh token
        if should_refresh_token(status, &body) {
            // The server rejected our access token (401/403). Proactively
            // refresh it in place so the retry loop reconnects with a fresh
            // token instead of surfacing a raw "Token refresh needed" error.
            let refresh_token = {
                let creds = credentials.read().await;
                creds.refresh_token.clone()
            };

            if refresh_token.is_empty() {
                return Err(OpenAIStreamFailure::Other(anyhow::anyhow!(
                    "OpenAI rejected the access token and no refresh token is available; run /login to re-authenticate: {}",
                    body
                )));
            }

            match force_refresh_openai_token(&credentials, &refresh_token).await {
                Ok(_) => {
                    jcode_base::logging::info(
                        "OpenAI access token rejected; refreshed credentials and will retry",
                    );
                    // Surface a retryable error so the retry loop reconnects
                    // with the freshly refreshed token.
                    return Err(OpenAIStreamFailure::Other(anyhow::anyhow!(
                        "openai token refreshed, retrying: {}",
                        body
                    )));
                }
                Err(refresh_err) => {
                    return Err(OpenAIStreamFailure::Other(anyhow::anyhow!(
                        "OpenAI token refresh failed; run /login to re-authenticate: {refresh_err:#}"
                    )));
                }
            }
        }

        // For rate limits, format structured payloads into a readable message.
        let msg = if status == StatusCode::TOO_MANY_REQUESTS {
            format_rate_limit_error(&body, retry_after.map(|hint| hint.remaining()))
        } else {
            format!("OpenAI API error {}: {}", status, body)
        };
        return Err(OpenAIStreamFailure::Other(
            jcode_provider_core::retry_after::error_with_retry_after(msg, retry_after),
        ));
    }

    emit_connection_phase(&tx, ConnectionPhase::WaitingForResponse).await;

    let _ = tx
        .send(Ok(StreamEvent::ConnectionType {
            connection: "https/sse".to_string(),
        }))
        .await;

    // Stream the response
    let meaningful_progress_at = Arc::new(StdMutex::new(Instant::now()));
    let meaningful_timeout_secs = effective_openai_stall_timeout_secs(&request);
    let mut stream = if meaningful_timeout_secs.is_some() {
        OpenAIResponsesStream::new_with_progress(
            response.bytes_stream(),
            Arc::clone(&meaningful_progress_at),
        )
    } else {
        OpenAIResponsesStream::new(response.bytes_stream())
    };
    let mut saw_message_end = false;

    // Idle timeout between streamed events. Without this, a silently dead
    // connection (or a provider that never emits) would hang forever; with a
    // hardcoded short value, slow reasoning models that think silently for
    // minutes get cancelled prematurely. Resolved from
    // `[provider] stream_idle_timeout_secs` / `JCODE_STREAM_IDLE_TIMEOUT_SECS`
    // (issue #434).
    use futures::StreamExt;
    let mut last_stream_event_at = Instant::now();
    loop {
        let last_meaningful_progress_at = meaningful_progress_at
            .lock()
            .map(|timestamp| *timestamp)
            .unwrap_or_else(|poisoned| *poisoned.into_inner());
        let Some(wait_timeout) = effective_stream_wait_timeout(
            idle_timeout.saturating_sub(last_stream_event_at.elapsed()),
            last_meaningful_progress_at,
            meaningful_timeout_secs,
        ) else {
            log_openai_stream_lifecycle(
                jcode_base::logging::LogLevel::Warn,
                "https_stream_stall_timeout",
                vec![
                    ("model", request_model.clone()),
                    (
                        "meaningful_timeout_secs",
                        meaningful_timeout_secs.unwrap_or(0).to_string(),
                    ),
                    (
                        "elapsed_ms",
                        stream_started_at.elapsed().as_millis().to_string(),
                    ),
                ],
            );
            return Err(OpenAIStreamFailure::Other(anyhow::anyhow!(
                "OpenAI HTTPS stream timed out without meaningful progress for {} seconds",
                meaningful_timeout_secs.unwrap_or(0)
            )));
        };
        let result = match tokio::select! {
            biased;
            _ = tx.closed() => return Ok(()),
            result = tokio::time::timeout(wait_timeout, stream.next()) => result,
        } {
            Ok(Some(result)) => result,
            Ok(None) => break, // stream ended normally
            Err(_) => {
                // `OpenAIResponsesStream` may consume a meaningful raw SSE
                // frame (notably a buffered tool-argument delta) while still
                // waiting for a higher-level StreamEvent to return. Refresh
                // the local deadline before declaring this read timed out.
                let latest_meaningful_progress_at = meaningful_progress_at
                    .lock()
                    .map(|timestamp| *timestamp)
                    .unwrap_or_else(|poisoned| *poisoned.into_inner());
                let meaningful_stall = meaningful_timeout_secs.is_some_and(|secs| {
                    latest_meaningful_progress_at.elapsed() >= Duration::from_secs(secs)
                });
                if !meaningful_stall && last_stream_event_at.elapsed() < idle_timeout {
                    continue;
                }
                log_openai_stream_lifecycle(
                    jcode_base::logging::LogLevel::Warn,
                    if meaningful_stall {
                        "https_stream_stall_timeout"
                    } else {
                        "https_stream_idle_timeout"
                    },
                    vec![
                        ("model", request_model.clone()),
                        ("idle_timeout_secs", idle_timeout.as_secs().to_string()),
                        (
                            "meaningful_timeout_secs",
                            meaningful_timeout_secs
                                .map(|secs| secs.to_string())
                                .unwrap_or_else(|| "disabled".to_string()),
                        ),
                        (
                            "elapsed_ms",
                            stream_started_at.elapsed().as_millis().to_string(),
                        ),
                    ],
                );
                if meaningful_stall {
                    return Err(OpenAIStreamFailure::Other(anyhow::anyhow!(
                        "OpenAI HTTPS stream timed out without meaningful progress for {} seconds",
                        meaningful_timeout_secs.unwrap_or(0)
                    )));
                }
                return Err(OpenAIStreamFailure::Other(anyhow::anyhow!(
                    "Stream read timeout: no data received for {} seconds",
                    idle_timeout.as_secs()
                )));
            }
        };
        last_stream_event_at = Instant::now();
        match result {
            Ok(event) => {
                let meaningful_progress = is_meaningful_stream_progress(&event);
                if matches!(event, StreamEvent::MessageEnd { .. }) {
                    saw_message_end = true;
                }
                if let StreamEvent::Error { message, .. } = &event {
                    if let Some(model_name) = request.get("model").and_then(|m| m.as_str()) {
                        maybe_record_runtime_model_unavailable_from_stream_error(
                            model_name, message,
                        );
                    }
                    if is_retryable_error(&message.to_lowercase()) {
                        log_openai_stream_lifecycle(
                            jcode_base::logging::LogLevel::Warn,
                            "https_stream_retryable_error",
                            vec![
                                ("model", request_model.clone()),
                                ("error", message.clone()),
                                (
                                    "elapsed_ms",
                                    stream_started_at.elapsed().as_millis().to_string(),
                                ),
                            ],
                        );
                        return Err(OpenAIStreamFailure::Other(anyhow::anyhow!(
                            "Stream error: {}",
                            message
                        )));
                    }
                    return Err(OpenAIStreamFailure::Terminal(event));
                }
                if tx.send(Ok(event)).await.is_err() {
                    // Receiver dropped, stop streaming
                    log_openai_stream_lifecycle(
                        jcode_base::logging::LogLevel::Warn,
                        "consumer_dropped",
                        vec![
                            ("model", request_model.clone()),
                            ("transport", "https".to_string()),
                            (
                                "elapsed_ms",
                                stream_started_at.elapsed().as_millis().to_string(),
                            ),
                        ],
                    );
                    return Ok(());
                }
                if meaningful_progress {
                    *meaningful_progress_at
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Instant::now();
                }
            }
            Err(e) => {
                log_openai_stream_lifecycle(
                    jcode_base::logging::LogLevel::Warn,
                    "https_stream_error",
                    vec![
                        ("model", request_model.clone()),
                        ("error", e.to_string()),
                        (
                            "elapsed_ms",
                            stream_started_at.elapsed().as_millis().to_string(),
                        ),
                    ],
                );
                return Err(OpenAIStreamFailure::Other(anyhow::anyhow!(
                    "Stream error: {}",
                    e
                )));
            }
        }
    }

    if !saw_message_end {
        log_openai_stream_lifecycle(
            jcode_base::logging::LogLevel::Warn,
            "https_eof_before_message_end",
            vec![
                ("model", request_model.clone()),
                (
                    "elapsed_ms",
                    stream_started_at.elapsed().as_millis().to_string(),
                ),
            ],
        );
        return Err(OpenAIStreamFailure::Other(anyhow::anyhow!(
            "OpenAI HTTPS stream ended before message completion marker"
        )));
    }

    log_openai_stream_lifecycle(
        jcode_base::logging::LogLevel::Info,
        "https_stream_complete",
        vec![
            ("model", request_model),
            (
                "elapsed_ms",
                stream_started_at.elapsed().as_millis().to_string(),
            ),
        ],
    );
    Ok(())
}
