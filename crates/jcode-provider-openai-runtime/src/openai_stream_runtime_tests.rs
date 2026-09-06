use super::*;

#[test]
fn unauthorized_triggers_token_refresh() {
    assert!(should_refresh_token(StatusCode::UNAUTHORIZED, ""));
}

#[test]
fn forbidden_triggers_refresh_only_for_token_bodies() {
    assert!(should_refresh_token(
        StatusCode::FORBIDDEN,
        "access token expired"
    ));
    assert!(!should_refresh_token(
        StatusCode::FORBIDDEN,
        "region not allowed"
    ));
}

#[test]
fn refreshed_token_marker_is_retryable() {
    // After a 401/403 we force-refresh the OpenAI token and surface this
    // marker so the retry loop reconnects with the new credentials.
    assert!(is_retryable_error(
        "openai token refreshed, retrying: 401 unauthorized"
    ));
}

#[test]
fn missing_or_failed_refresh_is_not_retryable() {
    assert!(!is_retryable_error(
        "openai rejected the access token and no refresh token is available; run /login to re-authenticate: 401"
    ));
    assert!(!is_retryable_error(
        "openai token refresh failed; run /login to re-authenticate: network error"
    ));
}

#[test]
fn tls_transient_errors_are_retryable() {
    // Regression for issue #338: transient TLS faults must be retried on
    // the OpenAI path, matching every other provider. Callers pass the
    // error string already lowercased.
    assert!(is_retryable_error(
        "stream error: io error: received fatal alert: badrecordmac"
    ));
    assert!(is_retryable_error("received fatal alert: badrecordmac"));
    assert!(is_retryable_error("decryption failed or bad record mac"));
    assert!(is_retryable_error("tls handshake eof"));
    assert!(is_retryable_error("connection aborted"));
    assert!(is_retryable_error("temporary failure in name resolution"));
    assert!(is_retryable_error("no route to host"));
    assert!(is_retryable_error("network is unreachable"));
    // A send-level cause that callers now surface via the full anyhow
    // chain ({:#}) instead of the masked top-level context alone.
    assert!(is_retryable_error(
        "failed to send request to openai api: error sending request: received fatal alert: badrecordmac"
    ));
}

#[test]
fn rate_limit_is_retryable() {
    // Regression for issue #338 (gap #2): 429s should be retried, unifying
    // behavior with Anthropic/Copilot.
    assert!(is_retryable_error("429 too many requests"));
    assert!(is_retryable_error("rate limit exceeded"));
    assert!(is_retryable_error("rate_limit_exceeded"));
}

#[test]
fn auth_errors_remain_non_retryable() {
    assert!(!is_retryable_error("401 unauthorized"));
    assert!(!is_retryable_error("invalid api key"));
}
