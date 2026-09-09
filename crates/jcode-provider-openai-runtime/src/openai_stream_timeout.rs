//! Streaming timeout budgets for the OpenAI Responses transports.
//!
//! Both the HTTPS/SSE and websocket paths must agree on how long a silent model
//! is allowed to think. Reasoning effort drives that budget: without summaries a
//! max-effort turn emits nothing for minutes, which is indistinguishable from a
//! dead connection unless the budget scales with the requested effort.

use serde_json::Value;
use std::time::{Duration, Instant};

/// Build the Responses `reasoning` payload for a requested effort.
///
/// `summary` is mandatory. Without it the stream stays completely silent while
/// the model thinks, so high/xhigh/max efforts blow past the idle timeout and
/// get killed mid-thought, and no thinking ever renders for the user. Summary
/// deltas both keep the connection demonstrably alive and surface the thinking.
pub(crate) fn reasoning_payload(effort: &str) -> Value {
    serde_json::json!({ "effort": effort, "summary": "auto" })
}

/// Reasoning effort requested on this Responses payload, if any.
pub(crate) fn request_reasoning_effort(request: &Value) -> Option<&str> {
    request
        .get("reasoning")
        .and_then(|reasoning| reasoning.get("effort"))
        .and_then(|effort| effort.as_str())
}

/// Idle budget between HTTPS/SSE events for this request.
pub(crate) fn effective_https_idle_timeout(request: &Value) -> std::time::Duration {
    jcode_base::provider::stream_idle_timeout_for_effort(request_reasoning_effort(request))
}

/// Effective native OpenAI meaningful-progress budget for this request.
/// Disabled recovery preserves the existing transport-specific timeout paths.
pub(crate) fn effective_openai_stall_timeout_secs(request: &Value) -> Option<u64> {
    effective_openai_stall_timeout_secs_with_base(
        request,
        jcode_base::provider::openai_stall_recovery_enabled(),
        jcode_base::provider::openai_stall_timeout_secs(),
    )
}

/// Resolve a meaningful-progress budget from explicit settings. Kept separate
/// from the global config accessor so effort and enable/disable boundaries are
/// deterministic in focused tests.
pub(crate) fn effective_openai_stall_timeout_secs_with_base(
    request: &Value,
    enabled: bool,
    base_timeout_secs: u64,
) -> Option<u64> {
    enabled.then(|| {
        Duration::from_secs(base_timeout_secs.clamp(
            jcode_base::provider::OPENAI_STALL_TIMEOUT_SECS_MIN,
            jcode_base::provider::OPENAI_STALL_TIMEOUT_SECS_MAX,
        ))
        .as_secs()
        .saturating_mul(u64::from(
            jcode_base::provider::stream_idle_timeout_multiplier_for_effort(
                request_reasoning_effort(request),
            ),
        ))
    })
}

/// Bound one transport read by both its existing wire/event timeout and the
/// remaining meaningful-progress budget. Lifecycle or heartbeat frames do not
/// change `last_meaningful_progress_at`, so repeated control traffic cannot
/// postpone the absolute recovery deadline.
pub(crate) fn effective_stream_wait_timeout(
    existing_timeout: Duration,
    last_meaningful_progress_at: Instant,
    meaningful_timeout_secs: Option<u64>,
) -> Option<Duration> {
    let Some(meaningful_timeout_secs) = meaningful_timeout_secs else {
        return Some(existing_timeout);
    };
    let meaningful_timeout = Duration::from_secs(meaningful_timeout_secs);
    let remaining = meaningful_timeout.checked_sub(last_meaningful_progress_at.elapsed())?;
    Some(existing_timeout.min(remaining))
}

/// Effective websocket completion budget in seconds.
///
/// Starts from the built-in default, raised to `[provider]
/// stream_idle_timeout_secs` when the user configured a larger value so one
/// transport cannot cut off sooner than another (issue #434), then scaled by the
/// request's reasoning effort.
pub(crate) fn effective_ws_completion_timeout_secs(request: &Value) -> u64 {
    let multiplier = u64::from(
        jcode_base::provider::stream_idle_timeout_multiplier_for_effort(request_reasoning_effort(
            request,
        )),
    );
    jcode_provider_openai::websocket_health::WEBSOCKET_COMPLETION_TIMEOUT_SECS
        .max(jcode_base::provider::stream_idle_timeout().as_secs())
        .saturating_mul(multiplier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_payload_always_requests_summaries() {
        // Regression guard: dropping `summary` reintroduces silent streams that
        // trip the idle timeout on high reasoning efforts.
        for effort in ["low", "high", "xhigh", "max"] {
            let payload = reasoning_payload(effort);
            assert_eq!(payload["effort"], serde_json::json!(effort));
            assert_eq!(
                payload["summary"],
                serde_json::json!("auto"),
                "{effort} must request reasoning summaries"
            );
        }
    }

    #[test]
    fn reads_the_responses_reasoning_payload_shape() {
        assert_eq!(
            request_reasoning_effort(&serde_json::json!({"reasoning": {"effort": "high"}})),
            Some("high")
        );
        assert_eq!(request_reasoning_effort(&serde_json::json!({})), None);
        // Malformed shapes must not panic or be mistaken for an effort.
        assert_eq!(
            request_reasoning_effort(&serde_json::json!({"reasoning": "high"})),
            None
        );
        assert_eq!(
            request_reasoning_effort(&serde_json::json!({"reasoning": {"effort": 3}})),
            None
        );
    }

    #[test]
    fn ws_completion_budget_scales_with_reasoning_effort() {
        let base = effective_ws_completion_timeout_secs(&serde_json::json!({"model": "gpt-5.6"}));
        assert!(base > 0);

        // A request with an ordinary effort keeps the base budget.
        assert_eq!(
            effective_ws_completion_timeout_secs(
                &serde_json::json!({"reasoning": {"effort": "low", "summary": "auto"}})
            ),
            base
        );

        // Max effort can think silently far longer than an ordinary turn, so the
        // budget must grow rather than killing the stream mid-thought.
        let max_effort = effective_ws_completion_timeout_secs(
            &serde_json::json!({"reasoning": {"effort": "max", "summary": "auto"}}),
        );
        assert!(
            max_effort > base,
            "max effort budget {max_effort}s should exceed base {base}s"
        );
        assert!(
            effective_ws_completion_timeout_secs(
                &serde_json::json!({"reasoning": {"effort": "xhigh"}})
            ) > base
        );
    }

    #[test]
    fn https_and_ws_budgets_agree_on_effort_ordering() {
        // The two transports must not disagree about which requests get more
        // headroom, or a model that survives on HTTPS dies on websockets.
        let low = serde_json::json!({"reasoning": {"effort": "low"}});
        let max = serde_json::json!({"reasoning": {"effort": "max"}});
        assert!(effective_https_idle_timeout(&max) > effective_https_idle_timeout(&low));
        assert!(
            effective_ws_completion_timeout_secs(&max) > effective_ws_completion_timeout_secs(&low)
        );
    }

    #[test]
    fn configured_openai_stall_budget_is_enabled_and_effort_scaled() {
        assert_eq!(
            effective_openai_stall_timeout_secs_with_base(
                &serde_json::json!({
                    "reasoning": {"effort": "low"}
                }),
                true,
                300,
            ),
            Some(300)
        );
        assert_eq!(
            effective_openai_stall_timeout_secs_with_base(
                &serde_json::json!({
                    "reasoning": {"effort": "max"}
                }),
                true,
                300,
            ),
            Some(1200)
        );
    }

    #[test]
    fn disabled_openai_stall_recovery_leaves_existing_transport_budget_unchanged() {
        assert_eq!(
            effective_openai_stall_timeout_secs_with_base(
                &serde_json::json!({
                    "reasoning": {"effort": "max"}
                }),
                false,
                300,
            ),
            None
        );
    }

    #[test]
    fn meaningful_progress_wait_timeout_preserves_an_absolute_remaining_budget() {
        let recent = Instant::now() - Duration::from_secs(2);
        let remaining = effective_stream_wait_timeout(Duration::from_secs(60), recent, Some(5))
            .expect("progress budget should remain active");
        assert!(
            (Duration::from_secs(2)..=Duration::from_secs(4)).contains(&remaining),
            "repeated control frames must not reset the absolute budget: {remaining:?}"
        );
        assert!(
            effective_stream_wait_timeout(
                Duration::from_secs(60),
                Instant::now() - Duration::from_secs(6),
                Some(5),
            )
            .is_none()
        );
    }
}
