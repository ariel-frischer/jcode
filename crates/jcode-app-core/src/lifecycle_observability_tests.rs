//! Shared fixtures for lifecycle observability tests.
//!
//! These helpers deliberately use an isolated temporary directory, synthetic
//! identifiers, and a deterministic clock. They do not start the shared
//! daemon or access remote telemetry.

#![allow(dead_code)]

use chrono::{DateTime, Duration, Utc};
use jcode_session_types::lifecycle::{
    CompactionPolicyMode, CompactionPolicySnapshot, EffectivePolicySnapshot, HandoffPolicySnapshot,
    LIFECYCLE_POLICY_VERSION, LifecycleDecisionType, LifecycleEvent, LifecycleSemanticReason,
};
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

struct LifecycleTestProvider;

#[async_trait::async_trait]
impl crate::provider::Provider for LifecycleTestProvider {
    async fn complete(
        &self,
        _messages: &[crate::message::Message],
        _tools: &[crate::message::ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> anyhow::Result<crate::provider::EventStream> {
        anyhow::bail!("lifecycle test provider should not complete")
    }

    fn name(&self) -> &str {
        "lifecycle-test"
    }

    fn fork(&self) -> Arc<dyn crate::provider::Provider> {
        Arc::new(Self)
    }
}

pub(crate) const TEST_SESSION_ID: &str = "synthetic-session-001";
const BASE_TIMESTAMP_SECONDS: i64 = 1_700_000_000;

/// A self-contained fixture for one synthetic lifecycle session.
pub(crate) struct LifecycleTestHarness {
    pub(crate) temp_root: TempDir,
    pub(crate) session_dir: PathBuf,
    pub(crate) base_time: DateTime<Utc>,
}

impl LifecycleTestHarness {
    pub(crate) fn new() -> Self {
        let temp_root = tempfile::tempdir().expect("create lifecycle test root");
        let session_dir = temp_root.path().join(TEST_SESSION_ID);
        std::fs::create_dir(&session_dir).expect("create synthetic lifecycle session dir");

        Self {
            temp_root,
            session_dir,
            base_time: DateTime::from_timestamp(BASE_TIMESTAMP_SECONDS, 0)
                .expect("valid deterministic lifecycle timestamp"),
        }
    }

    pub(crate) fn timestamp(&self, offset_seconds: i64) -> DateTime<Utc> {
        self.base_time + Duration::seconds(offset_seconds)
    }

    /// Yield once so async recorder tests can let queued work make progress.
    pub(crate) async fn yield_to_recorder() {
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn lifecycle_harness_is_isolated_and_deterministic() {
    let harness = LifecycleTestHarness::new();

    assert!(harness.session_dir.is_dir());
    assert_eq!(
        harness
            .session_dir
            .file_name()
            .and_then(|name| name.to_str()),
        Some(TEST_SESSION_ID)
    );
    assert_eq!(harness.timestamp(0), harness.base_time);
    assert_eq!(harness.timestamp(5).timestamp(), BASE_TIMESTAMP_SECONDS + 5);

    LifecycleTestHarness::yield_to_recorder().await;
}

fn compaction_event() -> LifecycleEvent {
    LifecycleEvent::Compaction {
        decision_type: LifecycleDecisionType::Accepted,
        semantic_reason: LifecycleSemanticReason::Automatic,
        suppression_reason: None,
        context_usage: None,
        process_manifest_id: None,
    }
}

fn policy_snapshot(fingerprint: &str) -> LifecycleEvent {
    LifecycleEvent::PolicySnapshot {
        snapshot: EffectivePolicySnapshot {
            policy_version: LIFECYCLE_POLICY_VERSION,
            fingerprint: fingerprint.to_string(),
            compaction: CompactionPolicySnapshot {
                mode: CompactionPolicyMode::Semantic,
                context_window_tokens: 200_000,
                threshold_ratio: 0.8,
                native_compaction: false,
            },
            handoff: HandoffPolicySnapshot {
                enabled: true,
                agent_enabled: true,
                confirmation_required: false,
                auto_start: true,
                max_chain_transitions: 4,
                copy_todos: true,
            },
        },
    }
}

#[tokio::test]
async fn lifecycle_recorder_respects_enablement_fanout_ordering_and_policy_deduplication() {
    use crate::config::LifecycleObservabilityConfig;
    use crate::lifecycle_observability::{LifecycleRecorder, LifecycleSubmitOutcome};

    let disabled_harness = LifecycleTestHarness::new();
    let disabled = LifecycleRecorder::new_with_clock(
        LifecycleObservabilityConfig {
            enabled: false,
            persist_session_events: true,
            emit_structured_logs: true,
        },
        disabled_harness.temp_root.path().to_path_buf(),
        8,
        Arc::new({
            let timestamp = disabled_harness.timestamp(0);
            move || timestamp
        }),
    );
    assert_eq!(
        disabled.submit(TEST_SESSION_ID, compaction_event()),
        LifecycleSubmitOutcome::Disabled
    );
    assert!(disabled.flush().await.is_empty());
    assert!(
        !crate::session::lifecycle_path_in_dir(disabled_harness.temp_root.path(), TEST_SESSION_ID)
            .expect("valid lifecycle path")
            .exists()
    );

    let harness = LifecycleTestHarness::new();
    let recorder = LifecycleRecorder::new_with_clock(
        LifecycleObservabilityConfig {
            enabled: true,
            persist_session_events: true,
            emit_structured_logs: false,
        },
        harness.temp_root.path().to_path_buf(),
        8,
        Arc::new({
            let timestamp = harness.timestamp(5);
            move || timestamp
        }),
    );
    assert_eq!(
        recorder.submit(TEST_SESSION_ID, policy_snapshot("policy-v1")),
        LifecycleSubmitOutcome::Accepted
    );
    assert_eq!(
        recorder.submit(TEST_SESSION_ID, policy_snapshot("policy-v1")),
        LifecycleSubmitOutcome::DuplicatePolicy
    );
    assert_eq!(
        recorder.submit(TEST_SESSION_ID, compaction_event()),
        LifecycleSubmitOutcome::Accepted
    );
    assert!(recorder.flush().await.is_empty());

    let stream = crate::session::read_lifecycle_stream_in_dir(
        harness.temp_root.path(),
        TEST_SESSION_ID,
        recorder.status(),
    )
    .expect("read recorder output");
    assert_eq!(stream.events.len(), 2);
    assert_eq!(stream.events[0].sequence, 1);
    assert_eq!(stream.events[1].sequence, 2);
    assert_eq!(stream.events[0].recorded_at, harness.timestamp(5));
}

#[tokio::test]
async fn lifecycle_recorder_output_matrix_only_persists_when_that_sink_is_enabled() {
    use crate::config::LifecycleObservabilityConfig;
    use crate::lifecycle_observability::{LifecycleRecorder, LifecycleSubmitOutcome};

    for (enabled, persist_session_events, emit_structured_logs) in [
        (false, false, false),
        (false, true, true),
        (true, false, false),
        (true, false, true),
        (true, true, false),
        (true, true, true),
    ] {
        let harness = LifecycleTestHarness::new();
        let config = LifecycleObservabilityConfig {
            enabled,
            persist_session_events,
            emit_structured_logs,
        };
        let expected_status = config.effective_status();
        let recorder = LifecycleRecorder::new_with_clock(
            config,
            harness.temp_root.path().to_path_buf(),
            8,
            Arc::new({
                let timestamp = harness.timestamp(2);
                move || timestamp
            }),
        );

        let expected_outcome = if enabled {
            LifecycleSubmitOutcome::Accepted
        } else {
            LifecycleSubmitOutcome::Disabled
        };
        assert_eq!(
            recorder.submit(TEST_SESSION_ID, compaction_event()),
            expected_outcome,
            "unexpected recorder result for enabled={enabled}, persistence={persist_session_events}, logs={emit_structured_logs}"
        );
        assert!(recorder.flush().await.is_empty());

        let stream = crate::session::read_lifecycle_stream_in_dir(
            harness.temp_root.path(),
            TEST_SESSION_ID,
            recorder.status(),
        )
        .expect("read output matrix stream");
        assert_eq!(
            stream.status, expected_status,
            "effective status mismatch for enabled={enabled}, persistence={persist_session_events}, logs={emit_structured_logs}"
        );
        assert_eq!(
            stream.events.len(),
            usize::from(enabled && persist_session_events),
            "persistence sink mismatch for enabled={enabled}, persistence={persist_session_events}, logs={emit_structured_logs}"
        );
        assert_eq!(
            crate::session::lifecycle_path_in_dir(harness.temp_root.path(), TEST_SESSION_ID)
                .expect("valid lifecycle path")
                .exists(),
            enabled && persist_session_events,
            "sidecar presence mismatch for enabled={enabled}, persistence={persist_session_events}, logs={emit_structured_logs}"
        );
    }
}

#[tokio::test]
async fn lifecycle_recorder_isolates_sink_and_queue_failures_and_filters_opaque_ids() {
    use crate::config::LifecycleObservabilityConfig;
    use crate::lifecycle_observability::{
        LifecycleRecorder, LifecycleRecorderDiagnostic, LifecycleSubmitOutcome,
    };

    let harness = LifecycleTestHarness::new();
    let invalid_base = harness.temp_root.path().join("not-a-directory");
    std::fs::write(&invalid_base, b"file").expect("create invalid storage root");
    let recorder = LifecycleRecorder::new_with_clock(
        LifecycleObservabilityConfig {
            enabled: true,
            persist_session_events: true,
            emit_structured_logs: false,
        },
        invalid_base,
        1,
        Arc::new({
            let timestamp = harness.timestamp(0);
            move || timestamp
        }),
    );

    let mut saw_queue_full = false;
    for _ in 0..10_000 {
        if recorder.submit(TEST_SESSION_ID, compaction_event()) == LifecycleSubmitOutcome::QueueFull
        {
            saw_queue_full = true;
            break;
        }
    }
    let diagnostics = recorder.flush().await;
    assert!(saw_queue_full);
    assert!(diagnostics.contains(&LifecycleRecorderDiagnostic::QueueFull));
    assert!(diagnostics.contains(&LifecycleRecorderDiagnostic::PersistenceFailure));

    let log_only = LifecycleRecorder::new_with_clock(
        LifecycleObservabilityConfig {
            enabled: true,
            persist_session_events: false,
            emit_structured_logs: true,
        },
        harness.temp_root.path().to_path_buf(),
        8,
        Arc::new({
            let timestamp = harness.timestamp(1);
            move || timestamp
        }),
    );
    let unsafe_event = LifecycleEvent::Block {
        decision_type: LifecycleDecisionType::Suppressed,
        semantic_reason: LifecycleSemanticReason::Policy,
        suppression_reason: None,
        process_manifest_id: Some("secret value with spaces".to_string()),
    };
    assert_eq!(
        log_only.submit(TEST_SESSION_ID, unsafe_event),
        LifecycleSubmitOutcome::Accepted
    );
    assert_eq!(
        log_only.submit("session id with spaces", compaction_event()),
        LifecycleSubmitOutcome::Rejected
    );
    assert!(
        log_only
            .flush()
            .await
            .contains(&LifecycleRecorderDiagnostic::InvalidPayload)
    );
    assert!(
        !crate::session::lifecycle_path_in_dir(harness.temp_root.path(), TEST_SESSION_ID)
            .expect("valid lifecycle path")
            .exists()
    );
}

#[tokio::test]
async fn agent_retains_injected_recorder_and_emits_one_policy_snapshot() {
    use crate::agent::Agent;
    use crate::config::LifecycleObservabilityConfig;
    use crate::lifecycle_observability::LifecycleRecorder;
    use crate::provider::Provider;
    use crate::tool::Registry;

    let harness = LifecycleTestHarness::new();
    let recorder = LifecycleRecorder::new_with_clock(
        LifecycleObservabilityConfig::default(),
        harness.temp_root.path().to_path_buf(),
        8,
        Arc::new({
            let timestamp = harness.timestamp(0);
            move || timestamp
        }),
    );
    let provider: Arc<dyn Provider> = Arc::new(LifecycleTestProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);
    let session_id = agent.session_id().to_string();

    assert!(!agent.has_lifecycle_recorder());
    agent.attach_lifecycle_recorder(recorder.clone());
    assert!(agent.has_lifecycle_recorder());
    agent.emit_effective_lifecycle_policy_snapshot();
    assert!(recorder.flush().await.is_empty());

    let stream = crate::session::read_lifecycle_stream_in_dir(
        harness.temp_root.path(),
        &session_id,
        recorder.status(),
    )
    .expect("read policy snapshot");
    assert_eq!(stream.events.len(), 1);
    assert!(matches!(
        stream.events[0].event,
        LifecycleEvent::PolicySnapshot { .. }
    ));
}

#[tokio::test]
async fn agent_decision_helpers_emit_typed_events_without_sensitive_details() {
    use crate::agent::Agent;
    use crate::config::LifecycleObservabilityConfig;
    use crate::lifecycle_observability::LifecycleRecorder;
    use crate::provider::Provider;
    use crate::session::lifecycle_types::{
        ContextUsage, LifecycleDecisionType, LifecycleSemanticReason, LifecycleSuppressionReason,
    };
    use crate::tool::Registry;

    let harness = LifecycleTestHarness::new();
    let recorder = LifecycleRecorder::new_with_clock(
        LifecycleObservabilityConfig::default(),
        harness.temp_root.path().to_path_buf(),
        16,
        Arc::new({
            let timestamp = harness.timestamp(0);
            move || timestamp
        }),
    );
    let provider: Arc<dyn Provider> = Arc::new(LifecycleTestProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);
    let session_id = agent.session_id().to_string();
    agent.attach_lifecycle_recorder(recorder.clone());

    agent.record_compaction_lifecycle(
        LifecycleDecisionType::Accepted,
        LifecycleSemanticReason::Manual,
        None,
        Some(ContextUsage {
            used_tokens: 12_000,
            context_window_tokens: 16_000,
            ratio: 0.75,
        }),
    );
    agent.record_retry_lifecycle(
        LifecycleDecisionType::Exhausted,
        LifecycleSemanticReason::RetryableFailure,
        None,
        3,
        3,
    );
    agent.record_strategy_switch_lifecycle(
        LifecycleDecisionType::Completed,
        LifecycleSemanticReason::ProviderFallback,
    );
    agent.record_block_lifecycle(
        LifecycleDecisionType::Suppressed,
        LifecycleSemanticReason::Policy,
        Some(LifecycleSuppressionReason::PolicyDenied),
    );

    assert!(recorder.flush().await.is_empty());
    let stream = crate::session::read_lifecycle_stream_in_dir(
        harness.temp_root.path(),
        &session_id,
        recorder.status(),
    )
    .expect("read decision events");
    assert_eq!(
        stream.events.len(),
        5,
        "policy snapshot plus four decisions"
    );
    assert!(matches!(
        stream.events[1].event,
        LifecycleEvent::Compaction {
            decision_type: LifecycleDecisionType::Accepted,
            context_usage: Some(_),
            ..
        }
    ));
    assert!(matches!(
        stream.events[2].event,
        LifecycleEvent::Retry {
            decision_type: LifecycleDecisionType::Exhausted,
            attempt: 3,
            max_attempts: 3,
            ..
        }
    ));
    assert!(matches!(
        stream.events[3].event,
        LifecycleEvent::StrategySwitch {
            decision_type: LifecycleDecisionType::Completed,
            ..
        }
    ));
    assert!(matches!(
        stream.events[4].event,
        LifecycleEvent::Block {
            suppression_reason: Some(LifecycleSuppressionReason::PolicyDenied),
            ..
        }
    ));

    let serialized = serde_json::to_string(&stream).expect("serialize decision stream");
    for sensitive in [
        "super-secret-value",
        "raw prompt value",
        "rm -rf",
        "captured output",
        "unnecessary todo text",
    ] {
        assert!(
            !serialized.contains(sensitive),
            "sensitive value leaked: {sensitive}"
        );
    }
}

#[tokio::test]
async fn server_owns_one_shared_lifecycle_recorder() {
    use crate::provider::Provider;
    use crate::server::Server;

    let provider: Arc<dyn Provider> = Arc::new(LifecycleTestProvider);
    let server = Server::new(provider);
    let first = server.lifecycle_recorder();
    let second = server.lifecycle_recorder();
    assert!(Arc::ptr_eq(&first, &second));
}

#[tokio::test]
async fn lifecycle_recorder_redacts_sensitive_values_across_all_categories_and_sinks() {
    use crate::config::LifecycleObservabilityConfig;
    use crate::lifecycle_observability::{LifecycleRecorder, LifecycleSubmitOutcome};
    use crate::session::lifecycle_types::{
        ContextUsage, HandoffLifecyclePayload, HandoffStartupOutcome, LifecycleDecisionType,
        LifecycleEventEnvelope, LifecycleSemanticReason, LifecycleSuppressionReason,
    };

    let harness = LifecycleTestHarness::new();
    let recorder = LifecycleRecorder::new_with_clock(
        LifecycleObservabilityConfig {
            enabled: true,
            persist_session_events: true,
            emit_structured_logs: true,
        },
        harness.temp_root.path().to_path_buf(),
        32,
        Arc::new({
            let timestamp = harness.timestamp(2);
            move || timestamp
        }),
    );

    let sensitive_values = [
        "synthetic prompt value",
        "synthetic secret value",
        "synthetic command text",
        "synthetic command output",
        "synthetic environment value",
        "synthetic raw error",
        "synthetic todo text",
    ];
    let sensitive_id = sensitive_values.join(" ");
    let events = vec![
        LifecycleEvent::PolicySnapshot {
            snapshot: EffectivePolicySnapshot {
                policy_version: LIFECYCLE_POLICY_VERSION,
                fingerprint: sensitive_id.clone(),
                compaction: CompactionPolicySnapshot {
                    mode: CompactionPolicyMode::Semantic,
                    context_window_tokens: 128_000,
                    threshold_ratio: 0.8,
                    native_compaction: false,
                },
                handoff: HandoffPolicySnapshot {
                    enabled: true,
                    agent_enabled: true,
                    confirmation_required: false,
                    auto_start: true,
                    max_chain_transitions: 4,
                    copy_todos: true,
                },
            },
        },
        LifecycleEvent::Compaction {
            decision_type: LifecycleDecisionType::Accepted,
            semantic_reason: LifecycleSemanticReason::Automatic,
            suppression_reason: None,
            context_usage: Some(ContextUsage {
                used_tokens: 10_000,
                context_window_tokens: 12_000,
                ratio: 0.833,
            }),
            process_manifest_id: Some(sensitive_id.clone()),
        },
        LifecycleEvent::Handoff {
            decision_type: LifecycleDecisionType::Completed,
            semantic_reason: LifecycleSemanticReason::ChildStartup,
            suppression_reason: None,
            payload: HandoffLifecyclePayload {
                chain_depth: 2,
                generated_prompt_bytes: sensitive_values[0].len(),
                todo_carryover_count: 3,
                parent_session_id: sensitive_id.clone(),
                child_session_id: Some(sensitive_id.clone()),
                startup_acknowledged: Some(true),
                startup_outcome: HandoffStartupOutcome::Completed,
            },
            process_manifest_id: Some(sensitive_id.clone()),
        },
        LifecycleEvent::Retry {
            decision_type: LifecycleDecisionType::Exhausted,
            semantic_reason: LifecycleSemanticReason::RetryableFailure,
            suppression_reason: Some(LifecycleSuppressionReason::QueueFull),
            attempt: 3,
            max_attempts: 3,
            process_manifest_id: Some(sensitive_id.clone()),
        },
        LifecycleEvent::StrategySwitch {
            decision_type: LifecycleDecisionType::Completed,
            semantic_reason: LifecycleSemanticReason::ProviderFallback,
            suppression_reason: None,
        },
        LifecycleEvent::Block {
            decision_type: LifecycleDecisionType::Suppressed,
            semantic_reason: LifecycleSemanticReason::Policy,
            suppression_reason: Some(LifecycleSuppressionReason::PolicyDenied),
            process_manifest_id: Some(sensitive_id.clone()),
        },
    ];

    for event in &events {
        assert_eq!(
            recorder.submit(TEST_SESSION_ID, event.clone()),
            LifecycleSubmitOutcome::Accepted
        );
    }
    assert!(recorder.flush().await.is_empty());

    let stream = crate::session::read_lifecycle_stream_in_dir(
        harness.temp_root.path(),
        TEST_SESSION_ID,
        recorder.status(),
    )
    .expect("read sanitized lifecycle stream");
    let sidecar = std::fs::read_to_string(
        crate::session::lifecycle_path_in_dir(harness.temp_root.path(), TEST_SESSION_ID)
            .expect("valid lifecycle path"),
    )
    .expect("read persisted lifecycle sidecar");
    let query_projection = serde_json::to_string(&stream).expect("serialize query projection");
    let log_projection = stream
        .events
        .iter()
        .map(|event| {
            serde_json::to_string(&LifecycleEventEnvelope {
                schema_version: event.schema_version,
                session_id: event.session_id.clone(),
                sequence: event.sequence,
                recorded_at: event.recorded_at,
                event: crate::lifecycle_observability::sanitize_event(event.event.clone()),
            })
            .expect("serialize structured log projection")
        })
        .collect::<Vec<_>>()
        .join("\n");

    for sensitive in sensitive_values {
        assert!(
            !sidecar.contains(sensitive),
            "persisted value leaked: {sensitive}"
        );
        assert!(
            !query_projection.contains(sensitive),
            "query value leaked: {sensitive}"
        );
        assert!(
            !log_projection.contains(sensitive),
            "log value leaked: {sensitive}"
        );
    }
    assert!(stream.events.iter().all(|event| {
        let json = serde_json::to_string(event).expect("serialize event");
        !json.contains("pid") && !json.contains("command") && !json.contains("output")
    }));
}
