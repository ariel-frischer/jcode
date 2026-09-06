//! Bounded retained observations. Read-only reporting never initializes a recorder.
use super::pricing;
use crate::session::memory_usage::{MAX_RECORDS, StorageWarning, UsageHistory};
use jcode_session_types::{
    lifecycle::LifecycleObservabilityStatus,
    memory_usage::{
        AttemptCoverage, MEMORY_USAGE_SCHEMA_VERSION, MemoryRequestObservation, RequestOutcome,
        RetainedWindow, SessionUsageSummary, SummaryCoverage, TokenSubtotal, TokenSubtotals,
    },
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportWarning {
    ArithmeticOverflow,
    HiddenAttemptsUnavailable,
}
#[derive(Debug, Serialize)]
pub struct UsageReport {
    pub schema_version: u16,
    pub pricing_policy: &'static str,
    pub controls: LifecycleObservabilityStatus,
    pub coverage: SummaryCoverage,
    pub storage_warnings: Vec<StorageWarning>,
    pub warnings: Vec<ReportWarning>,
    pub sessions: Vec<SessionUsageSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calls: Option<Vec<MemoryRequestObservation>>,
}

pub fn report_in_dir(
    base: &Path,
    session: Option<&str>,
    controls: LifecycleObservabilityStatus,
    calls: bool,
) -> anyhow::Result<UsageReport> {
    let history = crate::session::memory_usage::read_in_dir(base, session)?;
    Ok(summarize(history, controls, calls))
}

pub(super) fn summarize(
    mut history: UsageHistory,
    controls: LifecycleObservabilityStatus,
    include_calls: bool,
) -> UsageReport {
    let mut warnings = Vec::new();
    let mut seen = HashSet::new();
    if history.calls.len() > MAX_RECORDS {
        history.calls.truncate(MAX_RECORDS);
        warn(&mut history.warnings, StorageWarning::ScanLimit);
    }
    history.calls.retain(|call| {
        if call.validate().is_err() {
            warn(&mut history.warnings, StorageWarning::MalformedRecord);
            false
        } else if !seen.insert(call.request_id.clone()) {
            warn(&mut history.warnings, StorageWarning::DuplicateRecord);
            false
        } else {
            true
        }
    });
    history
        .calls
        .sort_by(|a, b| (a.recorded_at, &a.request_id).cmp(&(b.recorded_at, &b.request_id)));
    let initial_coverage = if !controls.enabled
        || !controls.persist_session_events
        || history.warnings.iter().any(|warning| {
            !matches!(
                warning,
                StorageWarning::RetainedWindowOnly
                    | StorageWarning::LossHistoryUnavailable
                    | StorageWarning::DuplicateRecord
            )
        }) {
        SummaryCoverage::Partial
    } else {
        SummaryCoverage::RetainedWindow
    };
    let mut sessions = BTreeMap::new();
    for call in &mut history.calls {
        // Never trust persisted estimates, which may come from an older rate table.
        call.pricing = match pricing::estimate(call) {
            Ok(cost) => cost,
            Err(_) => {
                warn(&mut warnings, ReportWarning::ArithmeticOverflow);
                pricing::unknown()
            }
        };
        let session = sessions
            .entry(call.context.session_id.clone())
            .or_insert_with(|| SessionUsageSummary {
                session_id: call.context.session_id.clone(),
                calls: 0,
                tokens: TokenSubtotals::default(),
                known_cost_subtotal_nano_usd: 0,
                unknown_cost_calls: 0,
                window: RetainedWindow {
                    first_recorded_at: Some(call.recorded_at),
                    last_recorded_at: None,
                },
                coverage: initial_coverage,
                controls,
            });
        add_call(session, call, &mut warnings);
    }
    let sessions: Vec<SessionUsageSummary> = sessions.into_values().collect();
    let coverage = if sessions.is_empty() {
        SummaryCoverage::Unavailable
    } else if sessions
        .iter()
        .any(|s| s.coverage == SummaryCoverage::Partial)
    {
        SummaryCoverage::Partial
    } else {
        initial_coverage
    };
    UsageReport {
        schema_version: MEMORY_USAGE_SCHEMA_VERSION,
        pricing_policy: "API-equivalent estimates, not actual billed charges. jcode-provider-core static standard-tier public API rates, no network refresh. Rates may be stale; actual service-tier/long-context premiums and cache-creation rates are unavailable. USD rounded half up to nano-USD once per call.",
        controls,
        coverage,
        storage_warnings: history.warnings,
        warnings,
        sessions,
        calls: include_calls.then_some(history.calls),
    }
}

fn add_call(
    session: &mut SessionUsageSummary,
    call: &MemoryRequestObservation,
    warnings: &mut Vec<ReportWarning>,
) {
    // Counts cannot overflow: each report is limited to MAX_RECORDS observations.
    session.calls += 1;
    session.window.last_recorded_at = Some(call.recorded_at);
    if call.outcome != RequestOutcome::Success {
        session.coverage = SummaryCoverage::Partial;
    }
    if call.attempt_coverage == AttemptCoverage::ProviderCallOnly {
        session.coverage = SummaryCoverage::Partial;
        warn(warnings, ReportWarning::HiddenAttemptsUnavailable);
    }
    for (subtotal, value) in [
        (&mut session.tokens.input_tokens, call.usage.input_tokens),
        (
            &mut session.tokens.cached_input_tokens,
            call.usage.cached_input_tokens,
        ),
        (
            &mut session.tokens.cache_creation_tokens,
            call.usage.cache_creation_tokens,
        ),
        (&mut session.tokens.output_tokens, call.usage.output_tokens),
        (
            &mut session.tokens.reasoning_tokens,
            call.usage.reasoning_tokens,
        ),
    ] {
        if !add_tokens(subtotal, value, warnings) {
            session.coverage = SummaryCoverage::Partial;
        }
    }
    let cost_sum = session
        .known_cost_subtotal_nano_usd
        .checked_add(call.pricing.known_subtotal_nano_usd);
    if let Some(sum) = cost_sum {
        session.known_cost_subtotal_nano_usd = sum;
    } else {
        warn(warnings, ReportWarning::ArithmeticOverflow);
    }
    if call.pricing.estimate_nano_usd.is_none() || cost_sum.is_none() {
        session.unknown_cost_calls += 1;
        session.coverage = SummaryCoverage::Partial;
    }
}

fn add_tokens(
    subtotal: &mut TokenSubtotal,
    value: Option<u64>,
    warnings: &mut Vec<ReportWarning>,
) -> bool {
    if let Some(value) = value {
        if let Some(sum) = subtotal.known_subtotal.checked_add(value) {
            subtotal.known_subtotal = sum;
            return true;
        }
        // Keep only representable known contributions, never wrap or report a
        // saturated number as an exact subtotal. The excluded call is unknown.
        warn(warnings, ReportWarning::ArithmeticOverflow);
    }
    subtotal.unknown_calls += 1;
    false
}

fn warn<T: PartialEq>(warnings: &mut Vec<T>, warning: T) {
    // Closed enums bound both warning lists independently of request count.
    if !warnings.contains(&warning) {
        warnings.push(warning);
    }
}

#[cfg(test)]
#[path = "summary_tests.rs"]
mod tests;
