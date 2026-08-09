use anyhow::{Result, bail};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub(super) struct RunCommandReport {
    pub(super) session_id: String,
    pub(super) provider: String,
    pub(super) model: String,
    pub(super) text: String,
    pub(super) usage: crate::agent::TokenUsage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) observed_usage: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) safety_bound: Option<crate::agent::run_safety::RunSafetyStopMetadata>,
}

pub(super) fn load_candidates(
    invocation: crate::config::RunSafetyConfig,
) -> Result<crate::agent::run_safety::RunSafetyCandidates> {
    let (persisted, environment) = crate::config::Config::run_safety_sources()?;
    let candidates = crate::agent::run_safety::RunSafetyCandidates {
        invocation,
        environment,
        persisted,
    };
    crate::agent::run_safety::resolve_run_safety(&candidates, Default::default())?;
    Ok(candidates)
}

pub(super) fn reject_schema(
    candidates: &crate::agent::run_safety::RunSafetyCandidates,
) -> Result<()> {
    let has_bound = [
        &candidates.invocation,
        &candidates.environment,
        &candidates.persisted,
    ]
    .into_iter()
    .any(|safety| {
        safety.max_turns.is_some()
            || safety.max_tool_steps.is_some()
            || safety.token_budget.is_some()
            || safety.deadline.is_some()
    });
    if has_bound {
        bail!(
            "run safety options are unsupported with --schema; use ordinary --json, --ndjson, or plain output"
        );
    }
    Ok(())
}

pub(super) fn install(
    agent: &mut crate::agent::Agent,
    candidates: crate::agent::run_safety::RunSafetyCandidates,
) -> Result<()> {
    let policy =
        crate::agent::run_safety::resolve_run_safety(&candidates, agent.token_usage_totals())?;
    if policy.max_turns.is_some()
        || policy.max_tool_steps.is_some()
        || policy.token_budget.is_some()
        || policy.deadline.is_some()
    {
        agent.install_run_safety(crate::agent::run_safety::RunSafetyController::new(policy));
    }
    Ok(())
}

pub(super) fn report(
    agent: &crate::agent::Agent,
    provider: &std::sync::Arc<dyn crate::provider::Provider>,
    text: String,
) -> RunCommandReport {
    RunCommandReport {
        session_id: agent.session_id().to_string(),
        provider: provider.name().to_string(),
        model: provider.model(),
        text,
        usage: agent.last_usage().clone(),
        stop_reason: agent
            .run_safety_stop_reason()
            .map(|reason| reason.code().to_string()),
        outcome: agent
            .run_safety_stop_reason()
            .map(|_| "bounded_stop".to_string()),
        observed_usage: agent
            .run_safety_stop_reason()
            .and_then(|_| agent.run_safety_controller())
            .map(|controller| controller.observed_usage()),
        safety_bound: agent
            .run_safety_controller()
            .and_then(|controller| controller.stop_metadata()),
    }
}

pub(super) fn complete_turn_and_should_stop(
    agent: &mut crate::agent::Agent,
    turns_completed: &mut usize,
) -> bool {
    agent.run_safety_complete_turn();
    *turns_completed += 1;
    agent.run_safety_stop_reason().is_some()
}

pub(super) fn print_plain_stop(agent: &crate::agent::Agent) {
    if let Some(reason) = agent.run_safety_stop_reason() {
        println!("Run stopped: {} ({})", reason.label(), reason.code());
    }
}

pub(super) fn annotate_ndjson_done(
    agent: &crate::agent::Agent,
    mut done: serde_json::Value,
) -> Result<serde_json::Value> {
    if let Some(reason) = agent.run_safety_stop_reason()
        && let Some(object) = done.as_object_mut()
    {
        object.insert(
            "stop_reason".to_string(),
            serde_json::Value::String(reason.code().to_string()),
        );
        object.insert(
            "outcome".to_string(),
            serde_json::Value::String("bounded_stop".to_string()),
        );
        if let Some(usage) = agent.run_safety_controller() {
            object.insert(
                "observed_usage".to_string(),
                serde_json::Value::from(usage.observed_usage()),
            );
            if let Some(metadata) = usage.stop_metadata() {
                object.insert("safety_bound".to_string(), serde_json::to_value(metadata)?);
            }
        }
    }
    Ok(done)
}
