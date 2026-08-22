use super::*;

#[test]
fn complete_profile_round_trips_without_losing_fields() {
    let input = r#"
[profiles.review]
provider = "openrouter"
model = "openai/gpt-5.6"
reasoning_effort = "high"
provider_profile = "review-gateway"
tool_profile = "minimal"
tools = ["read", "agentgrep"]
disabled_tools = ["bash"]
skills = ["pr-reviewer"]
instructions = "Review correctness and regression risk."
"#;

    let config: Config = toml::from_str(input).expect("complete profile should parse");
    let profile = config.profiles.get("review").expect("review profile");
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
        Some("Review correctness and regression risk.")
    );

    let serialized = toml::to_string(&config).expect("profile config should serialize");
    let round_trip: Config = toml::from_str(&serialized).expect("serialized profile should parse");
    assert_eq!(round_trip.profiles.get("review"), Some(profile));
}

#[test]
fn absent_profiles_preserve_defaults_and_stay_omitted() {
    let config: Config = toml::from_str("").expect("legacy empty config should parse");
    assert!(config.profiles.is_empty());
    assert_eq!(
        config.provider.default_model,
        Config::default().provider.default_model
    );
    assert_eq!(
        config.tools.selection(),
        Config::default().tools.selection()
    );

    let serialized = toml::to_string(&config).expect("legacy config should serialize");
    assert!(!serialized.contains("profiles"));
}
