use serde::{Deserialize, Serialize};

/// Search engine used by the websearch tool.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "lowercase")]
pub enum WebSearchEngine {
    /// DuckDuckGo HTML search, no API key required.
    #[default]
    #[serde(alias = "ddg")]
    Duckduckgo,
    /// Bing search. Uses the Bing API when configured, otherwise Bing HTML search.
    Bing,
    /// SearXNG metasearch instance (JSON API). Requires `searxng_url` (or the
    /// `JCODE_SEARXNG_URL` env var) to point at a SearXNG instance. Useful on
    /// hosts where DuckDuckGo/Bing block the request via TLS fingerprinting.
    #[serde(alias = "searx")]
    Searxng,
}

impl WebSearchEngine {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Duckduckgo => "duckduckgo",
            Self::Bing => "bing",
            Self::Searxng => "searxng",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "duckduckgo" | "ddg" => Some(Self::Duckduckgo),
            "bing" => Some(Self::Bing),
            "searxng" | "searx" => Some(Self::Searxng),
            _ => None,
        }
    }
}

/// Configuration for the websearch tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WebSearchConfig {
    /// Preferred engine when the tool input does not specify one.
    pub engine: WebSearchEngine,
    /// Keyless HTML engines to try after the preferred engine fails.
    pub fallback_engines: Vec<WebSearchEngine>,
    /// Optional Bing API key for primary Bing searches. Fallback Bing uses keyless HTML search.
    pub bing_api_key: Option<String>,
    /// Environment variable containing the Bing API key.
    pub bing_api_key_env: String,
    /// Bing market, e.g. "en-US" or "zh-CN".
    pub bing_market: String,
    /// Base URL of a SearXNG instance (e.g. "https://searx.example.org"), used
    /// by the `searxng` engine. When empty, the `searxng_url_env` variable is
    /// consulted instead.
    pub searxng_url: Option<String>,
    /// Environment variable containing the SearXNG base URL.
    pub searxng_url_env: String,
    /// Opt-in resilient orchestration controls. The legacy path ignores this
    /// section when `enabled` is false, which is the default.
    pub resilience: WebSearchResilienceConfig,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            engine: WebSearchEngine::Duckduckgo,
            fallback_engines: vec![WebSearchEngine::Bing],
            bing_api_key: None,
            bing_api_key_env: "JCODE_BING_API_KEY".to_string(),
            bing_market: "en-US".to_string(),
            searxng_url: None,
            searxng_url_env: "JCODE_SEARXNG_URL".to_string(),
            resilience: WebSearchResilienceConfig::default(),
        }
    }
}

/// Bounds for the opt-in resilient websearch policy.
pub const WEBSEARCH_MIN_ATTEMPT_TIMEOUT_MS: u64 = 100;
pub const WEBSEARCH_MAX_ATTEMPT_TIMEOUT_MS: u64 = 60_000;
pub const WEBSEARCH_MIN_RETRIES: u8 = 0;
pub const WEBSEARCH_MAX_RETRIES: u8 = 2;
pub const WEBSEARCH_MIN_HEALTH_FAILURE_THRESHOLD: u8 = 1;
pub const WEBSEARCH_MAX_HEALTH_FAILURE_THRESHOLD: u8 = 10;
pub const WEBSEARCH_MIN_HEALTH_COOLDOWN_MS: u64 = 1_000;
pub const WEBSEARCH_MAX_HEALTH_COOLDOWN_MS: u64 = 300_000;
pub const WEBSEARCH_RETRY_DELAY_MS: u64 = 200;
pub const WEBSEARCH_MAX_ENGINES: usize = 3;

/// Persisted/environment policy for bounded resilient websearch.
///
/// Every sub-control is useful once the master switch is enabled, but the
/// master switch itself remains disabled by default so legacy installations
/// continue to use the exact legacy execution path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct WebSearchResilienceConfig {
    /// Select resilient orchestration instead of the legacy loop.
    pub enabled: bool,
    /// Whether DuckDuckGo may be physically attempted in resilient mode.
    pub duckduckgo_enabled: bool,
    /// Whether keyless HTML Bing may be physically attempted in resilient mode.
    pub bing_enabled: bool,
    /// Whether configured trusted SearXNG may be physically attempted.
    pub searxng_enabled: bool,
    /// Optional persisted fallback candidate list. The canonical legacy
    /// `websearch.fallback_engines` field remains the fallback source when this
    /// is absent; request and environment candidates have higher precedence.
    pub fallback_order: Option<Vec<WebSearchEngine>>,
    /// Whether engines after the preferred engine may be considered.
    pub fallback_enabled: bool,
    /// Finite timeout for one physical engine request.
    pub attempt_timeout_ms: u64,
    /// Whether eligible transient outcomes may retry the same engine.
    pub retries_enabled: bool,
    /// Maximum additional attempts for one engine sequence.
    pub max_retries: u8,
    /// Whether repeated eligible failures temporarily suppress an engine.
    pub health_suppression_enabled: bool,
    /// Consecutive eligible terminal failures required for suppression.
    pub health_failure_threshold: u8,
    /// Suppression cooldown duration.
    pub health_cooldown_ms: u64,
    /// Whether bounded structured and presentation diagnostics are emitted.
    pub diagnostics_enabled: bool,
    /// Optional explicitly trusted SearXNG endpoint. The legacy
    /// `websearch.searxng_url` remains supported and is used as a fallback.
    pub trusted_searxng_url: Option<String>,
}

impl Default for WebSearchResilienceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            duckduckgo_enabled: true,
            bing_enabled: true,
            searxng_enabled: true,
            fallback_order: None,
            fallback_enabled: true,
            attempt_timeout_ms: 10_000,
            retries_enabled: true,
            max_retries: 1,
            health_suppression_enabled: true,
            health_failure_threshold: 2,
            health_cooldown_ms: 30_000,
            diagnostics_enabled: true,
            trusted_searxng_url: None,
        }
    }
}

impl WebSearchResilienceConfig {
    /// Validate all bounded values before the first network request.
    pub fn validate(&self) -> Result<(), String> {
        validate_websearch_bounds(
            self.attempt_timeout_ms,
            self.max_retries,
            self.health_failure_threshold,
            self.health_cooldown_ms,
        )?;
        validate_fallback_order(self.fallback_order.as_deref())
    }
}

/// Non-secret per-request or per-session operational candidates. Credentials
/// and SearXNG endpoint URLs intentionally do not exist in this type.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct WebSearchPolicyOverride {
    pub enabled: Option<bool>,
    pub duckduckgo_enabled: Option<bool>,
    pub bing_enabled: Option<bool>,
    pub searxng_enabled: Option<bool>,
    pub fallback_order: Option<Vec<WebSearchEngine>>,
    pub fallback_enabled: Option<bool>,
    pub attempt_timeout_ms: Option<u64>,
    pub retries_enabled: Option<bool>,
    pub max_retries: Option<u8>,
    pub health_suppression_enabled: Option<bool>,
    pub health_failure_threshold: Option<u8>,
    pub health_cooldown_ms: Option<u64>,
    pub diagnostics_enabled: Option<bool>,
}

impl WebSearchPolicyOverride {
    /// Validate explicitly supplied request values before network work.
    pub fn validate(&self) -> Result<(), String> {
        validate_websearch_bounds(
            self.attempt_timeout_ms.unwrap_or(10_000),
            self.max_retries.unwrap_or(1),
            self.health_failure_threshold.unwrap_or(2),
            self.health_cooldown_ms.unwrap_or(30_000),
        )?;
        validate_fallback_order(self.fallback_order.as_deref())
    }
}

fn validate_websearch_bounds(
    attempt_timeout_ms: u64,
    max_retries: u8,
    health_failure_threshold: u8,
    health_cooldown_ms: u64,
) -> Result<(), String> {
    if !(WEBSEARCH_MIN_ATTEMPT_TIMEOUT_MS..=WEBSEARCH_MAX_ATTEMPT_TIMEOUT_MS)
        .contains(&attempt_timeout_ms)
    {
        return Err(format!(
            "websearch attempt_timeout_ms must be between {WEBSEARCH_MIN_ATTEMPT_TIMEOUT_MS} and {WEBSEARCH_MAX_ATTEMPT_TIMEOUT_MS}"
        ));
    }
    if !(WEBSEARCH_MIN_RETRIES..=WEBSEARCH_MAX_RETRIES).contains(&max_retries) {
        return Err(format!(
            "websearch max_retries must be between {WEBSEARCH_MIN_RETRIES} and {WEBSEARCH_MAX_RETRIES}"
        ));
    }
    if !(WEBSEARCH_MIN_HEALTH_FAILURE_THRESHOLD..=WEBSEARCH_MAX_HEALTH_FAILURE_THRESHOLD)
        .contains(&health_failure_threshold)
    {
        return Err(format!(
            "websearch health_failure_threshold must be between {WEBSEARCH_MIN_HEALTH_FAILURE_THRESHOLD} and {WEBSEARCH_MAX_HEALTH_FAILURE_THRESHOLD}"
        ));
    }
    if !(WEBSEARCH_MIN_HEALTH_COOLDOWN_MS..=WEBSEARCH_MAX_HEALTH_COOLDOWN_MS)
        .contains(&health_cooldown_ms)
    {
        return Err(format!(
            "websearch health_cooldown_ms must be between {WEBSEARCH_MIN_HEALTH_COOLDOWN_MS} and {WEBSEARCH_MAX_HEALTH_COOLDOWN_MS}"
        ));
    }
    Ok(())
}

fn validate_fallback_order(order: Option<&[WebSearchEngine]>) -> Result<(), String> {
    if let Some(order) = order {
        if order.is_empty() {
            return Err("websearch fallback_order must not be empty when supplied".to_string());
        }
        if order.len() > WEBSEARCH_MAX_ENGINES {
            return Err(format!(
                "websearch fallback_order supports at most {WEBSEARCH_MAX_ENGINES} engines"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod websearch_resilience_contract_tests {
    use super::*;

    #[test]
    fn resilient_policy_defaults_are_opt_in_but_ready_when_enabled() {
        let policy = WebSearchResilienceConfig::default();

        assert!(!policy.enabled);
        assert!(policy.duckduckgo_enabled);
        assert!(policy.bing_enabled);
        assert!(policy.searxng_enabled);
        assert!(policy.fallback_enabled);
        assert!(policy.retries_enabled);
        assert_eq!(policy.attempt_timeout_ms, 10_000);
        assert_eq!(policy.max_retries, 1);
        assert!(policy.health_suppression_enabled);
        assert_eq!(policy.health_failure_threshold, 2);
        assert_eq!(policy.health_cooldown_ms, 30_000);
        assert!(policy.diagnostics_enabled);
        assert!(policy.validate().is_ok());
    }

    #[test]
    fn policy_override_decodes_independent_non_secret_fields() {
        let override_value: WebSearchPolicyOverride = serde_json::from_value(serde_json::json!({
            "enabled": true,
            "bing_enabled": false,
            "fallback_order": ["ddg", "bing", "searx"],
            "attempt_timeout_ms": 250,
            "max_retries": 0,
            "health_failure_threshold": 10,
            "health_cooldown_ms": 300_000,
        }))
        .expect("valid operational override");

        assert_eq!(override_value.enabled, Some(true));
        assert_eq!(override_value.bing_enabled, Some(false));
        assert_eq!(
            override_value.fallback_order,
            Some(vec![
                WebSearchEngine::Duckduckgo,
                WebSearchEngine::Bing,
                WebSearchEngine::Searxng,
            ])
        );
        assert!(override_value.validate().is_ok());
    }

    #[test]
    fn policy_override_rejects_credentials_urls_unknown_keys_and_bad_bounds() {
        for value in [
            serde_json::json!({"bing_api_key": "secret"}),
            serde_json::json!({"trusted_searxng_url": "http://127.0.0.1:8080"}),
            serde_json::json!({"not_a_policy_field": true}),
        ] {
            assert!(
                serde_json::from_value::<WebSearchPolicyOverride>(value).is_err(),
                "sensitive or unknown request fields must be rejected"
            );
        }

        for (field, value) in [
            ("attempt_timeout_ms", serde_json::json!(99)),
            ("attempt_timeout_ms", serde_json::json!(60_001)),
            ("max_retries", serde_json::json!(3)),
            ("health_failure_threshold", serde_json::json!(0)),
            ("health_failure_threshold", serde_json::json!(11)),
            ("health_cooldown_ms", serde_json::json!(999)),
            ("health_cooldown_ms", serde_json::json!(300_001)),
        ] {
            let value = serde_json::json!({field: value});
            let parsed: WebSearchPolicyOverride = serde_json::from_value(value).unwrap();
            assert!(parsed.validate().is_err(), "{field} should be bounded");
        }
    }

    #[test]
    fn legacy_websearch_config_decodes_without_a_resilience_section() {
        let config: WebSearchConfig = toml::from_str(
            r#"
                engine = "ddg"
                fallback_engines = ["bing", "searx"]
                bing_api_key = "legacy-secret"
                bing_api_key_env = "BING_KEY"
                bing_market = "zh-CN"
                searxng_url = "https://search.example.test"
                searxng_url_env = "SEARX_URL"
            "#,
        )
        .expect("legacy websearch config remains valid");

        assert_eq!(config.engine, WebSearchEngine::Duckduckgo);
        assert_eq!(
            config.fallback_engines,
            vec![WebSearchEngine::Bing, WebSearchEngine::Searxng]
        );
        assert_eq!(config.bing_market, "zh-CN");
        assert_eq!(config.bing_api_key.as_deref(), Some("legacy-secret"));
        assert_eq!(
            config.searxng_url.as_deref(),
            Some("https://search.example.test")
        );
        assert!(!config.resilience.enabled);
    }

    #[test]
    fn explicit_master_setting_round_trips() {
        let mut config = WebSearchConfig::default();
        config.resilience.enabled = true;
        let encoded = toml::to_string(&config).expect("serialize resilient config");
        assert!(encoded.contains("enabled = true"));

        let decoded: WebSearchConfig = toml::from_str(&encoded).expect("decode resilient config");
        assert!(decoded.resilience.enabled);
        assert!(decoded.resilience.validate().is_ok());
    }
}
