use serde::{Deserialize, Serialize};

/// Optional raw safety bounds for unattended `jcode run` invocations.
///
/// Values deliberately remain strings until the application-layer resolver
/// validates them. This preserves source-aware diagnostics for empty,
/// malformed, and out-of-range input from TOML and the environment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct RunSafetyConfig {
    /// Maximum number of completed top-level turns.
    pub max_turns: Option<String>,
    /// Maximum number of jcode Registry tool executions.
    pub max_tool_steps: Option<String>,
    /// Maximum native token-usage delta for this invocation.
    pub token_budget: Option<String>,
    /// Absolute RFC3339 deadline with an explicit offset.
    pub deadline: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::RunSafetyConfig;

    #[test]
    fn run_safety_config_is_optional_and_round_trips_documented_fields() {
        let absent: RunSafetyConfig =
            serde_json::from_str("{}").expect("empty config should parse");
        assert_eq!(absent, RunSafetyConfig::default());

        let config: RunSafetyConfig = serde_json::from_str(
            r#"{"max_turns":"3","max_tool_steps":"7","token_budget":"1000","deadline":"2030-01-01T00:00:00Z"}"#,
        )
        .expect("run safety fields should parse as raw strings");
        assert_eq!(config.max_turns.as_deref(), Some("3"));
        assert_eq!(config.max_tool_steps.as_deref(), Some("7"));
        assert_eq!(config.token_budget.as_deref(), Some("1000"));
        assert_eq!(config.deadline.as_deref(), Some("2030-01-01T00:00:00Z"));

        let encoded = serde_json::to_string(&config).expect("run safety fields should serialize");
        assert!(encoded.contains("\"max_turns\":\"3\""));
        assert!(encoded.contains("\"max_tool_steps\":\"7\""));
        assert!(encoded.contains("\"token_budget\":\"1000\""));
        assert!(encoded.contains("\"deadline\":\"2030-01-01T00:00:00Z\""));
    }
}
