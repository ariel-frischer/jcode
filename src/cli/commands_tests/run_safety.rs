use crate::cli::commands::run_safety::{RunCommandReport, reject_schema};

#[test]
fn legacy_unset_run_report_omits_bounded_stop_fields() {
    let report = RunCommandReport {
        session_id: "session".to_string(),
        provider: "test".to_string(),
        model: "model".to_string(),
        text: "ok".to_string(),
        usage: crate::agent::TokenUsage::default(),
        stop_reason: None,
        outcome: None,
        observed_usage: None,
        safety_bound: None,
    };
    let encoded = serde_json::to_value(report).expect("report should serialize");
    assert!(encoded.get("stop_reason").is_none());
    assert!(encoded.get("outcome").is_none());
    assert!(encoded.get("observed_usage").is_none());
}

#[test]
fn run_safety_cli_precedence_matches_invocation_environment_persisted() {
    let mut candidates = crate::agent::run_safety::RunSafetyCandidates::default();
    candidates.persisted.max_turns = Some("9".to_string());
    candidates.environment.max_turns = Some("7".to_string());
    candidates.invocation.max_turns = Some("3".to_string());
    candidates.persisted.max_tool_steps = Some("8".to_string());
    candidates.environment.token_budget = Some("100".to_string());
    candidates.invocation.deadline = Some("2030-01-01T00:00:00Z".to_string());
    let policy = crate::agent::run_safety::resolve_run_safety(&candidates, Default::default())
        .expect("precedence should resolve");
    assert_eq!(policy.max_turns.map(std::num::NonZeroU64::get), Some(3));
    assert_eq!(
        policy.sources.max_turns,
        crate::agent::run_safety::RunSafetySource::Invocation
    );
    assert_eq!(
        policy.max_tool_steps.map(std::num::NonZeroU64::get),
        Some(8)
    );
    assert_eq!(
        policy.sources.max_tool_steps,
        crate::agent::run_safety::RunSafetySource::Persisted
    );
    assert_eq!(
        policy.token_budget.map(std::num::NonZeroU64::get),
        Some(100)
    );
    assert_eq!(
        policy.sources.token_budget,
        crate::agent::run_safety::RunSafetySource::Environment
    );
    assert!(policy.deadline.is_some());
    assert_eq!(
        policy.sources.deadline,
        crate::agent::run_safety::RunSafetySource::Invocation
    );
}

#[test]
fn invalid_run_safety_preflight_rejects_before_agent_start() {
    let mut candidates = crate::agent::run_safety::RunSafetyCandidates::default();
    candidates.invocation.token_budget = Some("0".to_string());

    let error = crate::agent::run_safety::resolve_run_safety(&candidates, Default::default())
        .expect_err("invalid safety input must fail during preflight");
    assert_eq!(
        error.bound,
        crate::agent::run_safety::RunSafetyBound::TokenBudget
    );
    assert_eq!(
        error.source,
        crate::agent::run_safety::RunSafetySource::Invocation
    );
    assert!(error.to_string().contains("positive decimal whole number"));
}

#[test]
fn bounded_json_report_includes_canonical_reason_and_effective_source() {
    let metadata = crate::agent::run_safety::RunSafetyStopMetadata {
        bound: crate::agent::run_safety::RunSafetyBound::TokenBudget,
        source: crate::agent::run_safety::RunSafetySource::Environment,
    };
    let report = RunCommandReport {
        session_id: "session".to_string(),
        provider: "test".to_string(),
        model: "model".to_string(),
        text: "partial".to_string(),
        usage: crate::agent::TokenUsage::default(),
        stop_reason: Some("token_budget_exceeded".to_string()),
        outcome: Some("bounded_stop".to_string()),
        observed_usage: Some(100),
        safety_bound: Some(metadata),
    };
    let encoded = serde_json::to_value(report).expect("report should serialize");
    assert_eq!(encoded["stop_reason"], "token_budget_exceeded");
    assert_eq!(encoded["outcome"], "bounded_stop");
    assert_eq!(encoded["observed_usage"], 100);
    assert_eq!(encoded["safety_bound"]["bound"], "token_budget");
    assert_eq!(encoded["safety_bound"]["source"], "environment");
}

#[test]
fn bounded_stop_reasons_keep_stable_codes_labels_and_bound_metadata() {
    let cases = [
        (
            crate::agent::run_safety::RunStopReason::MaxTurnsExceeded,
            "max_turns_exceeded",
            "maximum turns exceeded",
            "max_turns",
        ),
        (
            crate::agent::run_safety::RunStopReason::MaxToolStepsExceeded,
            "max_tool_steps_exceeded",
            "maximum tool steps exceeded",
            "max_tool_steps",
        ),
        (
            crate::agent::run_safety::RunStopReason::TokenBudgetExceeded,
            "token_budget_exceeded",
            "token budget exceeded",
            "token_budget",
        ),
        (
            crate::agent::run_safety::RunStopReason::DeadlineExceeded,
            "deadline_exceeded",
            "deadline exceeded",
            "deadline",
        ),
    ];

    for (reason, code, label, bound) in cases {
        assert_eq!(reason.code(), code);
        assert_eq!(reason.label(), label);
        assert_eq!(reason.bound().name(), bound);
        let metadata = crate::agent::run_safety::RunSafetyStopMetadata {
            bound: reason.bound(),
            source: crate::agent::run_safety::RunSafetySource::Invocation,
        };
        let encoded = serde_json::to_value(metadata).expect("metadata should serialize");
        assert_eq!(encoded["bound"], bound);
        assert_eq!(
            encoded["source"],
            serde_json::Value::String("invocation".to_string())
        );
        assert!(format!("Run stopped: {label} ({code})").contains(label));
    }
}

#[test]
fn schema_mode_rejects_explicit_run_safety_flags_before_bridge_start() {
    let safety = crate::config::RunSafetyConfig {
        max_turns: Some("1".to_string()),
        ..Default::default()
    };
    let error = reject_schema(&safety).expect_err("schema must reject bound");
    assert!(error.to_string().contains("unsupported with --schema"));
    reject_schema(&Default::default()).expect("unset safety is allowed");
}
