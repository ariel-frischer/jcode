use super::*;

#[test]
fn memory_reasoning_effort_is_optional_and_legacy_config_defaults_to_none() {
    assert_eq!(Config::default().agents.memory_reasoning_effort, None);

    let legacy: Config = toml::from_str("[agents]\nmemory_model = \"gpt-5.6-luna\"\n")
        .expect("legacy agents config should parse without memory effort");
    assert_eq!(legacy.agents.memory_reasoning_effort, None);
}

#[test]
fn memory_reasoning_effort_round_trips_under_its_dedicated_key() {
    let cfg: Config = toml::from_str(
        "[agents]\nmemory_model = \"gpt-5.6-luna\"\nmemory_reasoning_effort = \"xhigh\"\n",
    )
    .expect("memory reasoning effort should parse");

    assert_eq!(cfg.agents.memory_reasoning_effort.as_deref(), Some("xhigh"));
    assert_eq!(cfg.provider.openai_reasoning_effort.as_deref(), Some("low"));

    let encoded = toml::to_string(&cfg).expect("config should serialize");
    assert!(encoded.contains("memory_reasoning_effort = \"xhigh\""));
    assert!(!encoded.contains("provider.openai_reasoning_effort"));
}

#[test]
fn test_memory_reasoning_env_overrides_persisted_value_and_is_independent() {
    let _guard = crate::storage::lock_test_env();
    let previous = std::env::var_os("JCODE_MEMORY_REASONING_EFFORT");
    crate::env::set_var("JCODE_MEMORY_REASONING_EFFORT", "  XHIGH ");

    let mut cfg = Config::default();
    cfg.agents.memory_reasoning_effort = Some("low".to_string());
    cfg.provider.openai_reasoning_effort = Some("medium".to_string());
    cfg.apply_env_overrides();

    assert_eq!(cfg.agents.memory_reasoning_effort.as_deref(), Some("xhigh"));
    assert_eq!(
        cfg.provider.openai_reasoning_effort.as_deref(),
        Some("medium")
    );

    restore_env_var("JCODE_MEMORY_REASONING_EFFORT", previous);
}

#[test]
fn test_blank_memory_reasoning_env_preserves_persisted_value() {
    let _guard = crate::storage::lock_test_env();
    let previous = std::env::var_os("JCODE_MEMORY_REASONING_EFFORT");
    crate::env::set_var("JCODE_MEMORY_REASONING_EFFORT", "  \t");

    let mut cfg = Config::default();
    cfg.agents.memory_reasoning_effort = Some("low".to_string());
    cfg.apply_env_overrides();

    assert_eq!(cfg.agents.memory_reasoning_effort.as_deref(), Some("low"));

    restore_env_var("JCODE_MEMORY_REASONING_EFFORT", previous);
}

#[test]
fn config_summary_reports_explicit_memory_reasoning_effort() {
    let mut cfg = Config::default();
    cfg.agents.memory_model = Some("gpt-5.6-luna".to_string());
    cfg.agents.memory_reasoning_effort = Some("xhigh".to_string());

    let summary = cfg.display_string();
    assert!(
        summary
            .contains("Memory reasoning: configured: xhigh; effective: xhigh; model: gpt-5.6-luna")
    );
}

#[test]
fn config_summary_distinguishes_unset_luna_default_and_omitted_effort() {
    let mut luna = Config::default();
    luna.agents.memory_model = Some("gpt-5.6-luna".to_string());
    let luna_summary = luna.display_string();
    assert!(luna_summary.contains("configured: (unset); effective: none; model: gpt-5.6-luna"));
    assert!(luna_summary.contains("model default none"));

    let mut other = Config::default();
    other.agents.memory_model = Some("gpt-4.1-mini".to_string());
    let other_summary = other.display_string();
    assert!(other_summary.contains("configured: (unset); effective: omitted; model: gpt-4.1-mini"));
    assert!(other_summary.contains("reasoning omitted"));
}

#[test]
fn config_summary_reports_oauth_fallback_and_invalid_memory_effort() {
    let _guard = crate::storage::lock_test_env();
    crate::provider::record_model_unavailable_for_account(
        "gpt-5.6-luna",
        "test memory summary fallback",
    );

    let mut fallback = Config::default();
    fallback.agents.memory_model = Some("gpt-5.6-luna".to_string());
    let fallback_summary = fallback.display_string();
    assert!(fallback_summary.contains("configured: (unset); effective: low; model: gpt-5.4"));
    assert!(fallback_summary.contains("OAuth fallback default low"));

    crate::provider::clear_all_model_unavailability_for_account();
    let mut invalid = Config::default();
    invalid.agents.memory_model = Some("gpt-5.6-luna".to_string());
    invalid.agents.memory_reasoning_effort = Some("turbo".to_string());
    let invalid_summary = invalid.display_string();
    assert!(invalid_summary.contains("invalid: Invalid memory reasoning effort 'turbo'"));
    assert!(invalid_summary.contains("gpt-5.6-luna"));
    assert!(invalid_summary.contains("Supported alternatives"));
}

#[test]
fn test_generated_default_config_uses_low_openai_reasoning_effort() {
    let _guard = crate::storage::lock_test_env();
    let prev_home = std::env::var_os("JCODE_HOME");
    let dir = tempfile::TempDir::new().expect("tempdir");
    crate::env::set_var("JCODE_HOME", dir.path());

    let path = Config::create_default_config_file().expect("create default config file");
    let content = std::fs::read_to_string(path).expect("read default config file");

    assert!(
        content.contains("openai_reasoning_effort = \"low\""),
        "generated default config should use low OpenAI reasoning effort"
    );
    assert!(
        content.contains("openai_service_tier = \"off\""),
        "generated default config should use Standard OpenAI mode"
    );
    assert!(
        content.contains("[tools]") && content.contains("profile = \"full\""),
        "generated default config should document tool profiles"
    );
    assert!(
        content.contains("[acp]") && content.contains("tool_profile = \"acp\""),
        "generated default config should document ACP profile settings"
    );
    assert!(
        content.contains("[agents]") && content.contains("swarm_spawn_mode = \"inline\""),
        "generated default config should document agent spawn defaults"
    );
    assert!(
        content.contains("memory_model = \"gpt-5.6-luna\"")
            && content.contains("reasoning effort \"none\"")
            && content.contains("memory_reasoning_effort = \"xhigh\"")
            && content.contains("JCODE_MEMORY_REASONING_EFFORT")
            && content.contains("none, minimal, low, medium, high, xhigh, or max"),
        "generated default config should document the Luna memory sidecar default"
    );

    // Effort keys come from the per-platform keybinding registry; the template
    // placeholders must always be substituted.
    assert!(
        !content.contains("@EFFORT_INCREASE@") && !content.contains("@EFFORT_DECREASE@"),
        "generated default config should substitute effort key placeholders"
    );
    let expected_increase = if cfg!(target_os = "macos") {
        "effort_increase = \"cmd+right\""
    } else {
        "effort_increase = \"alt+right\""
    };
    assert!(
        content.contains(expected_increase),
        "generated default config should use the platform effort_increase default"
    );

    // The generated file must always be valid TOML for the current Config schema.
    let parsed: Config =
        toml::from_str(&content).expect("generated default config should parse as Config");
    assert_eq!(parsed.agents.swarm_spawn_mode, SwarmSpawnMode::Inline);

    if let Some(prev) = prev_home {
        crate::env::set_var("JCODE_HOME", prev);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
}
