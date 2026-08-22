//! Optional safety bounds for unattended Agent runs.
//!
//! This module intentionally owns only raw-source resolution and per-run
//! enforcement state. The CLI supplies invocation/config candidates; Agent
//! loops consult the controller at provider and Registry boundaries.

use chrono::{DateTime, Utc};
use jcode_config_types::RunSafetyConfig;
use serde::Serialize;
use std::num::NonZeroU64;
use std::time::{Duration, Instant};

use crate::logging;
use crate::message::{ContentBlock, Role, ToolCall};
use crate::protocol::ServerEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunSafetyBound {
    MaxTurns,
    MaxToolSteps,
    TokenBudget,
    Deadline,
}

impl RunSafetyBound {
    pub const fn name(self) -> &'static str {
        match self {
            Self::MaxTurns => "max_turns",
            Self::MaxToolSteps => "max_tool_steps",
            Self::TokenBudget => "token_budget",
            Self::Deadline => "deadline",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RunSafetySource {
    Invocation,
    Environment,
    Persisted,
    Unset,
}

impl RunSafetySource {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Invocation => "invocation",
            Self::Environment => "environment",
            Self::Persisted => "persisted configuration",
            Self::Unset => "unset",
        }
    }
}

/// Raw candidates for one invocation. Each field is resolved independently.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunSafetyCandidates {
    pub invocation: RunSafetyConfig,
    pub environment: RunSafetyConfig,
    pub persisted: RunSafetyConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveRunSafetyPolicy {
    pub max_turns: Option<NonZeroU64>,
    pub max_tool_steps: Option<NonZeroU64>,
    pub token_budget: Option<NonZeroU64>,
    pub deadline: Option<Instant>,
    pub sources: RunSafetySources,
    pub usage_baseline: crate::protocol::TokenUsageTotals,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RunSafetySources {
    pub max_turns: RunSafetySource,
    pub max_tool_steps: RunSafetySource,
    pub token_budget: RunSafetySource,
    pub deadline: RunSafetySource,
}

impl Default for RunSafetySources {
    fn default() -> Self {
        Self {
            max_turns: RunSafetySource::Unset,
            max_tool_steps: RunSafetySource::Unset,
            token_budget: RunSafetySource::Unset,
            deadline: RunSafetySource::Unset,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSafetyError {
    pub bound: RunSafetyBound,
    pub source: RunSafetySource,
    pub value: String,
    pub correction: &'static str,
}

impl std::fmt::Display for RunSafetyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid {} safety bound from {} value {:?}: {}; set a valid value and retry",
            self.bound.name(),
            self.source.name(),
            self.value,
            self.correction
        )
    }
}

impl std::error::Error for RunSafetyError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStopReason {
    MaxTurnsExceeded,
    MaxToolStepsExceeded,
    TokenBudgetExceeded,
    DeadlineExceeded,
}

impl RunStopReason {
    pub const fn bound(self) -> RunSafetyBound {
        match self {
            Self::MaxTurnsExceeded => RunSafetyBound::MaxTurns,
            Self::MaxToolStepsExceeded => RunSafetyBound::MaxToolSteps,
            Self::TokenBudgetExceeded => RunSafetyBound::TokenBudget,
            Self::DeadlineExceeded => RunSafetyBound::Deadline,
        }
    }

    pub const fn code(self) -> &'static str {
        match self {
            Self::MaxTurnsExceeded => "max_turns_exceeded",
            Self::MaxToolStepsExceeded => "max_tool_steps_exceeded",
            Self::TokenBudgetExceeded => "token_budget_exceeded",
            Self::DeadlineExceeded => "deadline_exceeded",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::MaxTurnsExceeded => "maximum turns exceeded",
            Self::MaxToolStepsExceeded => "maximum tool steps exceeded",
            Self::TokenBudgetExceeded => "token budget exceeded",
            Self::DeadlineExceeded => "deadline exceeded",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RunSafetyStopMetadata {
    pub bound: RunSafetyBound,
    pub source: RunSafetySource,
}

fn source_value(config: &RunSafetyConfig, bound: RunSafetyBound) -> Option<&str> {
    match bound {
        RunSafetyBound::MaxTurns => config.max_turns.as_deref(),
        RunSafetyBound::MaxToolSteps => config.max_tool_steps.as_deref(),
        RunSafetyBound::TokenBudget => config.token_budget.as_deref(),
        RunSafetyBound::Deadline => config.deadline.as_deref(),
    }
}

fn resolve_raw(
    candidates: &RunSafetyCandidates,
    bound: RunSafetyBound,
) -> (Option<&str>, RunSafetySource) {
    for (source, config) in [
        (RunSafetySource::Invocation, &candidates.invocation),
        (RunSafetySource::Environment, &candidates.environment),
        (RunSafetySource::Persisted, &candidates.persisted),
    ] {
        if let Some(value) = source_value(config, bound) {
            return (Some(value), source);
        }
    }
    (None, RunSafetySource::Unset)
}

fn parse_positive(
    bound: RunSafetyBound,
    source: RunSafetySource,
    value: &str,
) -> Result<NonZeroU64, RunSafetyError> {
    let correction = "use a positive decimal whole number (u64 greater than zero)";
    let trimmed = value.trim();
    let parsed = match trimmed.parse::<u64>() {
        Ok(value) => NonZeroU64::new(value),
        Err(_) => None,
    };
    parsed.ok_or_else(|| RunSafetyError {
        bound,
        source,
        value: value.to_string(),
        correction,
    })
}

fn parse_deadline(source: RunSafetySource, value: &str) -> Result<Instant, RunSafetyError> {
    let correction = "use a future RFC3339 timestamp with an explicit UTC offset";
    let parsed = match DateTime::parse_from_rfc3339(value.trim()) {
        Ok(parsed) => parsed,
        Err(_) => {
            return Err(RunSafetyError {
                bound: RunSafetyBound::Deadline,
                source,
                value: value.to_string(),
                correction,
            });
        }
    };
    let parsed = parsed.with_timezone(&Utc);
    let now = Utc::now();
    let remaining = match (parsed - now).to_std() {
        Ok(remaining) => remaining,
        Err(_) => {
            return Err(RunSafetyError {
                bound: RunSafetyBound::Deadline,
                source,
                value: value.to_string(),
                correction,
            });
        }
    };
    Instant::now()
        .checked_add(remaining)
        .ok_or_else(|| RunSafetyError {
            bound: RunSafetyBound::Deadline,
            source,
            value: value.to_string(),
            correction,
        })
}

pub fn resolve_run_safety(
    candidates: &RunSafetyCandidates,
    usage_baseline: crate::protocol::TokenUsageTotals,
) -> Result<EffectiveRunSafetyPolicy, RunSafetyError> {
    let (max_turns, max_turns_source) = resolve_raw(candidates, RunSafetyBound::MaxTurns);
    let (max_tool_steps, max_tool_steps_source) =
        resolve_raw(candidates, RunSafetyBound::MaxToolSteps);
    let (token_budget, token_budget_source) = resolve_raw(candidates, RunSafetyBound::TokenBudget);
    let (deadline, deadline_source) = resolve_raw(candidates, RunSafetyBound::Deadline);

    Ok(EffectiveRunSafetyPolicy {
        max_turns: max_turns
            .map(|value| parse_positive(RunSafetyBound::MaxTurns, max_turns_source, value))
            .transpose()?,
        max_tool_steps: max_tool_steps
            .map(|value| parse_positive(RunSafetyBound::MaxToolSteps, max_tool_steps_source, value))
            .transpose()?,
        token_budget: token_budget
            .map(|value| parse_positive(RunSafetyBound::TokenBudget, token_budget_source, value))
            .transpose()?,
        deadline: deadline
            .map(|value| parse_deadline(deadline_source, value))
            .transpose()?,
        sources: RunSafetySources {
            max_turns: max_turns_source,
            max_tool_steps: max_tool_steps_source,
            token_budget: token_budget_source,
            deadline: deadline_source,
        },
        usage_baseline,
    })
}

fn usage_delta(
    baseline: crate::protocol::TokenUsageTotals,
    current: crate::protocol::TokenUsageTotals,
) -> u64 {
    current
        .input_tokens
        .saturating_sub(baseline.input_tokens)
        .saturating_add(current.output_tokens.saturating_sub(baseline.output_tokens))
        .saturating_add(
            current
                .cache_read_input_tokens
                .saturating_sub(baseline.cache_read_input_tokens),
        )
        .saturating_add(
            current
                .cache_creation_input_tokens
                .saturating_sub(baseline.cache_creation_input_tokens),
        )
}

/// Mutable per-invocation counters shared by Agent execution paths.
#[derive(Debug, Clone)]
pub struct RunSafetyController {
    policy: EffectiveRunSafetyPolicy,
    completed_turns: u64,
    tool_steps: u64,
    observed_usage: u64,
    stop_reason: Option<RunStopReason>,
}

impl RunSafetyController {
    pub fn new(policy: EffectiveRunSafetyPolicy) -> Self {
        Self {
            policy,
            completed_turns: 0,
            tool_steps: 0,
            observed_usage: 0,
            stop_reason: None,
        }
    }

    pub fn policy(&self) -> &EffectiveRunSafetyPolicy {
        &self.policy
    }

    pub fn stop_reason(&self) -> Option<RunStopReason> {
        self.stop_reason
    }

    pub fn stop_metadata(&self) -> Option<RunSafetyStopMetadata> {
        let reason = self.stop_reason?;
        let source = match reason.bound() {
            RunSafetyBound::MaxTurns => self.policy.sources.max_turns,
            RunSafetyBound::MaxToolSteps => self.policy.sources.max_tool_steps,
            RunSafetyBound::TokenBudget => self.policy.sources.token_budget,
            RunSafetyBound::Deadline => self.policy.sources.deadline,
        };
        Some(RunSafetyStopMetadata {
            bound: reason.bound(),
            source,
        })
    }

    pub fn completed_turns(&self) -> u64 {
        self.completed_turns
    }

    pub fn tool_steps(&self) -> u64 {
        self.tool_steps
    }

    pub fn observed_usage(&self) -> u64 {
        self.observed_usage
    }

    pub fn deadline_remaining(&self) -> Option<Duration> {
        self.policy
            .deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    fn select(&mut self, reason: RunStopReason) {
        if self.stop_reason.is_none() {
            self.stop_reason = Some(reason);
        }
    }

    /// Observe hard ceilings before provider work or after a stream event.
    pub fn observe(&mut self, current_usage: crate::protocol::TokenUsageTotals) -> bool {
        self.observed_usage = usage_delta(self.policy.usage_baseline, current_usage);
        if self
            .policy
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.select(RunStopReason::DeadlineExceeded);
        } else if self
            .policy
            .token_budget
            .is_some_and(|budget| self.observed_usage >= budget.get())
        {
            self.select(RunStopReason::TokenBudgetExceeded);
        } else if self
            .policy
            .max_tool_steps
            .is_some_and(|limit| self.tool_steps >= limit.get())
        {
            self.select(RunStopReason::MaxToolStepsExceeded);
        }
        self.stop_reason.is_some()
    }

    /// Authorize the next Registry execution.
    pub fn before_tool_step(&mut self) -> bool {
        if self.stop_reason.is_some() {
            return false;
        }
        if self
            .policy
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.select(RunStopReason::DeadlineExceeded);
        } else if self
            .policy
            .max_tool_steps
            .is_some_and(|limit| self.tool_steps >= limit.get())
        {
            self.select(RunStopReason::MaxToolStepsExceeded);
        } else {
            self.tool_steps = self.tool_steps.saturating_add(1);
        }
        self.stop_reason.is_none()
    }

    pub fn complete_turn(&mut self) {
        self.completed_turns = self.completed_turns.saturating_add(1);
        if self.stop_reason.is_none()
            && self
                .policy
                .max_turns
                .is_some_and(|limit| self.completed_turns >= limit.get())
        {
            self.select(RunStopReason::MaxTurnsExceeded);
        }
    }

    pub fn before_turn(&mut self, current_usage: crate::protocol::TokenUsageTotals) -> bool {
        if self.observe(current_usage) {
            return false;
        }
        if self
            .policy
            .max_turns
            .is_some_and(|limit| self.completed_turns >= limit.get())
        {
            self.select(RunStopReason::MaxTurnsExceeded);
        }
        self.stop_reason.is_none()
    }
}

pub(crate) const RUN_SAFETY_SKIPPED_TOOL_RESULT: &str = "[Skipped: run safety bound reached]";

impl crate::agent::Agent {
    pub(super) fn record_run_safety_skipped_tool_result(&mut self, tool_call: &ToolCall) {
        self.add_message(
            Role::User,
            vec![ContentBlock::ToolResult {
                tool_use_id: tool_call.id.clone(),
                content: RUN_SAFETY_SKIPPED_TOOL_RESULT.to_string(),
                is_error: Some(true),
            }],
        );
    }

    pub(super) fn record_run_safety_skipped_tool_results(&mut self, tool_calls: &[ToolCall]) {
        for tool_call in tool_calls {
            self.record_run_safety_skipped_tool_result(tool_call);
        }
    }

    pub(super) fn run_safety_skip_tool_calls(&mut self, tool_calls: &[ToolCall]) -> bool {
        self.record_run_safety_skipped_tool_results(tool_calls);
        if !tool_calls.is_empty() {
            self.record_block_lifecycle(
                crate::session::lifecycle_types::LifecycleDecisionType::Suppressed,
                crate::session::lifecycle_types::LifecycleSemanticReason::Policy,
                Some(crate::session::lifecycle_types::LifecycleSuppressionReason::PolicyDenied),
            );
        }
        !tool_calls.is_empty()
    }

    pub(super) fn emit_run_safety_skipped_tool_results(
        &mut self,
        event_tx: &tokio::sync::mpsc::UnboundedSender<ServerEvent>,
        tool_calls: &[ToolCall],
    ) -> bool {
        for tool_call in tool_calls {
            let content = RUN_SAFETY_SKIPPED_TOOL_RESULT.to_string();
            if let Err(error) = event_tx.send(ServerEvent::ToolDone {
                id: tool_call.id.clone(),
                name: tool_call.name.clone(),
                output: content.clone(),
                error: Some(content),
            }) {
                logging::warn(&format!(
                    "run-safety ToolDone notification dropped: {error:?}"
                ));
            }
            self.record_run_safety_skipped_tool_result(tool_call);
        }
        if !tool_calls.is_empty() {
            self.record_block_lifecycle(
                crate::session::lifecycle_types::LifecycleDecisionType::Suppressed,
                crate::session::lifecycle_types::LifecycleSemanticReason::Policy,
                Some(crate::session::lifecycle_types::LifecycleSuppressionReason::PolicyDenied),
            );
        }
        !tool_calls.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates() -> RunSafetyCandidates {
        RunSafetyCandidates {
            invocation: RunSafetyConfig::default(),
            environment: RunSafetyConfig::default(),
            persisted: RunSafetyConfig::default(),
        }
    }

    #[test]
    fn precedence_is_independent_per_bound() {
        let mut candidates = candidates();
        candidates.persisted.max_turns = Some("9".into());
        candidates.environment.max_turns = Some("7".into());
        candidates.invocation.max_turns = Some("3".into());
        candidates.persisted.max_tool_steps = Some("8".into());
        candidates.environment.token_budget = Some("100".into());
        let policy = resolve_run_safety(&candidates, Default::default()).expect("valid policy");
        assert_eq!(policy.max_turns.map(NonZeroU64::get), Some(3));
        assert_eq!(policy.max_tool_steps.map(NonZeroU64::get), Some(8));
        assert_eq!(policy.token_budget.map(NonZeroU64::get), Some(100));
        assert!(policy.deadline.is_none());
        assert_eq!(policy.sources.max_turns, RunSafetySource::Invocation);
        assert_eq!(policy.sources.max_tool_steps, RunSafetySource::Persisted);
        assert_eq!(policy.sources.deadline, RunSafetySource::Unset);
    }

    #[test]
    fn invalid_higher_precedence_value_does_not_fall_through() {
        let mut candidates = candidates();
        candidates.persisted.max_turns = Some("9".into());
        candidates.environment.max_turns = Some("0".into());
        let error = resolve_run_safety(&candidates, Default::default()).expect_err("must reject");
        assert_eq!(error.bound, RunSafetyBound::MaxTurns);
        assert_eq!(error.source, RunSafetySource::Environment);
        assert_eq!(error.value, "0");
    }

    #[test]
    fn invalid_values_report_bound_source_value_and_correction() {
        for value in ["", "  ", "0", "-1", "not-a-number", "18446744073709551616"] {
            let mut candidates = candidates();
            candidates.invocation.max_tool_steps = Some(value.to_string());
            let error = resolve_run_safety(&candidates, Default::default())
                .expect_err("invalid tool-step value must fail");
            assert_eq!(error.bound, RunSafetyBound::MaxToolSteps);
            assert_eq!(error.source, RunSafetySource::Invocation);
            assert_eq!(error.value, value);
            assert!(error.correction.contains("positive"));
            assert!(error.to_string().contains("max_tool_steps"));
        }

        let mut candidates = candidates();
        candidates.invocation.deadline = Some("2020-01-01".to_string());
        let error = resolve_run_safety(&candidates, Default::default())
            .expect_err("deadline without offset must fail");
        assert_eq!(error.bound, RunSafetyBound::Deadline);
        assert!(error.correction.contains("RFC3339"));
    }

    #[test]
    fn controller_uses_saturating_usage_delta_and_deterministic_priority() {
        let mut candidates = candidates();
        candidates.invocation.token_budget = Some("10".into());
        candidates.invocation.max_tool_steps = Some("2".into());
        let baseline = crate::protocol::TokenUsageTotals {
            input_tokens: 100,
            ..Default::default()
        };
        let policy = resolve_run_safety(&candidates, baseline).expect("valid policy");
        let mut controller = RunSafetyController::new(policy);
        assert!(!controller.observe(crate::protocol::TokenUsageTotals {
            input_tokens: 101,
            output_tokens: 8,
            ..Default::default()
        }));
        assert!(controller.observe(crate::protocol::TokenUsageTotals {
            input_tokens: 101,
            output_tokens: 10,
            ..Default::default()
        }));
        assert_eq!(
            controller.stop_reason(),
            Some(RunStopReason::TokenBudgetExceeded)
        );

        let policy = EffectiveRunSafetyPolicy {
            max_turns: NonZeroU64::new(1),
            max_tool_steps: NonZeroU64::new(1),
            token_budget: NonZeroU64::new(1),
            deadline: Some(Instant::now()),
            sources: RunSafetySources::default(),
            usage_baseline: Default::default(),
        };
        let mut controller = RunSafetyController::new(policy);
        assert!(controller.observe(Default::default()));
        assert_eq!(
            controller.stop_reason(),
            Some(RunStopReason::DeadlineExceeded)
        );
    }
}
