use crate::session_librarian::{
    LibrarianCompletion, LibrarianInvocation, LibrarianResult, SessionLibrarian,
};
use crate::tool::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use jcode_base::config::LibrarianInvocationOverrides;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

#[derive(Debug, Default, Deserialize)]
struct Input {
    session_id: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    reasoning_effort: Option<String>,
    max_input_tokens: Option<String>,
    max_output_tokens: Option<String>,
    max_requests: Option<String>,
    max_cost_usd: Option<String>,
    deadline_seconds: Option<String>,
}

impl Input {
    fn overrides(self) -> LibrarianInvocationOverrides {
        LibrarianInvocationOverrides {
            provider: self.provider,
            model: self.model,
            reasoning_effort: self.reasoning_effort,
            max_input_tokens: self.max_input_tokens,
            max_output_tokens: self.max_output_tokens,
            max_requests: self.max_requests,
            max_cost_usd: self.max_cost_usd,
            deadline_seconds: self.deadline_seconds,
        }
    }
}

pub struct SessionLibrarianTool {
    librarian: Arc<dyn SessionLibrarian>,
    in_flight: Mutex<HashSet<String>>,
}

impl SessionLibrarianTool {
    pub fn new(librarian: Arc<dyn SessionLibrarian>) -> Self {
        Self {
            librarian,
            in_flight: Mutex::new(HashSet::new()),
        }
    }

    fn acquire(&self, session_id: &str) -> Option<InvocationGuard<'_>> {
        let mut in_flight = self
            .in_flight
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !in_flight.insert(session_id.to_string()) {
            return None;
        }
        Some(InvocationGuard {
            in_flight: &self.in_flight,
            session_id: session_id.to_string(),
        })
    }
}

struct InvocationGuard<'a> {
    in_flight: &'a Mutex<HashSet<String>>,
    session_id: String,
}

impl Drop for InvocationGuard<'_> {
    fn drop(&mut self) {
        self.in_flight
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.session_id);
    }
}

#[async_trait]
impl Tool for SessionLibrarianTool {
    fn name(&self) -> &str {
        "session_librarian"
    }

    fn description(&self) -> &str {
        "Generate or reuse one bounded summary for the current session or one explicit persisted session."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "intent": super::intent_schema_property(),
                "session_id": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 256,
                    "description": "Optional persisted session identifier. Defaults to the current ToolContext session."
                },
                "provider": bounded_string("Optional librarian provider override.", 128),
                "model": bounded_string("Optional librarian model override.", 256),
                "reasoning_effort": bounded_string("Optional librarian reasoning-effort override.", 32),
                "max_input_tokens": bounded_string("Optional positive input-token budget override.", 16),
                "max_output_tokens": bounded_string("Optional positive output-token budget override.", 16),
                "max_requests": bounded_string("Optional request budget override. The supported value is 1.", 8),
                "max_cost_usd": bounded_string("Optional positive decimal USD exposure override.", 32),
                "deadline_seconds": bounded_string("Optional positive deadline-seconds override.", 16)
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let input: Input = serde_json::from_value(input)?;
        let requested_session = match input.session_id.as_deref() {
            Some(session_id) if session_id.trim().is_empty() => {
                return Ok(ToolOutput::new(
                    "session_librarian failed [invalid_session_id]: session_id must be non-empty.",
                ));
            }
            Some(session_id) => session_id.trim().to_string(),
            None => ctx.session_id,
        };

        let Some(_guard) = self.acquire(&requested_session) else {
            return Ok(ToolOutput::new(format!(
                "session_librarian skipped: an attempt is already in progress for session {requested_session}."
            )));
        };

        // The tool surface has the authoritative server session identifier, not
        // ownership of the live Session value. Resolution therefore remains in
        // the librarian workflow for both the current-default and explicit forms.
        let invocation = LibrarianInvocation::persisted(&requested_session, input.overrides());

        Ok(render_result(self.librarian.invoke(invocation).await))
    }
}

fn bounded_string(description: &'static str, max_length: usize) -> Value {
    json!({
        "type": "string",
        "maxLength": max_length,
        "description": description
    })
}

fn render_result(result: LibrarianResult) -> ToolOutput {
    match result {
        LibrarianResult::Reused(completion) => render_completion("reused", completion),
        LibrarianResult::Succeeded(completion) => render_completion("generated", completion),
        LibrarianResult::Failed(failure) => {
            let usage = failure
                .usage
                .as_ref()
                .map(format_usage)
                .unwrap_or_else(|| "usage unavailable".to_string());
            ToolOutput::new(format!(
                "session_librarian failed at {:?} [{}]: {} ({usage})",
                failure.stage, failure.code, failure.message
            ))
        }
    }
}

fn render_completion(status: &str, completion: LibrarianCompletion) -> ToolOutput {
    let route = &completion.source_fingerprint.configuration_identity.route;
    ToolOutput::new(format!(
        "session_librarian {status} summary for session {}\nartifact_directory: {}\nfingerprint: {}:{}\nroute: {}/{}/{} ({})\nusage: {}",
        completion.session_id,
        completion.artifacts.directory().display(),
        completion.source_fingerprint.algorithm_version,
        completion.source_fingerprint.digest,
        route.provider,
        route.api_method,
        route.model,
        route.reasoning_effort,
        format_usage(&completion.usage),
    ))
}

fn format_usage(usage: &jcode_session_types::BoundedUsage) -> String {
    format!(
        "input_tokens={}, output_tokens={}, requests={}, elapsed_ms={}, cost_micros_usd={}",
        usage.input_tokens,
        usage.output_tokens,
        usage.request_count,
        usage.elapsed_ms,
        usage.cost_micros_usd
    )
}

#[cfg(test)]
mod tests {
    use super::SessionLibrarianTool;
    use crate::session_librarian::{
        LibrarianArtifactPaths, LibrarianCompletion, LibrarianFailure, LibrarianFailureStage,
        LibrarianInvocation, LibrarianResult, SessionLibrarian,
    };
    use crate::tool::{Tool, ToolContext, ToolExecutionMode};
    use async_trait::async_trait;
    use jcode_session_types::{
        BoundedUsage, LibrarianBudgetIdentity, LibrarianConfigurationIdentity, RouteIdentity,
        SourceFingerprint,
    };
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    use std::time::Duration;
    use tokio::sync::Notify;

    const SECRET_SENTINEL: &str = "credential-sentinel-must-not-appear";

    #[derive(Default)]
    struct FakeLibrarian {
        attempts: Mutex<Vec<String>>,
        provider_requests: AtomicUsize,
        artifact_writes: AtomicUsize,
        blocked_started: Notify,
        release_blocked: Notify,
    }

    impl FakeLibrarian {
        fn attempted_sessions(&self) -> Vec<String> {
            self.attempts
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        }

        fn provider_requests(&self) -> usize {
            self.provider_requests.load(Ordering::SeqCst)
        }

        fn artifact_writes(&self) -> usize {
            self.artifact_writes.load(Ordering::SeqCst)
        }

        fn successful_result(&self, session_id: &str) -> LibrarianResult {
            self.provider_requests.fetch_add(1, Ordering::SeqCst);
            self.artifact_writes.fetch_add(2, Ordering::SeqCst);
            LibrarianResult::Succeeded(completion(session_id))
        }
    }

    #[async_trait]
    impl SessionLibrarian for FakeLibrarian {
        async fn invoke(&self, invocation: LibrarianInvocation<'_>) -> LibrarianResult {
            let session_id = invocation.target.requested_session_id().to_string();
            self.attempts
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(session_id.clone());

            match session_id.as_str() {
                "missing-session" => LibrarianResult::Failed(LibrarianFailure {
                    stage: LibrarianFailureStage::Resolution,
                    code: "source_session_not_found",
                    message: "No persisted session exists for the requested identifier.".into(),
                    usage: None,
                }),
                "orchestration-failure" => LibrarianResult::Failed(LibrarianFailure {
                    stage: LibrarianFailureStage::Generation,
                    code: "librarian_generation_failed",
                    message: "The librarian could not generate a summary; credential details were redacted."
                        .into(),
                    usage: None,
                }),
                "blocked-session" => {
                    self.blocked_started.notify_waiters();
                    self.release_blocked.notified().await;
                    self.successful_result(&session_id)
                }
                _ => self.successful_result(&session_id),
            }
        }
    }

    fn tool_context(session_id: &str) -> ToolContext {
        ToolContext {
            session_id: session_id.to_string(),
            message_id: "message-1".into(),
            tool_call_id: "tool-call-1".into(),
            working_dir: None,
            stdin_request_tx: None,
            graceful_shutdown_signal: None,
            execution_mode: ToolExecutionMode::Direct,
        }
    }

    fn completion(session_id: &str) -> LibrarianCompletion {
        LibrarianCompletion {
            session_id: session_id.to_string(),
            source_fingerprint: SourceFingerprint {
                algorithm_version: "session-librarian-fingerprint.v1".into(),
                digest: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
                configuration_identity: LibrarianConfigurationIdentity {
                    budgets: LibrarianBudgetIdentity {
                        deadline_seconds: 120,
                        max_cost_micros_usd: 500_000,
                        max_input_tokens: 12_000,
                        max_output_tokens: 2_500,
                        max_requests: 1,
                    },
                    filter_version: "session-librarian-filter.v1".into(),
                    prompt_version: "session-librarian-prompt.v1".into(),
                    receipt_version: "session-librarian-receipt.v1".into(),
                    route: RouteIdentity {
                        provider: "openai".into(),
                        api_method: "openai-oauth".into(),
                        model: "gpt-5.6-luna".into(),
                        reasoning_effort: "xhigh".into(),
                    },
                    schema_version: "session-summary.v1".into(),
                },
            },
            artifacts: LibrarianArtifactPaths::new(PathBuf::from(format!(
                "/feedback/sessions/{session_id}/fingerprint"
            ))),
            usage: BoundedUsage {
                input_tokens: 100,
                output_tokens: 20,
                request_count: 1,
                elapsed_ms: 50,
                cost_micros_usd: 10_000,
            },
        }
    }

    #[test]
    fn registration_is_inert_until_the_tool_is_explicitly_executed() {
        let librarian = Arc::new(FakeLibrarian::default());

        let _tool = SessionLibrarianTool::new(librarian.clone());

        assert!(librarian.attempted_sessions().is_empty());
        assert_eq!(librarian.provider_requests(), 0);
        assert_eq!(librarian.artifact_writes(), 0);
    }

    #[test]
    fn schema_is_manual_and_limits_inputs_to_target_and_librarian_overrides() {
        let tool = SessionLibrarianTool::new(Arc::new(FakeLibrarian::default()));
        let schema = tool.parameters_schema();
        let properties = schema["properties"].as_object().expect("object properties");

        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], json!(null));
        assert_eq!(properties["session_id"]["maxLength"], 256);
        assert_eq!(properties["max_requests"]["maxLength"], 8);
        assert_eq!(properties.len(), 10);
    }

    #[tokio::test]
    async fn omitted_target_uses_the_tool_context_session_once() {
        let librarian = Arc::new(FakeLibrarian::default());
        let tool = SessionLibrarianTool::new(librarian.clone());

        let output = tool
            .execute(
                json!({"intent": "Summarize the current session."}),
                tool_context("active-session"),
            )
            .await
            .expect("a valid current-session invocation should return a tool result");

        assert_eq!(librarian.attempted_sessions(), ["active-session"]);
        assert_eq!(librarian.provider_requests(), 1);
        assert_eq!(librarian.artifact_writes(), 2);
        assert!(output.output.contains("active-session"));
        assert!(output.output.contains("generated"));
        assert!(output.output.contains("artifact_directory:"));
        assert!(output.output.contains("session-librarian-fingerprint.v1"));
        assert!(
            output
                .output
                .contains("openai/openai-oauth/gpt-5.6-luna (xhigh)")
        );
        assert!(output.output.contains("input_tokens=100"));
    }

    #[tokio::test]
    async fn explicit_target_uses_the_persisted_session_without_switching_the_active_session() {
        let librarian = Arc::new(FakeLibrarian::default());
        let tool = SessionLibrarianTool::new(librarian.clone());
        let context = tool_context("active-session");

        let output = tool
            .execute(
                json!({
                    "intent": "Summarize the requested persisted session.",
                    "session_id": "persisted-session"
                }),
                context.clone(),
            )
            .await
            .expect("a valid explicit-session invocation should return a tool result");

        assert_eq!(context.session_id, "active-session");
        assert_eq!(librarian.attempted_sessions(), ["persisted-session"]);
        assert_eq!(librarian.provider_requests(), 1);
        assert!(output.output.contains("persisted-session"));
    }

    #[tokio::test]
    async fn missing_explicit_session_fails_actionably_before_provider_or_artifact_work() {
        let librarian = Arc::new(FakeLibrarian::default());
        let tool = SessionLibrarianTool::new(librarian.clone());

        let output = tool
            .execute(
                json!({
                    "intent": "Summarize the requested persisted session.",
                    "session_id": "missing-session"
                }),
                tool_context("active-session"),
            )
            .await
            .expect("a missing source should be represented as one tool result");

        assert_eq!(librarian.attempted_sessions(), ["missing-session"]);
        assert_eq!(librarian.provider_requests(), 0);
        assert_eq!(librarian.artifact_writes(), 0);
        assert!(output.output.contains("source_session_not_found"));
        assert!(output.output.contains("persisted session"));
    }

    #[tokio::test]
    async fn empty_explicit_session_is_rejected_before_orchestration() {
        let librarian = Arc::new(FakeLibrarian::default());
        let tool = SessionLibrarianTool::new(librarian.clone());

        let output = tool
            .execute(
                json!({
                    "intent": "Reject an invalid persisted session identifier.",
                    "session_id": "   "
                }),
                tool_context("active-session"),
            )
            .await
            .expect("invalid input should be represented as one actionable tool result");

        assert!(librarian.attempted_sessions().is_empty());
        assert_eq!(librarian.provider_requests(), 0);
        assert_eq!(librarian.artifact_writes(), 0);
        assert!(output.output.contains("session_id"));
        assert!(output.output.contains("non-empty"));
    }

    #[tokio::test]
    async fn concurrent_duplicate_is_rejected_without_a_second_orchestration_attempt() {
        let librarian = Arc::new(FakeLibrarian::default());
        let tool = Arc::new(SessionLibrarianTool::new(librarian.clone()));
        let started = librarian.blocked_started.notified();
        let first_tool = tool.clone();
        let first = tokio::spawn(async move {
            first_tool
                .execute(
                    json!({
                        "intent": "Start the first bounded attempt.",
                        "session_id": "blocked-session"
                    }),
                    tool_context("active-session"),
                )
                .await
        });
        started.await;

        let duplicate = tokio::time::timeout(
            Duration::from_secs(1),
            tool.execute(
                json!({
                    "intent": "Do not duplicate an in-flight attempt.",
                    "session_id": "blocked-session"
                }),
                tool_context("active-session"),
            ),
        )
        .await
        .expect("duplicate detection must not wait for the first invocation")
        .expect("a duplicate invocation should be represented as one tool result");

        assert_eq!(librarian.attempted_sessions(), ["blocked-session"]);
        assert_eq!(librarian.provider_requests(), 0);
        assert!(duplicate.output.contains("already"));
        assert!(duplicate.output.contains("blocked-session"));

        librarian.release_blocked.notify_waiters();
        first
            .await
            .expect("the first invocation task should finish")
            .expect("the first invocation should return a tool result");
        assert_eq!(librarian.provider_requests(), 1);
    }

    #[tokio::test]
    async fn orchestration_failures_return_one_actionable_result_without_secrets() {
        let librarian = Arc::new(FakeLibrarian::default());
        let tool = SessionLibrarianTool::new(librarian.clone());

        let output = tool
            .execute(
                json!({
                    "intent": "Exercise the failure boundary.",
                    "session_id": "orchestration-failure"
                }),
                tool_context("active-session"),
            )
            .await
            .expect("an orchestration failure should be represented as one tool result");

        assert_eq!(librarian.attempted_sessions(), ["orchestration-failure"]);
        assert_eq!(librarian.provider_requests(), 0);
        assert_eq!(librarian.artifact_writes(), 0);
        assert!(output.output.contains("librarian_generation_failed"));
        assert!(!output.output.contains(SECRET_SENTINEL));
    }
}
