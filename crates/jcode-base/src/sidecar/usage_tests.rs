use super::usage::UsageAccumulator;
use jcode_session_types::memory_usage::{RequestOutcome, TokenUsage};
use serde_json::json;

#[test]
fn openai_snapshot_subsets_and_duplicate_terminal() {
    let mut a = UsageAccumulator::default();
    let event = json!({"type":"response.completed","response":{"usage":{"input_tokens":100,"input_tokens_details":{"cached_tokens":30},"output_tokens":20,"output_tokens_details":{"reasoning_tokens":12}}}});
    a.openai(&event);
    a.openai(&event);
    assert_eq!(a.usage.input_tokens, Some(100));
    assert_eq!(a.usage.cached_input_tokens, Some(30));
    assert_eq!(a.usage.reasoning_tokens, Some(12));
    assert_eq!(a.usage.total_tokens().unwrap(), Some(120));
    assert_eq!(a.outcome, RequestOutcome::Success);
}

#[test]
fn absent_zero_and_missing_details_remain_distinct() {
    let mut a = UsageAccumulator::default();
    a.openai(&json!({"status":"completed"}));
    assert_eq!(a.usage, TokenUsage::default());
    a.openai(&json!({"usage":{"input_tokens":0,"output_tokens":0}}));
    assert_eq!(a.usage.input_tokens, Some(0));
    assert_eq!(a.usage.output_tokens, Some(0));
    assert_eq!(a.usage.cached_input_tokens, None);
    assert_eq!(a.usage.reasoning_tokens, None);
}

#[test]
fn partial_usage_survives_failure_and_incomplete() {
    for terminal in ["response.failed", "error", "response.incomplete"] {
        let mut a = UsageAccumulator::default();
        a.openai(&json!({"type":"response.created","response":{"usage":{"input_tokens":9}}}));
        a.openai(&json!({"type":terminal,"error":"SENSITIVE_PROVIDER_ERROR"}));
        assert_eq!(a.usage.input_tokens, Some(9));
        assert_eq!(a.usage.output_tokens, None);
        assert_eq!(
            a.outcome,
            if terminal == "response.incomplete" {
                RequestOutcome::Incomplete
            } else {
                RequestOutcome::Error
            }
        );
    }
}

#[test]
fn malformed_negative_overflow_and_invalid_subsets_are_unknown() {
    for bad in [json!(-1), json!("secret"), json!(1.5), json!(null)] {
        let mut a = UsageAccumulator::default();
        a.openai(&json!({"usage":{"input_tokens":bad,"output_tokens":3}}));
        assert_eq!(a.usage.input_tokens, None);
        assert_eq!(a.usage.output_tokens, Some(3));
    }
    let mut a = UsageAccumulator::default();
    a.openai(&json!({"usage":{"input_tokens":1,"input_tokens_details":{"cached_tokens":2},"output_tokens":3,"output_tokens_details":{"reasoning_tokens":4}}}));
    assert_eq!(a.usage.cached_input_tokens, None);
    assert_eq!(a.usage.reasoning_tokens, None);
    a.openai(&json!({"usage":{"input_tokens":u64::MAX,"output_tokens":1}}));
    assert!(a.usage.validate().is_ok());
    assert_eq!(a.usage.total_tokens().unwrap(), None);
}

#[test]
fn claude_normalizes_cache_once_and_stream_delta_preserves_input() {
    let usage = json!({"input_tokens":100,"cache_read_input_tokens":30,"cache_creation_input_tokens":10,"output_tokens":2});
    let mut a = UsageAccumulator::default();
    a.claude(&json!({"type":"message_start","message":{"usage":usage}}));
    a.claude(&json!({"type":"message_delta","usage":{"output_tokens":20}}));
    a.claude(&json!({"type":"message_delta","usage":{"output_tokens":20}}));
    a.claude(&json!({"type":"message_stop"}));
    assert_eq!(a.usage.input_tokens, Some(140));
    assert_eq!(a.usage.output_tokens, Some(20));
    assert_eq!(a.usage.total_tokens().unwrap(), Some(160));
    assert_eq!(a.outcome, RequestOutcome::Success);
    let mut b = UsageAccumulator::default();
    b.claude(&json!({"usage":usage,"stop_reason":"end_turn"}));
    assert_eq!(b.usage.input_tokens, Some(140));
}

#[test]
fn claude_absent_cache_and_overflow_do_not_invent_total_input() {
    let mut a = UsageAccumulator::default();
    a.claude(&json!({"usage":{"input_tokens":1,"output_tokens":2}}));
    assert_eq!(a.usage.input_tokens, None);
    a.claude(&json!({"usage":{"input_tokens":u64::MAX,"cache_read_input_tokens":1,"cache_creation_input_tokens":0}}));
    assert_eq!(a.usage.input_tokens, None);
    assert!(a.usage.validate().is_ok());
}

#[test]
fn claude_stream_token_limit_remains_incomplete_after_stop() {
    let mut a = UsageAccumulator::default();
    a.claude(&json!({"type":"message_delta", "delta":{"stop_reason":"max_tokens"}, "usage":{"output_tokens":4}}));
    a.claude(&json!({"type":"message_stop"}));
    assert_eq!(a.outcome, RequestOutcome::Incomplete);
    assert_eq!(a.usage.output_tokens, Some(4));
}

#[test]
fn invalid_generic_cache_subsets_keep_valid_totals() {
    for (read, write) in [(0, 3), (1, 1)] {
        let mut a = UsageAccumulator::default();
        a.generic(Some(1), Some(4), Some(read), Some(write), "fixture");
        assert!(a.usage.validate().is_ok());
        assert_eq!(a.usage.input_tokens, Some(1));
        assert_eq!(a.usage.output_tokens, Some(4));
        assert_eq!(a.usage.cache_creation_tokens, None);
    }
}
