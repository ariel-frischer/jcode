use super::*;
use crate::memory_usage::pricing::{estimate, estimate_with_rates};
use crate::session::memory_usage::tests::record;
use jcode_provider_core::{RouteCheapnessEstimate, RouteCostConfidence, RouteCostSource};
use jcode_session_types::memory_usage::{CostEstimate, PricingBasis, TokenUsage};

fn known(id: &str) -> MemoryRequestObservation {
    let mut call = record(id);
    call.model = "gpt-5.4".into();
    call.usage = TokenUsage {
        input_tokens: Some(100),
        cached_input_tokens: Some(20),
        cache_creation_tokens: Some(0),
        output_tokens: Some(10),
        reasoning_tokens: Some(8),
    };
    call
}
fn controls() -> LifecycleObservabilityStatus {
    crate::config::LifecycleObservabilityConfig::default().effective_status()
}
fn history(calls: Vec<MemoryRequestObservation>) -> UsageHistory {
    UsageHistory {
        calls,
        warnings: vec![
            StorageWarning::RetainedWindowOnly,
            StorageWarning::LossHistoryUnavailable,
        ],
    }
}
fn rates(input: u64, output: u64, cached: Option<u64>) -> RouteCheapnessEstimate {
    RouteCheapnessEstimate::metered(
        RouteCostSource::PublicApiPricing,
        RouteCostConfidence::Exact,
        input,
        output,
        cached,
        None,
    )
}

#[test]
fn public_rates_charge_cache_once_and_reasoning_only_within_output() {
    let call = known("r");
    let cost = estimate(&call).unwrap();
    // 80 * 2500 + 20 * 250 + 10 * 15000 nano-USD.
    assert_eq!(cost.estimate_nano_usd, Some(355_000));
    assert_eq!(cost.known_subtotal_nano_usd, 355_000);
    assert_eq!(cost.basis, PricingBasis::PublicApiEquivalent);
    assert_eq!(call.usage.total_tokens().unwrap(), Some(110));
    let mut claude = call;
    claude.provider = "claude".into();
    claude.model = "claude-sonnet-4-6".into();
    assert_eq!(estimate(&claude).unwrap().estimate_nano_usd, Some(396_000));
}

#[test]
fn missing_cache_details_rates_and_creation_keep_known_components() {
    let mut call = known("r");
    call.model = "gpt-5.4-pro".into(); // Existing rate contract has no cache rate.
    let cost = estimate(&call).unwrap();
    assert_eq!(cost.estimate_nano_usd, None);
    assert_eq!(cost.known_subtotal_nano_usd, 4_200_000);
    call.usage.cached_input_tokens = Some(0);
    assert_eq!(estimate(&call).unwrap().estimate_nano_usd, Some(4_800_000));
    call.model = "gpt-5.4".into();
    call.usage.cached_input_tokens = None;
    assert_eq!(estimate(&call).unwrap().known_subtotal_nano_usd, 150_000);
    assert_eq!(estimate(&call).unwrap().estimate_nano_usd, None);
    call.usage.cached_input_tokens = Some(20);
    call.usage.cache_creation_tokens = Some(5);
    assert_eq!(estimate(&call).unwrap().known_subtotal_nano_usd, 342_500);
    assert_eq!(estimate(&call).unwrap().estimate_nano_usd, None);
    call.usage = TokenUsage::default();
    assert_eq!(estimate(&call).unwrap().estimate_nano_usd, None);
}

#[test]
fn luna_unknown_provider_and_forged_persisted_price_are_not_guessed() {
    let mut luna = known("luna");
    luna.model = "gpt-5.6-luna".into();
    luna.pricing = CostEstimate {
        basis: PricingBasis::PublicApiEquivalent,
        estimate_nano_usd: Some(123),
        known_subtotal_nano_usd: 123,
    };
    assert_eq!(estimate(&luna).unwrap().basis, PricingBasis::Unknown);
    let report = summarize(history(vec![luna]), controls(), true);
    assert_eq!(report.sessions[0].unknown_cost_calls, 1);
    assert_eq!(report.calls.unwrap()[0].pricing.estimate_nano_usd, None);
    let mut generic = known("generic");
    generic.provider = "openrouter".into();
    assert_eq!(estimate(&generic).unwrap().basis, PricingBasis::Unknown);
}

#[test]
fn actual_zero_rates_and_zero_usage_are_distinct_from_absence() {
    let mut call = known("r");
    let free = rates(0, 0, Some(0));
    assert_eq!(
        estimate_with_rates(&call.usage, Some(&free))
            .unwrap()
            .estimate_nano_usd,
        Some(0)
    );
    call.usage = TokenUsage {
        input_tokens: Some(0),
        cached_input_tokens: Some(0),
        cache_creation_tokens: Some(0),
        output_tokens: Some(0),
        reasoning_tokens: None,
    };
    assert_eq!(estimate(&call).unwrap().estimate_nano_usd, Some(0));
    call.usage.output_tokens = None;
    assert_eq!(
        estimate_with_rates(&call.usage, Some(&free))
            .unwrap()
            .estimate_nano_usd,
        None
    );
    assert_eq!(
        estimate_with_rates(&call.usage, None).unwrap().basis,
        PricingBasis::Unknown
    );
}

#[test]
fn fixed_precision_rounds_once_per_call_and_checks_wide_overflow() {
    let mut usage = known("r").usage;
    usage.input_tokens = Some(1);
    usage.cached_input_tokens = Some(0);
    usage.output_tokens = Some(1);
    usage.reasoning_tokens = None;
    // Each known term is 0.4 nano-USD. Sum first, round half up to 1.
    assert_eq!(
        estimate_with_rates(&usage, Some(&rates(400, 400, None)))
            .unwrap()
            .estimate_nano_usd,
        Some(1)
    );
    assert_eq!(
        estimate_with_rates(&usage, Some(&rates(249, 250, None)))
            .unwrap()
            .estimate_nano_usd,
        Some(0)
    );
    assert_eq!(
        estimate_with_rates(&usage, Some(&rates(250, 250, None)))
            .unwrap()
            .estimate_nano_usd,
        Some(1)
    );
    usage.input_tokens = Some(u64::MAX);
    usage.output_tokens = Some(0);
    assert!(estimate_with_rates(&usage, Some(&rates(u64::MAX, 0, None))).is_err());
    usage.cached_input_tokens = Some(u64::MAX);
    usage.cache_creation_tokens = Some(1);
    assert!(estimate_with_rates(&usage, Some(&rates(1, 1, Some(1)))).is_err());
}

#[test]
fn distinct_sessions_ownerless_votes_retries_and_duplicates_reconcile() {
    let a = known("vote-a");
    let mut b = known("vote-b");
    b.recorded_at = a.recorded_at;
    b.context.session_id = Some("session-b".into());
    let mut retry = known("retry");
    retry.recorded_at = a.recorded_at;
    retry.usage.output_tokens = None;
    retry.usage.reasoning_tokens = None;
    retry.outcome = RequestOutcome::Error;
    let mut ownerless = known("ownerless");
    ownerless.recorded_at = a.recorded_at;
    ownerless.context.session_id = None;
    ownerless.attempt_coverage = AttemptCoverage::ProviderCallOnly;
    let report = summarize(
        history(vec![b, a.clone(), retry, a, ownerless]),
        controls(),
        true,
    );
    assert_eq!(report.sessions.len(), 3);
    assert_eq!(
        report
            .sessions
            .iter()
            .map(|s| s.session_id.as_deref())
            .collect::<Vec<_>>(),
        vec![None, Some("session-a"), Some("session-b")]
    );
    let session = &report.sessions[1];
    assert_eq!(session.calls, 2);
    assert_eq!(session.tokens.input_tokens.known_subtotal, 200);
    assert_eq!(session.tokens.output_tokens.known_subtotal, 10);
    assert_eq!(session.tokens.output_tokens.unknown_calls, 1);
    assert_eq!(session.unknown_cost_calls, 1);
    assert_eq!(session.known_cost_subtotal_nano_usd, 560_000);
    assert_eq!(session.coverage, SummaryCoverage::Partial);
    assert!(
        report
            .warnings
            .contains(&ReportWarning::HiddenAttemptsUnavailable)
    );
    assert!(
        report
            .storage_warnings
            .contains(&StorageWarning::DuplicateRecord)
    );
    assert_eq!(
        report
            .calls
            .unwrap()
            .iter()
            .map(|c| c.request_id.as_str())
            .collect::<Vec<_>>(),
        vec!["ownerless", "retry", "vote-a", "vote-b"]
    );
}

#[test]
fn empty_disabled_corrupt_bounded_and_overflow_reports_are_honest() {
    let empty = summarize(history(vec![]), controls(), false);
    assert!(empty.sessions.is_empty());
    assert!(empty.calls.is_none());
    assert_eq!(empty.coverage, SummaryCoverage::Unavailable);
    let disabled = crate::config::LifecycleObservabilityConfig {
        enabled: false,
        persist_session_events: true,
        emit_structured_logs: true,
    }
    .effective_status();
    let mut bad = known("bad");
    bad.model = "PRIVATE_SENTINEL unsafe".into();
    let report = summarize(history(vec![known("ok"), bad]), disabled, true);
    assert_eq!(report.sessions[0].calls, 1);
    assert_eq!(report.coverage, SummaryCoverage::Partial);
    assert!(
        !serde_json::to_string(&report)
            .unwrap()
            .contains("PRIVATE_SENTINEL")
    );
    assert!(
        report
            .storage_warnings
            .contains(&StorageWarning::MalformedRecord)
    );
    let mut huge = known("huge");
    huge.usage = TokenUsage {
        input_tokens: Some(u64::MAX),
        cached_input_tokens: Some(0),
        cache_creation_tokens: Some(0),
        output_tokens: Some(0),
        reasoning_tokens: Some(0),
    };
    let report = summarize(history(vec![huge, known("small")]), controls(), true);
    assert!(report.warnings.contains(&ReportWarning::ArithmeticOverflow));
    assert_eq!(report.sessions[0].tokens.input_tokens.unknown_calls, 1);
    assert_eq!(report.sessions[0].unknown_cost_calls, 1);
    let many = (0..MAX_RECORDS + 1)
        .map(|i| known(&format!("r{i}")))
        .collect();
    let report = summarize(history(many), controls(), true);
    assert_eq!(report.calls.unwrap().len(), MAX_RECORDS);
    assert!(report.storage_warnings.contains(&StorageWarning::ScanLimit));
}
