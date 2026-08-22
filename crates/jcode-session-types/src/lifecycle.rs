//! Stable lifecycle observability contracts.
//!
//! These types are intentionally dependency-light and contain only approved
//! operational metadata. Raw prompts, commands, command output, environment
//! values, secrets, and todo text have no representation in this module.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const LIFECYCLE_SCHEMA_VERSION: u16 = 1;
pub const LIFECYCLE_POLICY_VERSION: u16 = 1;

/// One ordered, versioned lifecycle observation owned by a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LifecycleEventEnvelope {
    pub schema_version: u16,
    pub session_id: String,
    pub sequence: u64,
    pub recorded_at: DateTime<Utc>,
    pub event: LifecycleEvent,
}

/// Exhaustive set of lifecycle categories supported by the built-in recorder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "category", rename_all = "snake_case")]
pub enum LifecycleEvent {
    PolicySnapshot {
        snapshot: EffectivePolicySnapshot,
    },
    Compaction {
        decision_type: LifecycleDecisionType,
        semantic_reason: LifecycleSemanticReason,
        suppression_reason: Option<LifecycleSuppressionReason>,
        context_usage: Option<ContextUsage>,
        process_manifest_id: Option<String>,
    },
    Handoff {
        decision_type: LifecycleDecisionType,
        semantic_reason: LifecycleSemanticReason,
        suppression_reason: Option<LifecycleSuppressionReason>,
        payload: HandoffLifecyclePayload,
        process_manifest_id: Option<String>,
    },
    Retry {
        decision_type: LifecycleDecisionType,
        semantic_reason: LifecycleSemanticReason,
        suppression_reason: Option<LifecycleSuppressionReason>,
        attempt: u32,
        max_attempts: u32,
        process_manifest_id: Option<String>,
    },
    StrategySwitch {
        decision_type: LifecycleDecisionType,
        semantic_reason: LifecycleSemanticReason,
        suppression_reason: Option<LifecycleSuppressionReason>,
    },
    Block {
        decision_type: LifecycleDecisionType,
        semantic_reason: LifecycleSemanticReason,
        suppression_reason: Option<LifecycleSuppressionReason>,
        process_manifest_id: Option<String>,
    },
}

/// Closed lifecycle outcome vocabulary shared by all decision categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleDecisionType {
    Attempted,
    Accepted,
    Suppressed,
    Started,
    Completed,
    Failed,
    Exhausted,
    Pending,
}

/// Closed allow-list of semantic reasons. No caller-provided free-form text is
/// accepted by the lifecycle contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleSemanticReason {
    Manual,
    Automatic,
    NativeFallback,
    ContextLimit,
    AlreadyWithinBudget,
    RetryableFailure,
    ProviderFallback,
    Policy,
    Startup,
    ChildStartup,
    Shutdown,
}

/// Closed allow-list for why a lifecycle decision was not applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleSuppressionReason {
    Disabled,
    AlreadyWithinBudget,
    ConfirmationRequired,
    ChainLimit,
    PolicyDenied,
    NoChildSession,
    QueueFull,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ContextUsage {
    pub used_tokens: u64,
    pub context_window_tokens: u64,
    pub ratio: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionPolicyMode {
    Reactive,
    Proactive,
    Semantic,
    Native,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactionPolicySnapshot {
    pub mode: CompactionPolicyMode,
    pub context_window_tokens: u64,
    pub threshold_ratio: f32,
    pub native_compaction: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffPolicySnapshot {
    pub enabled: bool,
    pub agent_enabled: bool,
    pub confirmation_required: bool,
    pub auto_start: bool,
    pub max_chain_transitions: usize,
    pub copy_todos: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectivePolicySnapshot {
    pub policy_version: u16,
    pub fingerprint: String,
    pub compaction: CompactionPolicySnapshot,
    pub handoff: HandoffPolicySnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffLifecyclePayload {
    pub chain_depth: usize,
    pub generated_prompt_bytes: usize,
    pub todo_carryover_count: usize,
    pub parent_session_id: String,
    pub child_session_id: Option<String>,
    pub startup_acknowledged: Option<bool>,
    pub startup_outcome: HandoffStartupOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffStartupOutcome {
    Pending,
    Started,
    Completed,
    Failed,
    Incomplete,
}

/// Opaque reference to a durable background-process manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessManifestReference {
    pub process_manifest_id: String,
    pub session_id: String,
}

/// Effective local outputs after applying the master switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleObservabilityStatus {
    pub enabled: bool,
    pub persist_session_events: bool,
    pub emit_structured_logs: bool,
}

impl LifecycleObservabilityStatus {
    pub const DISABLED: Self = Self {
        enabled: false,
        persist_session_events: false,
        emit_structured_logs: false,
    };
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionLifecycleStream {
    pub session_id: String,
    pub status: LifecycleObservabilityStatus,
    pub events: Vec<LifecycleEventEnvelope>,
    pub warnings: Vec<LifecycleCompatibilityWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LifecycleCompatibilityWarning {
    TornTail { line: usize },
    MalformedRecord { line: usize },
    UnsupportedSchemaVersion { line: usize, version: u16 },
    PersistenceUnavailable,
    DroppedEvent,
}

impl LifecycleCompatibilityWarning {
    pub fn unsupported_schema_version(line: usize, version: u16) -> Self {
        Self::UnsupportedSchemaVersion { line, version }
    }

    pub fn line(&self) -> usize {
        match self {
            Self::TornTail { line }
            | Self::MalformedRecord { line }
            | Self::UnsupportedSchemaVersion { line, .. } => *line,
            Self::PersistenceUnavailable | Self::DroppedEvent => 0,
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::TornTail { line } => format!("incomplete lifecycle record at line {line}"),
            Self::MalformedRecord { line } => format!("malformed lifecycle record at line {line}"),
            Self::UnsupportedSchemaVersion { line, version } => {
                format!("lifecycle record at line {line} uses newer schema version {version}")
            }
            Self::PersistenceUnavailable => "lifecycle persistence is unavailable".to_string(),
            Self::DroppedEvent => "lifecycle event was dropped before delivery".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn envelope(event: LifecycleEvent) -> LifecycleEventEnvelope {
        LifecycleEventEnvelope {
            schema_version: LIFECYCLE_SCHEMA_VERSION,
            session_id: "session-contract-test".to_string(),
            sequence: 7,
            recorded_at: Utc.timestamp_opt(1_700_000_007, 0).unwrap(),
            event,
        }
    }

    fn policy_snapshot() -> EffectivePolicySnapshot {
        EffectivePolicySnapshot {
            policy_version: LIFECYCLE_POLICY_VERSION,
            fingerprint: "policy-fingerprint-safe".to_string(),
            compaction: CompactionPolicySnapshot {
                mode: CompactionPolicyMode::Reactive,
                context_window_tokens: 128_000,
                threshold_ratio: 0.85,
                native_compaction: false,
            },
            handoff: HandoffPolicySnapshot {
                enabled: true,
                agent_enabled: true,
                confirmation_required: false,
                auto_start: true,
                max_chain_transitions: 8,
                copy_todos: true,
            },
        }
    }

    #[test]
    fn schema_version_and_all_categories_round_trip() {
        let events = vec![
            LifecycleEvent::PolicySnapshot {
                snapshot: policy_snapshot(),
            },
            LifecycleEvent::Compaction {
                decision_type: LifecycleDecisionType::Accepted,
                semantic_reason: LifecycleSemanticReason::ContextLimit,
                suppression_reason: None,
                context_usage: Some(ContextUsage {
                    used_tokens: 108_000,
                    context_window_tokens: 128_000,
                    ratio: 0.84375,
                }),
                process_manifest_id: None,
            },
            LifecycleEvent::Handoff {
                decision_type: LifecycleDecisionType::Completed,
                semantic_reason: LifecycleSemanticReason::Manual,
                suppression_reason: None,
                payload: HandoffLifecyclePayload {
                    chain_depth: 2,
                    generated_prompt_bytes: 512,
                    todo_carryover_count: 3,
                    parent_session_id: "parent-session".to_string(),
                    child_session_id: Some("child-session".to_string()),
                    startup_acknowledged: Some(true),
                    startup_outcome: HandoffStartupOutcome::Completed,
                },
                process_manifest_id: Some("task-manifest-opaque".to_string()),
            },
            LifecycleEvent::Retry {
                decision_type: LifecycleDecisionType::Exhausted,
                semantic_reason: LifecycleSemanticReason::RetryableFailure,
                suppression_reason: None,
                attempt: 3,
                max_attempts: 3,
                process_manifest_id: None,
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
                process_manifest_id: None,
            },
        ];

        for event in events {
            let expected = envelope(event);
            let encoded = serde_json::to_string(&expected).unwrap();
            let decoded: LifecycleEventEnvelope = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, expected);
            assert_eq!(decoded.schema_version, 1);
            assert_eq!(decoded.sequence, 7);
            assert_eq!(
                decoded.recorded_at,
                Utc.timestamp_opt(1_700_000_007, 0).unwrap()
            );
        }
    }

    #[test]
    fn required_decision_fields_are_tagged_and_safe() {
        let event = LifecycleEvent::Compaction {
            decision_type: LifecycleDecisionType::Suppressed,
            semantic_reason: LifecycleSemanticReason::AlreadyWithinBudget,
            suppression_reason: Some(LifecycleSuppressionReason::Disabled),
            context_usage: Some(ContextUsage {
                used_tokens: 1,
                context_window_tokens: 2,
                ratio: 0.5,
            }),
            process_manifest_id: Some("opaque-task-id".to_string()),
        };

        let json = serde_json::to_value(envelope(event)).unwrap();
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["event"]["category"], "compaction");
        assert_eq!(json["event"]["decision_type"], "suppressed");
        assert_eq!(json["event"]["semantic_reason"], "already_within_budget");
        assert_eq!(json["event"]["suppression_reason"], "disabled");
        assert_eq!(json["event"]["process_manifest_id"], "opaque-task-id");
        assert!(json.get("prompt").is_none());
        assert!(json.get("command").is_none());
        assert!(json.get("output").is_none());
    }

    #[test]
    fn unsupported_newer_schema_is_reported_as_a_warning() {
        let value = serde_json::json!({
            "schema_version": LIFECYCLE_SCHEMA_VERSION + 1,
            "session_id": "session-contract-test",
            "sequence": 1,
            "recorded_at": "2023-11-14T22:13:20Z",
            "event": {
                "category": "compaction",
                "decision_type": "accepted",
                "semantic_reason": "manual",
                "suppression_reason": null,
                "context_usage": null,
                "process_manifest_id": null
            }
        });

        let warning = LifecycleCompatibilityWarning::unsupported_schema_version(
            1,
            LIFECYCLE_SCHEMA_VERSION + 1,
        );
        assert_eq!(warning.line(), 1);
        assert!(warning.message().contains("newer"));
        assert!(value["schema_version"].as_u64().unwrap() > 1);
    }
}
