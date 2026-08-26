use super::{Arc, Provider, Registry, SavedEnv, TestProvider};
use crate::cli::commands::run_safety::{RunCommandReport, reject_schema};

const PROCESS_CHILD_ENV: &str = "JCODE_RUN_SAFETY_PROCESS_CHILD";

#[tokio::test]
async fn run_safety_process_child() {
    let Ok(mode) = std::env::var(PROCESS_CHILD_ENV) else {
        return;
    };

    let _guard = crate::storage::lock_test_env();
    let _saved = SavedEnv::capture(&["JCODE_HOME", "JCODE_RUN_AUTO_POKE"]);
    let temp = tempfile::tempdir().expect("tempdir");
    crate::env::set_var("JCODE_HOME", temp.path());
    crate::env::set_var("JCODE_RUN_AUTO_POKE", "0");
    let provider: Arc<dyn Provider> = Arc::new(TestProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = crate::agent::Agent::new(provider.clone(), registry);
    let candidates = crate::agent::run_safety::RunSafetyCandidates {
        invocation: crate::config::RunSafetyConfig {
            max_turns: Some("1".to_string()),
            ..Default::default()
        },
        ..Default::default()
    };
    crate::cli::commands::run_safety::install(&mut agent, candidates).expect("install run safety");

    let (emit_json, emit_ndjson) = match mode.as_str() {
        "plain" => (false, false),
        "json" => (true, false),
        "ndjson" => (false, true),
        other => panic!("unknown process child mode: {other}"),
    };
    crate::cli::commands::run_single_message_with_agent(
        &mut agent,
        provider,
        "Return the test response.",
        emit_json,
        emit_ndjson,
    )
    .await
    .expect("bounded child run");
}

fn process_output(mode: &str) -> std::process::Output {
    std::process::Command::new(std::env::current_exe().expect("current test executable"))
        .args(["run_safety_process_child", "--nocapture"])
        .env(PROCESS_CHILD_ENV, mode)
        .output()
        .unwrap_or_else(|error| panic!("spawn {mode} process child: {error}"))
}

fn json_objects(output: &str) -> Vec<serde_json::Value> {
    output
        .lines()
        .filter_map(|line| line.find('{').map(|start| &line[start..]))
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn first_json_object(output: &str) -> serde_json::Value {
    let start = output.find('{').expect("JSON object start");
    let end = output[start..]
        .find("\n}")
        .map(|offset| start + offset + 2)
        .expect("JSON object end");
    serde_json::from_str(&output[start..end]).expect("valid JSON report")
}

#[test]
fn bounded_run_output_channels_and_exit_status_match_the_contract() {
    let stop_message = "Run stopped: maximum turns exceeded (max_turns_exceeded)";
    let plain = process_output("plain");
    assert!(plain.status.success(), "plain status: {:?}", plain.status);
    let plain_stdout = String::from_utf8(plain.stdout).expect("plain stdout");
    let plain_stderr = String::from_utf8(plain.stderr).expect("plain stderr");
    assert!(
        plain_stderr.contains(stop_message),
        "stderr: {plain_stderr:?}"
    );
    assert!(
        !plain_stdout.contains(stop_message),
        "stdout: {plain_stdout:?}"
    );

    let json = process_output("json");
    assert!(json.status.success(), "JSON status: {:?}", json.status);
    let json_stdout = String::from_utf8(json.stdout).expect("JSON stdout");
    let report = first_json_object(&json_stdout);
    assert_eq!(report["stop_reason"], "max_turns_exceeded");
    assert_eq!(report["outcome"], "bounded_stop");

    let ndjson = process_output("ndjson");
    assert!(
        ndjson.status.success(),
        "NDJSON status: {:?}",
        ndjson.status
    );
    let values = json_objects(&String::from_utf8(ndjson.stdout).expect("NDJSON stdout"));
    let done = values.last().expect("NDJSON done event");
    assert_eq!(done["type"], "done");
    assert_eq!(done["stop_reason"], "max_turns_exceeded");
    assert_eq!(done["outcome"], "bounded_stop");
}

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
    let candidates = crate::agent::run_safety::RunSafetyCandidates {
        invocation: crate::config::RunSafetyConfig {
            max_turns: Some("3".to_string()),
            deadline: Some("2030-01-01T00:00:00Z".to_string()),
            ..Default::default()
        },
        environment: crate::config::RunSafetyConfig {
            token_budget: Some("100".to_string()),
            ..Default::default()
        },
        persisted: crate::config::RunSafetyConfig {
            max_turns: Some("9".to_string()),
            max_tool_steps: Some("8".to_string()),
            ..Default::default()
        },
    };
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
        limit: None,
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
            crate::agent::run_safety::RunStopReason::MaxToolRoundsExceeded,
            "max_tool_rounds_exceeded",
            "maximum tool rounds exceeded",
            "max_tool_rounds",
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
            limit: (reason == crate::agent::run_safety::RunStopReason::MaxToolRoundsExceeded)
                .then_some(32),
        };
        let encoded = serde_json::to_value(metadata).expect("metadata should serialize");
        assert_eq!(encoded["bound"], bound);
        assert_eq!(
            encoded["source"],
            serde_json::Value::String("invocation".to_string())
        );
        if reason == crate::agent::run_safety::RunStopReason::MaxToolRoundsExceeded {
            assert_eq!(encoded["limit"], 32);
        } else {
            assert!(encoded.get("limit").is_none());
        }
        assert!(format!("Run stopped: {label} ({code})").contains(label));
    }
}

#[tokio::test]
async fn tool_round_stop_maps_through_json_and_ndjson_reports() {
    let provider: Arc<dyn Provider> = Arc::new(TestProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = crate::agent::Agent::new(provider.clone(), registry);
    let candidates = crate::agent::run_safety::RunSafetyCandidates {
        invocation: crate::config::RunSafetyConfig {
            max_turns: Some("1".to_string()),
            ..Default::default()
        },
        ..Default::default()
    };
    let policy = crate::agent::run_safety::resolve_run_safety(&candidates, Default::default())
        .expect("resolve run safety");
    let mut controller = crate::agent::run_safety::RunSafetyController::new(policy);
    assert!(controller.before_turn(Default::default()));
    for _ in 0..32 {
        controller.complete_tool_round();
    }
    assert!(controller.observe(Default::default()));
    agent.install_run_safety(controller);

    let report = crate::cli::commands::run_safety::report(&agent, &provider, String::new());
    let encoded = serde_json::to_value(report).expect("JSON report");
    assert_eq!(encoded["stop_reason"], "max_tool_rounds_exceeded");
    assert_eq!(encoded["safety_bound"]["bound"], "max_tool_rounds");
    assert_eq!(encoded["safety_bound"]["source"], "invocation");
    assert_eq!(encoded["safety_bound"]["limit"], 32);

    let done = crate::cli::commands::run_safety::annotate_ndjson_done(
        &agent,
        serde_json::json!({"type": "done"}),
    )
    .expect("NDJSON done report");
    assert_eq!(done["stop_reason"], "max_tool_rounds_exceeded");
    assert_eq!(done["safety_bound"]["bound"], "max_tool_rounds");
    assert_eq!(done["safety_bound"]["source"], "invocation");
    assert_eq!(done["safety_bound"]["limit"], 32);
}

#[test]
fn schema_mode_rejects_explicit_run_safety_flags_before_bridge_start() {
    let mut candidates = crate::agent::run_safety::RunSafetyCandidates {
        invocation: crate::config::RunSafetyConfig {
            max_turns: Some("1".to_string()),
            ..Default::default()
        },
        ..Default::default()
    };
    let error = reject_schema(&candidates).expect_err("schema must reject bound");
    assert!(error.to_string().contains("unsupported with --schema"));
    candidates = Default::default();
    candidates.persisted.max_turns = Some("1".to_string());
    reject_schema(&candidates).expect_err("persisted bound must reject schema");
    candidates = Default::default();
    candidates.environment.max_turns = Some("1".to_string());
    reject_schema(&candidates).expect_err("environment bound must reject schema");
    reject_schema(&Default::default()).expect("unset safety is allowed");
}
