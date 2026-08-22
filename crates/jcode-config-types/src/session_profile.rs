use serde::{Deserialize, Serialize};

/// Persisted values selectable for one interactive or one-shot agent session.
///
/// Resolution and environment-dependent validation intentionally live in
/// `jcode-base`; this crate owns only the dependency-light serde contract.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionProfileConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_profile: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub disabled_tools: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn session_profile_preserves_every_supported_field() {
        let value = json!({
            "provider": "openrouter",
            "model": "openai/gpt-5.6",
            "reasoning_effort": "high",
            "provider_profile": "review-gateway",
            "tool_profile": "minimal",
            "tools": ["read", "agentgrep"],
            "disabled_tools": ["bash"],
            "skills": ["pr-reviewer"],
            "instructions": "Review for correctness and regression risk."
        });

        let profile: SessionProfileConfig =
            serde_json::from_value(value.clone()).expect("complete profile should deserialize");

        assert_eq!(profile.provider.as_deref(), Some("openrouter"));
        assert_eq!(profile.model.as_deref(), Some("openai/gpt-5.6"));
        assert_eq!(profile.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(profile.provider_profile.as_deref(), Some("review-gateway"));
        assert_eq!(profile.tool_profile.as_deref(), Some("minimal"));
        assert_eq!(profile.tools, ["read", "agentgrep"]);
        assert_eq!(profile.disabled_tools, ["bash"]);
        assert_eq!(profile.skills, ["pr-reviewer"]);
        assert_eq!(
            profile.instructions.as_deref(),
            Some("Review for correctness and regression risk.")
        );
        assert_eq!(serde_json::to_value(profile).unwrap(), value);
    }

    #[test]
    fn omitted_fields_use_empty_optional_defaults() {
        let profile: SessionProfileConfig = serde_json::from_value(json!({})).unwrap();

        assert!(profile.provider.is_none());
        assert!(profile.model.is_none());
        assert!(profile.reasoning_effort.is_none());
        assert!(profile.provider_profile.is_none());
        assert!(profile.tool_profile.is_none());
        assert!(profile.tools.is_empty());
        assert!(profile.disabled_tools.is_empty());
        assert!(profile.skills.is_empty());
        assert!(profile.instructions.is_none());
    }
}
