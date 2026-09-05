#[test]
fn auth_test_retryable_error_detection_rejects_hard_usage_limit_exhaustion() {
    // Regression for #1148: the text contains "rate limit" but the quota
    // resets in weeks, so retrying is pointless.
    let err = anyhow::anyhow!(
        "Rate limited: The usage limit has been reached. Plan: free. Resets in 28d 19h 47m (2026-09-30 18:44 UTC)."
    );
    assert!(!auth_test_error_is_retryable(&err));
    let err = anyhow::anyhow!("OpenAI request failed (HTTP 429): insufficient_quota");
    assert!(!auth_test_error_is_retryable(&err));
}
