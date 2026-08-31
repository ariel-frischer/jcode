use super::{Config, WebSearchEngine};

fn parse_env_bool(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn parse_env_list(raw: &str) -> Vec<String> {
    raw.split([',', '\n'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect()
}

impl Config {
    /// Resolve one invocation's resilient websearch policy. Environment values
    /// are read here as candidates rather than trusted blindly so invalid
    /// values can fall through to persisted configuration without weakening a
    /// bounded policy.
    pub fn resolve_websearch_policy(
        &self,
        request: Option<&jcode_config_types::WebSearchPolicyOverride>,
    ) -> anyhow::Result<super::ResolvedWebSearchPolicy> {
        if let Some(request) = request {
            request.validate().map_err(anyhow::Error::msg)?;
        }

        let persisted = &self.websearch.resilience;
        persisted.validate().map_err(anyhow::Error::msg)?;

        let enabled = resolve_bool(
            request.and_then(|v| v.enabled),
            &[
                "JCODE_WEBSEARCH_RESILIENCE_ENABLED",
                "JCODE_WEBSEARCH_ENABLED",
            ],
            persisted.enabled,
        );
        let duckduckgo_enabled = resolve_bool(
            request.and_then(|v| v.duckduckgo_enabled),
            &["JCODE_WEBSEARCH_DUCKDUCKGO_ENABLED"],
            persisted.duckduckgo_enabled,
        );
        let bing_enabled = resolve_bool(
            request.and_then(|v| v.bing_enabled),
            &["JCODE_WEBSEARCH_BING_ENABLED"],
            persisted.bing_enabled,
        );
        let searxng_enabled = resolve_bool(
            request.and_then(|v| v.searxng_enabled),
            &["JCODE_WEBSEARCH_SEARXNG_ENABLED"],
            persisted.searxng_enabled,
        );
        let fallback_enabled = resolve_bool(
            request.and_then(|v| v.fallback_enabled),
            &["JCODE_WEBSEARCH_FALLBACK_ENABLED"],
            persisted.fallback_enabled,
        );
        let retries_enabled = resolve_bool(
            request.and_then(|v| v.retries_enabled),
            &["JCODE_WEBSEARCH_RETRIES_ENABLED"],
            persisted.retries_enabled,
        );
        let health_suppression_enabled = resolve_bool(
            request.and_then(|v| v.health_suppression_enabled),
            &[
                "JCODE_WEBSEARCH_HEALTH_SUPPRESSION_ENABLED",
                "JCODE_WEBSEARCH_HEALTH_ENABLED",
            ],
            persisted.health_suppression_enabled,
        );
        let diagnostics_enabled = resolve_bool(
            request.and_then(|v| v.diagnostics_enabled),
            &["JCODE_WEBSEARCH_DIAGNOSTICS_ENABLED"],
            persisted.diagnostics_enabled,
        );
        let attempt_timeout_ms = resolve_bounded_u64(
            request.and_then(|v| v.attempt_timeout_ms),
            &[
                "JCODE_WEBSEARCH_ATTEMPT_TIMEOUT_MS",
                "JCODE_WEBSEARCH_TIMEOUT_MS",
            ],
            persisted.attempt_timeout_ms,
            jcode_config_types::WEBSEARCH_MIN_ATTEMPT_TIMEOUT_MS,
            jcode_config_types::WEBSEARCH_MAX_ATTEMPT_TIMEOUT_MS,
            "attempt_timeout_ms",
        );
        let max_retries = resolve_bounded_u8(
            request.and_then(|v| v.max_retries),
            &["JCODE_WEBSEARCH_MAX_RETRIES"],
            persisted.max_retries,
            jcode_config_types::WEBSEARCH_MIN_RETRIES,
            jcode_config_types::WEBSEARCH_MAX_RETRIES,
            "max_retries",
        );
        let health_failure_threshold = resolve_bounded_u8(
            request.and_then(|v| v.health_failure_threshold),
            &[
                "JCODE_WEBSEARCH_HEALTH_FAILURE_THRESHOLD",
                "JCODE_WEBSEARCH_HEALTH_THRESHOLD",
            ],
            persisted.health_failure_threshold,
            jcode_config_types::WEBSEARCH_MIN_HEALTH_FAILURE_THRESHOLD,
            jcode_config_types::WEBSEARCH_MAX_HEALTH_FAILURE_THRESHOLD,
            "health_failure_threshold",
        );
        let health_cooldown_ms = resolve_bounded_u64(
            request.and_then(|v| v.health_cooldown_ms),
            &["JCODE_WEBSEARCH_HEALTH_COOLDOWN_MS"],
            persisted.health_cooldown_ms,
            jcode_config_types::WEBSEARCH_MIN_HEALTH_COOLDOWN_MS,
            jcode_config_types::WEBSEARCH_MAX_HEALTH_COOLDOWN_MS,
            "health_cooldown_ms",
        );

        let fallback_order = resolve_fallback_order(request, self)?;
        let trusted_searxng_url = resolve_trusted_searxng_url(self)?;

        Ok(super::ResolvedWebSearchPolicy {
            enabled,
            duckduckgo_enabled,
            bing_enabled,
            searxng_enabled,
            fallback_order,
            fallback_enabled,
            attempt_timeout_ms,
            retries_enabled,
            max_retries,
            health_suppression_enabled,
            health_failure_threshold,
            health_cooldown_ms,
            diagnostics_enabled,
            trusted_searxng_url,
        })
    }
}

fn resolve_bool(candidate: Option<bool>, env_keys: &[&str], persisted: bool) -> bool {
    if let Some(candidate) = candidate {
        return candidate;
    }
    for key in env_keys {
        if let Ok(raw) = std::env::var(key) {
            if let Some(value) = parse_env_bool(&raw) {
                return value;
            }
            warn_invalid_websearch_env(key);
            break;
        }
    }
    persisted
}

fn resolve_bounded_u64(
    candidate: Option<u64>,
    env_keys: &[&str],
    persisted: u64,
    min: u64,
    max: u64,
    field: &str,
) -> u64 {
    if let Some(candidate) = candidate {
        return candidate;
    }
    for key in env_keys {
        if let Ok(raw) = std::env::var(key) {
            if let Ok(value) = raw.trim().parse::<u64>()
                && (min..=max).contains(&value)
            {
                return value;
            }
            warn_invalid_websearch_env_field(key, field);
            break;
        }
    }
    persisted
}

fn resolve_bounded_u8(
    candidate: Option<u8>,
    env_keys: &[&str],
    persisted: u8,
    min: u8,
    max: u8,
    field: &str,
) -> u8 {
    if let Some(candidate) = candidate {
        return candidate;
    }
    for key in env_keys {
        if let Ok(raw) = std::env::var(key) {
            if let Ok(value) = raw.trim().parse::<u8>()
                && (min..=max).contains(&value)
            {
                return value;
            }
            warn_invalid_websearch_env_field(key, field);
            break;
        }
    }
    persisted
}

fn resolve_fallback_order(
    request: Option<&jcode_config_types::WebSearchPolicyOverride>,
    config: &Config,
) -> anyhow::Result<Vec<WebSearchEngine>> {
    if let Some(order) = request.and_then(|v| v.fallback_order.as_ref()) {
        return Ok(order.clone());
    }
    if let Ok(raw) = std::env::var("JCODE_WEBSEARCH_FALLBACK_ENGINES") {
        let parsed = parse_engine_order_candidate(&raw);
        if let Some(order) = parsed {
            return Ok(order);
        }
        warn_invalid_websearch_env("JCODE_WEBSEARCH_FALLBACK_ENGINES");
    }
    if let Some(order) = config.websearch.resilience.fallback_order.clone() {
        return Ok(order);
    }
    if config.websearch.fallback_engines.is_empty() {
        Ok(vec![WebSearchEngine::Bing])
    } else {
        Ok(config.websearch.fallback_engines.clone())
    }
}

pub(super) fn parse_engine_order_candidate(raw: &str) -> Option<Vec<WebSearchEngine>> {
    let values = parse_env_list(raw);
    if values.is_empty() || values.len() > jcode_config_types::WEBSEARCH_MAX_ENGINES {
        return None;
    }
    let mut engines = Vec::with_capacity(values.len());
    for value in values {
        let engine = WebSearchEngine::parse(&value)?;
        engines.push(engine);
    }
    Some(engines)
}

fn resolve_trusted_searxng_url(config: &Config) -> anyhow::Result<Option<String>> {
    let persisted = config
        .websearch
        .resilience
        .trusted_searxng_url
        .as_deref()
        .or(config.websearch.searxng_url.as_deref());
    let persisted = match persisted {
        Some(value) if !value.trim().is_empty() => Some(validate_trusted_searxng_url(value)?),
        _ => None,
    };

    if let Ok(raw) = std::env::var("JCODE_WEBSEARCH_TRUSTED_SEARXNG_URL") {
        if raw.trim().is_empty() {
            warn_invalid_websearch_env("JCODE_WEBSEARCH_TRUSTED_SEARXNG_URL");
        } else {
            match validate_trusted_searxng_url(&raw) {
                Ok(value) => return Ok(Some(value)),
                Err(_) => warn_invalid_websearch_env("JCODE_WEBSEARCH_TRUSTED_SEARXNG_URL"),
            }
        }
    }
    Ok(persisted)
}

/// Accept HTTPS endpoints, plus HTTP loopback fixtures for local trusted
/// instances. Userinfo and missing hosts are never accepted.
pub fn validate_trusted_searxng_url(raw: &str) -> anyhow::Result<String> {
    let parsed = url::Url::parse(raw.trim()).map_err(|_| anyhow::anyhow!("invalid SearXNG URL"))?;
    if parsed.username() != "" || parsed.password().is_some() {
        anyhow::bail!("SearXNG URL must not contain userinfo");
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("SearXNG URL must contain a host"))?;
    let trusted_scheme = parsed.scheme() == "https"
        || (parsed.scheme() == "http" && matches!(host, "localhost" | "127.0.0.1" | "::1"));
    if !trusted_scheme {
        anyhow::bail!("SearXNG URL must use HTTPS unless it targets loopback HTTP");
    }
    Ok(parsed.as_str().trim_end_matches('/').to_string())
}

pub(super) fn warn_invalid_websearch_env(key: &str) {
    crate::logging::warn(&format!(
        "websearch: ignoring invalid environment candidate {key}"
    ));
}

pub(super) fn warn_invalid_websearch_env_field(key: &str, field: &str) {
    crate::logging::warn(&format!(
        "websearch: ignoring invalid environment candidate {key} for {field}"
    ));
}

#[cfg(test)]
mod websearch_policy_precedence_tests {
    use super::super::{Config, WebSearchEngine};
    use jcode_config_types::WebSearchPolicyOverride;

    const WEBSEARCH_ENV_KEYS: &[&str] = &[
        "JCODE_WEBSEARCH_RESILIENCE_ENABLED",
        "JCODE_WEBSEARCH_ENABLED",
        "JCODE_WEBSEARCH_DUCKDUCKGO_ENABLED",
        "JCODE_WEBSEARCH_BING_ENABLED",
        "JCODE_WEBSEARCH_SEARXNG_ENABLED",
        "JCODE_WEBSEARCH_FALLBACK_ENGINES",
        "JCODE_WEBSEARCH_FALLBACK_ENABLED",
        "JCODE_WEBSEARCH_ATTEMPT_TIMEOUT_MS",
        "JCODE_WEBSEARCH_TIMEOUT_MS",
        "JCODE_WEBSEARCH_RETRIES_ENABLED",
        "JCODE_WEBSEARCH_MAX_RETRIES",
        "JCODE_WEBSEARCH_HEALTH_SUPPRESSION_ENABLED",
        "JCODE_WEBSEARCH_HEALTH_ENABLED",
        "JCODE_WEBSEARCH_HEALTH_FAILURE_THRESHOLD",
        "JCODE_WEBSEARCH_HEALTH_THRESHOLD",
        "JCODE_WEBSEARCH_HEALTH_COOLDOWN_MS",
        "JCODE_WEBSEARCH_DIAGNOSTICS_ENABLED",
        "JCODE_WEBSEARCH_TRUSTED_SEARXNG_URL",
    ];

    fn with_clean_environment() -> impl Drop {
        let previous = WEBSEARCH_ENV_KEYS
            .iter()
            .map(|key| (*key, std::env::var_os(key)))
            .collect::<Vec<_>>();
        for key in WEBSEARCH_ENV_KEYS {
            crate::env::remove_var(key);
        }
        struct Restore(Vec<(&'static str, Option<std::ffi::OsString>)>);
        impl Drop for Restore {
            fn drop(&mut self) {
                for (key, value) in self.0.drain(..) {
                    match value {
                        Some(value) => crate::env::set_var(key, value),
                        None => crate::env::remove_var(key),
                    }
                }
            }
        }
        Restore(previous)
    }

    #[test]
    fn resolves_each_field_request_then_environment_then_persisted_then_default() {
        let _lock = crate::storage::lock_test_env();
        let _environment = with_clean_environment();
        let mut config = Config::default();
        config.websearch.resilience.enabled = false;
        config.websearch.resilience.attempt_timeout_ms = 20_000;
        config.websearch.resilience.max_retries = 2;
        config.websearch.fallback_engines = vec![WebSearchEngine::Searxng];

        let defaults = config.resolve_websearch_policy(None).unwrap();
        assert!(!defaults.enabled);
        assert_eq!(defaults.attempt_timeout_ms, 20_000);
        assert_eq!(defaults.max_retries, 2);
        assert_eq!(defaults.fallback_order, vec![WebSearchEngine::Searxng]);

        crate::env::set_var("JCODE_WEBSEARCH_RESILIENCE_ENABLED", "true");
        crate::env::set_var("JCODE_WEBSEARCH_ATTEMPT_TIMEOUT_MS", "30000");
        crate::env::set_var("JCODE_WEBSEARCH_FALLBACK_ENGINES", "bing,ddg");
        let environment = config.resolve_websearch_policy(None).unwrap();
        assert!(environment.enabled);
        assert_eq!(environment.attempt_timeout_ms, 30_000);
        assert_eq!(environment.max_retries, 2);
        assert_eq!(
            environment.fallback_order,
            vec![WebSearchEngine::Bing, WebSearchEngine::Duckduckgo]
        );

        let request = WebSearchPolicyOverride {
            enabled: Some(false),
            attempt_timeout_ms: Some(100),
            fallback_order: Some(vec![WebSearchEngine::Duckduckgo]),
            ..WebSearchPolicyOverride::default()
        };
        let request_policy = config.resolve_websearch_policy(Some(&request)).unwrap();
        assert!(!request_policy.enabled);
        assert_eq!(request_policy.attempt_timeout_ms, 100);
        assert_eq!(
            request_policy.fallback_order,
            vec![WebSearchEngine::Duckduckgo]
        );
    }

    #[test]
    fn invalid_environment_values_warn_and_fall_through_without_becoming_policy_values() {
        let _lock = crate::storage::lock_test_env();
        let _environment = with_clean_environment();
        let mut config = Config::default();
        config.websearch.resilience.attempt_timeout_ms = 5_000;
        config.websearch.resilience.max_retries = 1;
        config.websearch.fallback_engines = vec![WebSearchEngine::Bing];

        crate::env::set_var("JCODE_WEBSEARCH_ATTEMPT_TIMEOUT_MS", "secret-60001");
        crate::env::set_var("JCODE_WEBSEARCH_MAX_RETRIES", "not-a-number");
        crate::env::set_var("JCODE_WEBSEARCH_FALLBACK_ENGINES", "unknown-engine");
        let policy = config.resolve_websearch_policy(None).unwrap();
        assert_eq!(policy.attempt_timeout_ms, 5_000);
        assert_eq!(policy.max_retries, 1);
        assert_eq!(policy.fallback_order, vec![WebSearchEngine::Bing]);
    }

    #[test]
    fn invalid_request_values_fail_before_network_work() {
        let _lock = crate::storage::lock_test_env();
        let _environment = with_clean_environment();
        let config = Config::default();
        let request = WebSearchPolicyOverride {
            attempt_timeout_ms: Some(60_001),
            ..WebSearchPolicyOverride::default()
        };
        let error = config.resolve_websearch_policy(Some(&request)).unwrap_err();
        assert!(error.to_string().contains("attempt_timeout_ms"));
    }

    #[test]
    fn missing_engine_flags_default_to_eligible_and_unknown_order_entries_fail() {
        let _lock = crate::storage::lock_test_env();
        let _environment = with_clean_environment();
        let config = Config::default();
        let policy = config.resolve_websearch_policy(None).unwrap();
        assert!(policy.duckduckgo_enabled);
        assert!(policy.bing_enabled);
        assert!(policy.searxng_enabled);

        crate::env::set_var("JCODE_WEBSEARCH_FALLBACK_ENGINES", "ddg,not-real");
        let policy = config.resolve_websearch_policy(None).unwrap();
        assert_eq!(policy.fallback_order, vec![WebSearchEngine::Bing]);
    }
}
