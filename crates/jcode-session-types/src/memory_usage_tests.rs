use super::*;
use serde_json::{Value, json};

fn record() -> MemoryRequestObservation {
    serde_json::from_value(json!({
        "schema_version": 1, "request_id": "request-1",
        "context": {"session_id": null, "operation_id": "operation-1", "operation_kind": "rerank"},
        "recorded_at": "2026-09-05T22:00:00Z", "provider": "openai",
        "model": "gpt-5.6-luna", "effort": "xhigh", "auth_class": "oauth",
        "outcome": "incomplete", "usage": {}, "attempt_coverage": "physical_attempt",
        "pricing": {"basis": "unknown", "estimate_nano_usd": null,
                    "known_subtotal_nano_usd": 0}
    }))
    .unwrap()
}

#[test]
fn versioned_record_round_trip_preserves_null_owner_and_effective_metadata() {
    let observation = record();
    assert_eq!(observation.validate(), Ok(()));
    let value = serde_json::to_value(&observation).unwrap();
    assert_eq!(value["schema_version"], MEMORY_USAGE_SCHEMA_VERSION);
    assert_eq!(value["context"]["session_id"], Value::Null);
    assert_eq!(value["model"], "gpt-5.6-luna");
    assert_eq!(value["effort"], "xhigh");
    assert_eq!(
        serde_json::from_value::<MemoryRequestObservation>(value).unwrap(),
        observation
    );
}

#[test]
fn absent_usage_is_not_reported_zero() {
    let missing = TokenUsage::default();
    let zero: TokenUsage =
        serde_json::from_value(json!({"input_tokens":0,"output_tokens":0})).unwrap();
    assert_ne!(missing, zero);
    assert_eq!(missing.total_tokens(), Ok(None));
    assert_eq!(zero.total_tokens(), Ok(Some(0)));
    assert_eq!(
        serde_json::to_value(missing).unwrap()["input_tokens"],
        Value::Null
    );
}

#[test]
fn reasoning_and_cache_are_subsets_not_additional_total_tokens() {
    let usage: TokenUsage = serde_json::from_value(json!({
        "input_tokens":100,"cached_input_tokens":40,"cache_creation_tokens":10,
        "output_tokens":80,"reasoning_tokens":60
    }))
    .unwrap();
    assert_eq!(usage.validate(), Ok(()));
    assert_eq!(usage.total_tokens(), Ok(Some(180)));
}

#[test]
fn invalid_usage_is_rejected_without_coercion() {
    for value in [
        json!({"reasoning_tokens":3,"output_tokens":2}),
        json!({"cached_input_tokens":3,"input_tokens":2}),
        json!({"cached_input_tokens":2,"cache_creation_tokens":2,"input_tokens":3}),
        json!({"cached_input_tokens":u64::MAX,"cache_creation_tokens":1}),
        json!({"input_tokens":u64::MAX,"output_tokens":1}),
    ] {
        let usage: TokenUsage = serde_json::from_value(value).unwrap();
        assert!(usage.validate().is_err());
        assert!(usage.total_tokens().is_err());
        let mut observation = record();
        observation.usage = usage;
        assert!(observation.validate().is_err());
    }
    for raw in [
        "{\"input_tokens\":-1}",
        "{\"input_tokens\":18446744073709551616}",
        "{\"input_tokens\":1.5}",
    ] {
        assert!(serde_json::from_str::<TokenUsage>(raw).is_err());
    }
}

#[test]
fn missing_subset_parent_remains_unknown_not_zero() {
    let usage: TokenUsage = serde_json::from_value(json!({"reasoning_tokens":8})).unwrap();
    assert_eq!(usage.validate(), Ok(()));
    assert_eq!(usage.output_tokens, None);
    assert_eq!(usage.total_tokens(), Ok(None));
}

#[test]
fn identifiers_are_bounded_and_errors_do_not_echo_input() {
    for invalid in [
        "".to_string(),
        "../secret".into(),
        "secret\ntext".into(),
        "x".repeat(129),
        "secret@example.com".into(),
    ] {
        let mut observation = record();
        observation.context.session_id = Some(invalid.clone());
        let error = observation.validate().unwrap_err().to_string();
        assert_eq!(error, "invalid accounting identifier");
        observation.context.session_id = None;
        observation.request_id = invalid;
        assert!(observation.validate().is_err());
    }
    let mut observation = record();
    observation.context.session_id = Some("s".repeat(128));
    assert!(observation.validate().is_ok());
    observation.model = "provider/model-v1:latest".into();
    assert!(observation.validate().is_ok());
    observation.model = "https://host/secret?key=sentinel".into();
    assert!(observation.validate().is_err());
}

#[test]
fn closed_vocabularies_and_allowlisted_records_reject_extra_content() {
    let value = serde_json::to_value(record()).unwrap();
    for key in ["prompt", "text", "raw_error", "credentials", "account_id"] {
        let mut injected = value.clone();
        injected[key] = json!("SENSITIVE_SENTINEL");
        assert!(serde_json::from_value::<MemoryRequestObservation>(injected).is_err());
    }
    for key in ["outcome", "auth_class", "attempt_coverage", "effort"] {
        let mut injected = value.clone();
        injected[key] = json!("SENSITIVE_SENTINEL");
        assert!(serde_json::from_value::<MemoryRequestObservation>(injected).is_err());
    }
    let mut injected = value;
    injected["context"]["operation_kind"] = json!("SENSITIVE_SENTINEL");
    assert!(serde_json::from_value::<MemoryRequestObservation>(injected).is_err());
}

#[test]
fn unknown_schema_and_inconsistent_cost_are_invalid() {
    let mut observation = record();
    observation.schema_version = 2;
    assert_eq!(
        observation.validate(),
        Err(ValidationError::UnsupportedSchema)
    );
    observation.schema_version = 1;
    observation.pricing.estimate_nano_usd = Some(0);
    assert_eq!(
        observation.validate(),
        Err(ValidationError::InvalidEstimate)
    );
    observation.pricing.basis = PricingBasis::PublicApiEquivalent;
    assert_eq!(observation.validate(), Ok(()));
    observation.pricing.known_subtotal_nano_usd = 1;
    assert_eq!(
        observation.validate(),
        Err(ValidationError::InvalidEstimate)
    );
}

#[test]
fn session_summary_retains_unknown_counts_and_controls() {
    let value = json!({
        "session_id":null,"calls":2,
        "tokens":{"input_tokens":{"known_subtotal":5,"unknown_calls":1},
          "cached_input_tokens":{"known_subtotal":0,"unknown_calls":2},
          "cache_creation_tokens":{"known_subtotal":0,"unknown_calls":2},
          "output_tokens":{"known_subtotal":0,"unknown_calls":2},
          "reasoning_tokens":{"known_subtotal":0,"unknown_calls":2}},
        "known_cost_subtotal_nano_usd":0,"unknown_cost_calls":2,
        "window":{"first_recorded_at":null,"last_recorded_at":null},
        "coverage":"retained_window",
        "controls":{"enabled":false,"persist_session_events":false,"emit_structured_logs":false}
    });
    let summary: SessionUsageSummary = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(summary.tokens.input_tokens.unknown_calls, 1);
    assert_eq!(serde_json::to_value(summary).unwrap(), value);
}
