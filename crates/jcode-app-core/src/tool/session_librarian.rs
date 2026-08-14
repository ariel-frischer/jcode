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
