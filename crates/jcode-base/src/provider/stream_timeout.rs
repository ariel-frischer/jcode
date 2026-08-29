//! Provider-agnostic streaming idle-timeout budgets.
//!
//! Every streaming provider path shares these helpers so a slow reasoning model
//! cannot trip a premature timeout on one transport but not another (issue
//! #434). The base budget comes from config; reasoning effort scales it, because
//! high-effort turns can think silently for many minutes before emitting a
//! single event and would otherwise look like a dead connection.

use std::time::Duration;

/// Largest multiplier [`stream_idle_timeout_multiplier_for_effort`] can return.
///
/// Clients that guard against stalls without knowing the request's reasoning
/// effort must budget against this so they never cancel a stream the provider
/// would still happily accept.
pub const MAX_STREAM_IDLE_TIMEOUT_MULTIPLIER: u32 = 4;

/// Base streaming idle timeout: max time to wait between streamed chunks/events
/// before treating the connection as dead.
///
/// Resolved from `[provider] stream_idle_timeout_secs` /
/// `JCODE_STREAM_IDLE_TIMEOUT_SECS` (default 180).
pub fn stream_idle_timeout() -> Duration {
    let secs = crate::config::config()
        .provider
        .stream_idle_timeout_secs
        .max(1);
    Duration::from_secs(secs)
}

/// Multiplier applied to the base idle timeout for a given reasoning effort.
///
/// The base budget is tuned for ordinary turns that emit tokens promptly. High
/// efforts are silent for far longer, so they need proportionally more headroom.
pub fn stream_idle_timeout_multiplier_for_effort(effort: Option<&str>) -> u32 {
    match effort
        .map(|effort| effort.trim().to_ascii_lowercase())
        .as_deref()
        .unwrap_or("")
    {
        "high" => 2,
        "xhigh" => 3,
        // `swarm`/`swarm-deep` are Jcode UI sentinels that resolve to the top
        // wire effort upstream, so budget them like `max`.
        "max" | "swarm" | "swarm-deep" => MAX_STREAM_IDLE_TIMEOUT_MULTIPLIER,
        _ => 1,
    }
}

/// [`stream_idle_timeout`] scaled for the request's reasoning effort.
pub fn stream_idle_timeout_for_effort(effort: Option<&str>) -> Duration {
    stream_idle_timeout() * stream_idle_timeout_multiplier_for_effort(effort)
}

/// Provider transport budget that a client-side stall watchdog must outlast.
///
/// OpenAI's persistent websocket transport has a built-in completion timeout
/// that can exceed the generic stream-idle setting. A client watchdog derived
/// only from `stream_idle_timeout_secs` would therefore cancel a valid server
/// request before the owning transport times out (180s + grace versus the
/// websocket's 300s default). Keep the transport floor in the provider-owning
/// layer and scale it by the same reasoning-effort multiplier as the request.
pub fn stream_watchdog_timeout_for_provider(
    provider: Option<&str>,
    effort: Option<&str>,
) -> Duration {
    stream_watchdog_timeout_for_provider_with_base(provider, effort, stream_idle_timeout())
}

fn stream_watchdog_timeout_for_provider_with_base(
    provider: Option<&str>,
    effort: Option<&str>,
    base: Duration,
) -> Duration {
    let provider = provider.unwrap_or_default().trim().to_ascii_lowercase();
    let transport_base = if provider.contains("openai") {
        base.max(Duration::from_secs(
            jcode_provider_openai::websocket_health::WEBSOCKET_COMPLETION_TIMEOUT_SECS,
        ))
    } else {
        base
    };
    transport_base * stream_idle_timeout_multiplier_for_effort(effort)
}

/// [`stream_idle_timeout`] scaled by the maximum effort multiplier.
///
/// Use this where the reasoning effort is unknown but the budget must still
/// outlast any legitimate provider-side wait.
pub fn max_stream_idle_timeout() -> Duration {
    stream_idle_timeout() * MAX_STREAM_IDLE_TIMEOUT_MULTIPLIER
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiplier_scales_with_reasoning_effort() {
        // Ordinary efforts keep the base budget; the model emits tokens promptly.
        for effort in [
            None,
            Some("none"),
            Some("minimal"),
            Some("low"),
            Some("medium"),
        ] {
            assert_eq!(
                stream_idle_timeout_multiplier_for_effort(effort),
                1,
                "unexpected multiplier for {effort:?}"
            );
        }

        // High efforts think silently for minutes, so they need more headroom.
        assert_eq!(stream_idle_timeout_multiplier_for_effort(Some("high")), 2);
        assert_eq!(stream_idle_timeout_multiplier_for_effort(Some("xhigh")), 3);
        assert_eq!(stream_idle_timeout_multiplier_for_effort(Some("max")), 4);

        // Swarm sentinels resolve to the top wire effort upstream.
        assert_eq!(stream_idle_timeout_multiplier_for_effort(Some("swarm")), 4);
        assert_eq!(
            stream_idle_timeout_multiplier_for_effort(Some("swarm-deep")),
            4
        );

        // Casing and padding come from config/CLI input, so normalize both.
        assert_eq!(stream_idle_timeout_multiplier_for_effort(Some("  MAX ")), 4);

        // An unknown/future effort must fall back to the base budget rather
        // than panicking or silently inheriting a huge multiplier.
        assert_eq!(
            stream_idle_timeout_multiplier_for_effort(Some("ultra-turbo")),
            1
        );

        // No effort may exceed the ceiling clients budget against.
        for effort in [
            "none",
            "minimal",
            "low",
            "medium",
            "high",
            "xhigh",
            "max",
            "swarm",
            "swarm-deep",
        ] {
            assert!(
                stream_idle_timeout_multiplier_for_effort(Some(effort))
                    <= MAX_STREAM_IDLE_TIMEOUT_MULTIPLIER,
                "{effort} exceeds MAX_STREAM_IDLE_TIMEOUT_MULTIPLIER"
            );
        }
    }

    #[test]
    fn effort_scaling_never_shrinks_the_base_budget() {
        let base = stream_idle_timeout();
        assert_eq!(stream_idle_timeout_for_effort(None), base);
        assert!(stream_idle_timeout_for_effort(Some("max")) > base);
        assert_eq!(
            max_stream_idle_timeout(),
            base * MAX_STREAM_IDLE_TIMEOUT_MULTIPLIER
        );
        // The unknown-effort ceiling must cover the largest known effort, or a
        // client stall guard could fire before the provider's own timeout.
        assert!(max_stream_idle_timeout() >= stream_idle_timeout_for_effort(Some("max")));
    }

    #[test]
    fn openai_watchdog_outlasts_the_persistent_websocket_transport() {
        let configured = Duration::from_secs(180);
        let watchdog = stream_watchdog_timeout_for_provider_with_base(
            Some("openai-oauth"),
            Some("medium"),
            configured,
        );

        assert_eq!(
            watchdog,
            Duration::from_secs(
                jcode_provider_openai::websocket_health::WEBSOCKET_COMPLETION_TIMEOUT_SECS
            )
        );
        assert!(watchdog > configured);
    }

    #[test]
    fn openai_watchdog_honors_larger_config_and_unknown_effort_ceiling() {
        let configured = Duration::from_secs(600);
        assert_eq!(
            stream_watchdog_timeout_for_provider_with_base(
                Some("OpenAI"),
                Some("medium"),
                configured,
            ),
            configured
        );
        assert_eq!(
            stream_watchdog_timeout_for_provider_with_base(Some("OpenAI"), Some("max"), configured,),
            configured * MAX_STREAM_IDLE_TIMEOUT_MULTIPLIER
        );
    }

    #[test]
    fn non_openai_watchdog_keeps_the_generic_stream_budget() {
        let configured = Duration::from_secs(180);
        assert_eq!(
            stream_watchdog_timeout_for_provider_with_base(
                Some("Claude"),
                Some("high"),
                configured,
            ),
            configured * 2
        );
    }
}
