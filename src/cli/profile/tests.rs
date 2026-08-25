use super::ProfileRunOptions;
use crate::cli::args::Args;
use crate::cli::provider_init::ProviderChoice;
use crate::config::{
    Config, FieldSource, NamedProviderConfig, NamedProviderType, SessionProfileConfig, SkillsMode,
    ToolConfig,
};
use clap::Parser;
use std::collections::HashSet;
use std::ffi::OsString;

fn complete_profile_config() -> Config {
    let mut config = Config::default();
    config.profiles.insert(
        "review".to_owned(),
        SessionProfileConfig {
            provider: Some("openai".to_owned()),
            model: Some("gpt-5.6-luna".to_owned()),
            reasoning_effort: Some("high".to_owned()),
            provider_profile: Some("team-gateway".to_owned()),
            tool_profile: Some("minimal".to_owned()),
            tools: vec!["read".to_owned(), "write".to_owned()],
            disabled_tools: vec!["write".to_owned()],
            skills: vec!["rust".to_owned(), "testing".to_owned()],
            skills_mode: None,
            disabled_skills: Vec::new(),
            instructions: Some("Keep the review focused and actionable.".to_owned()),
            handoff: None,
            file_mentions_ignore: Vec::new(),
        },
    );
    config
}

fn env_snapshot() -> Vec<(String, Option<OsString>)> {
    [
        "JCODE_PROVIDER",
        "JCODE_MODEL",
        "JCODE_PROVIDER_PROFILE_NAME",
        "JCODE_PROVIDER_PROFILE_ACTIVE",
        "JCODE_OPENAI_REASONING_EFFORT",
        "JCODE_TOOL_PROFILE",
        "JCODE_TOOLS",
        "JCODE_DISABLED_TOOLS",
    ]
    .into_iter()
    .map(|key| (key.to_owned(), std::env::var_os(key)))
    .collect()
}

fn profile_with_values() -> SessionProfileConfig {
    SessionProfileConfig {
        provider: Some("openai".to_owned()),
        model: Some("profile-model".to_owned()),
        reasoning_effort: Some("high".to_owned()),
        provider_profile: Some("profile-gateway".to_owned()),
        tool_profile: Some("full".to_owned()),
        tools: vec!["edit".to_owned()],
        disabled_tools: vec!["write".to_owned()],
        skills: Vec::new(),
        skills_mode: None,
        disabled_skills: Vec::new(),
        instructions: None,
        handoff: None,
        file_mentions_ignore: Vec::new(),
    }
}

fn config_with_profile(profile: SessionProfileConfig) -> Config {
    let mut config = Config::default();
    if let Some(provider_profile) = profile.provider_profile.clone() {
        config.providers.insert(
            provider_profile,
            NamedProviderConfig {
                base_url: "https://profile.example.test".to_owned(),
                ..Default::default()
            },
        );
    }
    config.profiles.insert("review".to_owned(), profile);
    config
}

fn profile_args(extra: &[&str]) -> Args {
    let mut argv = vec!["jcode", "--profile", "review"];
    argv.extend_from_slice(extra);
    argv.extend(["run", "hello"]);
    Args::try_parse_from(argv).expect("profile run arguments should parse")
}

fn interactive_args(extra: &[&str]) -> Args {
    let mut argv = vec!["jcode", "--profile", "review"];
    argv.extend_from_slice(extra);
    Args::try_parse_from(argv).expect("interactive profile arguments should parse")
}

fn effective_tool_config() -> ToolConfig {
    ToolConfig {
        profile: "minimal".to_owned(),
        enabled: vec!["read".to_owned(), "write".to_owned()],
        disabled: vec!["bash".to_owned()],
        ..ToolConfig::default()
    }
}

struct ProviderProfileEnvGuard {
    previous_name: Option<OsString>,
    previous_active: Option<OsString>,
}

impl ProviderProfileEnvGuard {
    fn set(name: &str) -> Self {
        let previous_name = std::env::var_os("JCODE_PROVIDER_PROFILE_NAME");
        let previous_active = std::env::var_os("JCODE_PROVIDER_PROFILE_ACTIVE");
        crate::env::set_var("JCODE_PROVIDER_PROFILE_NAME", name);
        crate::env::set_var("JCODE_PROVIDER_PROFILE_ACTIVE", "1");
        Self {
            previous_name,
            previous_active,
        }
    }
}

impl Drop for ProviderProfileEnvGuard {
    fn drop(&mut self) {
        if let Some(value) = &self.previous_name {
            crate::env::set_var("JCODE_PROVIDER_PROFILE_NAME", value);
        } else {
            crate::env::remove_var("JCODE_PROVIDER_PROFILE_NAME");
        }
        if let Some(value) = &self.previous_active {
            crate::env::set_var("JCODE_PROVIDER_PROFILE_ACTIVE", value);
        } else {
            crate::env::remove_var("JCODE_PROVIDER_PROFILE_ACTIVE");
        }
    }
}

#[test]
fn profile_precedence_explicit_invocation_wins_over_environment_and_profile() {
    let mut config = config_with_profile(profile_with_values());
    // These values model Config after its existing environment override
    // pass. The resolver must not let the selected profile replace them
    // when the invocation supplies no matching flag.
    config.provider.default_provider = Some("claude".to_owned());
    config.provider.default_model = Some("environment-model".to_owned());
    config.provider.anthropic_reasoning_effort = Some("environment-reasoning".to_owned());
    config.tools = effective_tool_config();

    let args = profile_args(&[
        "--provider",
        "openrouter",
        "--model",
        "explicit-model",
        "--reasoning-effort",
        "max",
        "--tool-profile",
        "none",
        "--tools",
        "read",
        "--disabled-tools",
        "write",
    ]);
    let options = super::resolve_run_options(&args, &config)
        .expect("explicit profile run should resolve")
        .expect("--profile should produce run options");

    assert_eq!(options.provider, ProviderChoice::Openrouter);
    assert_eq!(options.model.as_deref(), Some("explicit-model"));
    assert_eq!(options.provider_profile, None);
    assert_eq!(options.reasoning_effort.as_deref(), Some("max"));
    assert_eq!(
        options.tool_selection,
        ToolConfig {
            profile: "none".to_owned(),
            enabled: vec!["read".to_owned()],
            disabled: vec!["write".to_owned()],
            ..ToolConfig::default()
        }
        .selection()
    );
}

#[test]
fn explicit_provider_clears_lower_precedence_profile_provider_profile() {
    let config = config_with_profile(SessionProfileConfig {
        provider_profile: Some("profile-gateway".to_owned()),
        ..Default::default()
    });
    let args = profile_args(&["--provider", "anthropic-api"]);

    let options = super::resolve_run_options(&args, &config)
        .expect("explicit provider should override the profile route")
        .expect("--profile should produce run options");

    assert_eq!(options.provider, ProviderChoice::AnthropicApi);
    assert_eq!(options.provider_profile, None);
}

#[test]
fn explicit_provider_profile_selects_its_runtime_and_clears_profile_provider() {
    let mut config = config_with_profile(SessionProfileConfig {
        provider: Some("openai".to_owned()),
        ..Default::default()
    });
    config.providers.insert(
        "anthropic-gateway".to_owned(),
        NamedProviderConfig {
            provider_type: NamedProviderType::AnthropicCompatible,
            base_url: "https://gateway.example.test".to_owned(),
            ..Default::default()
        },
    );
    let args = profile_args(&["--provider-profile", "anthropic-gateway"]);

    let options = super::resolve_run_options(&args, &config)
        .expect("explicit provider profile should resolve")
        .expect("--profile should produce run options");

    assert_eq!(options.provider, ProviderChoice::AnthropicApi);
    assert_eq!(
        options.provider_profile.as_deref(),
        Some("anthropic-gateway")
    );
}

#[test]
fn conflicting_explicit_provider_and_provider_profile_fail_before_provider_setup() {
    let mut config = config_with_profile(SessionProfileConfig::default());
    config.providers.insert(
        "anthropic-gateway".to_owned(),
        NamedProviderConfig {
            provider_type: NamedProviderType::AnthropicCompatible,
            base_url: "https://gateway.example.test".to_owned(),
            ..Default::default()
        },
    );
    let args = profile_args(&[
        "--provider",
        "openrouter",
        "--provider-profile",
        "anthropic-gateway",
    ]);

    let error = super::resolve_run_options(&args, &config)
        .expect_err("conflicting provider selectors must fail");

    assert!(
        error
            .to_string()
            .contains("--provider openrouter conflicts with --provider-profile anthropic-gateway")
    );
}

#[test]
fn direct_conflicting_provider_selectors_fail_without_a_session_profile() {
    let args = Args::try_parse_from([
        "jcode",
        "--provider",
        "openrouter",
        "--provider-profile",
        "anthropic-gateway",
        "run",
        "hello",
    ])
    .expect("direct run arguments should parse");

    let error = super::validate_explicit_provider_selectors(&args)
        .expect_err("direct conflicting selectors must fail");

    assert!(
        error
            .to_string()
            .contains("--provider openrouter conflicts with --provider-profile anthropic-gateway")
    );
}

#[test]
fn matching_explicit_provider_selectors_are_still_mutually_exclusive() {
    let args = Args::try_parse_from([
        "jcode",
        "--provider",
        "openai-compatible",
        "--provider-profile",
        "openai-gateway",
        "run",
        "hello",
    ])
    .expect("direct run arguments should parse");

    let error = super::validate_explicit_provider_selectors(&args)
        .expect_err("matching explicit selectors must still be rejected");

    assert!(
        error.to_string().contains(
            "--provider openai-compatible conflicts with --provider-profile openai-gateway"
        )
    );
}

#[test]
fn profile_precedence_selected_profile_wins_over_base_when_invocation_is_unset() {
    let _env_lock = crate::storage::lock_test_env();
    let _provider_profile_env = ProviderProfileEnvGuard::set("environment-gateway");
    let mut config = config_with_profile(profile_with_values());
    config.providers.insert(
        "environment-gateway".to_owned(),
        NamedProviderConfig {
            base_url: "https://environment.example.test".to_owned(),
            ..Default::default()
        },
    );
    // Config::load_strict applies base provider/model/reasoning/tool values
    // before this resolver is called. Keep them distinct from the selected
    // profile so the profile precedence is observable.
    config.provider.default_provider = Some("claude".to_owned());
    config.provider.default_model = Some("environment-model".to_owned());
    config.provider.anthropic_reasoning_effort = Some("low".to_owned());
    config.tools = effective_tool_config();

    let args = profile_args(&[]);
    assert_eq!(
        args.provider,
        ProviderChoice::Auto,
        "Auto is parser default, not explicit input"
    );

    let options = super::resolve_run_options(&args, &config)
        .expect("profile run should resolve")
        .expect("--profile should produce run options");

    assert_eq!(options.provider, ProviderChoice::Openai);
    assert_eq!(options.model.as_deref(), Some("profile-model"));
    assert_eq!(options.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(
        options.provider_profile.as_deref(),
        Some("environment-gateway")
    );
    assert_eq!(
        options.tool_selection,
        ToolConfig {
            profile: "full".to_owned(),
            enabled: vec!["edit".to_owned()],
            disabled: vec!["write".to_owned()],
            ..ToolConfig::default()
        }
        .selection(),
        "selected profile tool settings must beat base tool settings"
    );
}

#[test]
fn profile_precedence_unset_invocation_inherits_profile_values_and_auto_is_not_explicit() {
    let config = config_with_profile(profile_with_values());
    let args = profile_args(&[]);

    assert_eq!(args.provider, ProviderChoice::Auto);
    assert!(args.model.is_none());
    assert!(args.reasoning_effort.is_none());
    assert!(args.provider_profile.is_none());

    let options = super::resolve_run_options(&args, &config)
        .expect("profile run should resolve")
        .expect("--profile should produce run options");

    assert_eq!(options.provider, ProviderChoice::Openai);
    assert_eq!(options.model.as_deref(), Some("profile-model"));
    assert_eq!(options.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(options.provider_profile.as_deref(), Some("profile-gateway"));
    assert_eq!(
        options.tool_selection,
        ToolConfig {
            profile: "full".to_owned(),
            enabled: vec!["edit".to_owned()],
            disabled: vec!["write".to_owned()],
            ..ToolConfig::default()
        }
        .selection()
    );
}

#[test]
fn profile_precedence_base_config_wins_when_profile_fields_are_unset() {
    let mut config = config_with_profile(SessionProfileConfig::default());
    config.provider.default_provider = Some("openai".to_owned());
    config.provider.default_model = Some("base-model".to_owned());
    config.provider.openai_reasoning_effort = Some("base-reasoning".to_owned());
    config.tools = effective_tool_config();

    let args = profile_args(&[]);
    let options = super::resolve_run_options(&args, &config)
        .expect("profile run should resolve")
        .expect("--profile should produce run options");

    assert_eq!(options.provider, ProviderChoice::Openai);
    assert_eq!(options.model.as_deref(), Some("base-model"));
    assert_eq!(options.reasoning_effort.as_deref(), Some("base-reasoning"));
    assert_eq!(options.provider_profile, None);
    assert_eq!(options.tool_selection, effective_tool_config().selection());
}

#[test]
fn profile_precedence_built_in_defaults_apply_when_no_source_is_set() {
    let config = config_with_profile(SessionProfileConfig::default());
    let args = profile_args(&[]);
    let options = super::resolve_run_options(&args, &config)
        .expect("profile run should resolve")
        .expect("--profile should produce run options");

    assert_eq!(options.provider, ProviderChoice::Auto);
    assert_eq!(options.model, None);
    assert_eq!(options.reasoning_effort, None);
    assert_eq!(options.provider_profile, None);
    assert_eq!(options.tool_selection, ToolConfig::default().selection());
}

#[test]
fn selected_profile_becomes_effective_provider_model_tools_and_prompt_overlay() {
    let config = complete_profile_config();
    let resolved = config
        .resolve_session_profile(Some("review"))
        .expect("review profile should resolve");
    let base_tools = ToolConfig {
        profile: "full".to_owned(),
        ..ToolConfig::default()
    };

    let options = ProfileRunOptions::from_resolved_profile(
        ProviderChoice::Auto,
        Some("base-model"),
        Some("base-gateway"),
        Some("low"),
        &base_tools,
        &resolved,
    )
    .expect("a complete profile should convert without provider startup");

    assert_eq!(options.provider, ProviderChoice::Openai);
    assert_eq!(options.model.as_deref(), Some("gpt-5.6-luna"));
    assert_eq!(options.provider_profile.as_deref(), Some("team-gateway"));
    assert_eq!(options.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(
        options.tool_selection.allowed_tools,
        Some(HashSet::from(["read".to_owned()]))
    );
    assert_eq!(
        options.tool_selection.disabled_tools,
        HashSet::from(["write".to_owned()])
    );
    assert_eq!(
        options.prompt_overlay.skill_names,
        vec!["rust".to_owned(), "testing".to_owned()]
    );
    assert_eq!(
        options.prompt_overlay.instructions.as_deref(),
        Some("Keep the review focused and actionable.")
    );
}

#[test]
fn profile_conversion_preserves_base_values_when_no_profile_is_selected() {
    let base_tools = ToolConfig {
        profile: "minimal".to_owned(),
        ..ToolConfig::default()
    };
    let base_selection = base_tools.selection();

    let options = ProfileRunOptions::from_resolved_profile(
        ProviderChoice::Auto,
        Some("base-model"),
        Some("base-gateway"),
        Some("low"),
        &base_tools,
        &Default::default(),
    )
    .expect("the no-profile path should retain existing defaults");

    assert_eq!(options.provider, ProviderChoice::Auto);
    assert_eq!(options.model.as_deref(), Some("base-model"));
    assert_eq!(options.provider_profile.as_deref(), Some("base-gateway"));
    assert_eq!(options.reasoning_effort.as_deref(), Some("low"));
    assert_eq!(options.tool_selection, base_selection);
    assert!(options.prompt_overlay.is_empty());
}

#[test]
fn profile_conversion_is_pure_and_does_not_mutate_config_or_runtime_environment() {
    let config = complete_profile_config();
    let before_config = toml::to_string(&config).expect("config should serialize");
    let before_env = env_snapshot();
    let resolved = config
        .resolve_session_profile(Some("review"))
        .expect("review profile should resolve");

    let _options = ProfileRunOptions::from_resolved_profile(
        ProviderChoice::Auto,
        None,
        None,
        None,
        &config.tools,
        &resolved,
    )
    .expect("conversion should be synchronous and data-only");

    assert_eq!(
        toml::to_string(&config).expect("config should serialize"),
        before_config,
        "profile conversion must not save or rewrite persisted Config"
    );
    assert_eq!(
        env_snapshot(),
        before_env,
        "profile conversion must not mutate process-global provider/tool environment"
    );
}

#[test]
fn unknown_profile_fails_before_cli_options_can_reach_provider_setup() {
    let mut config = Config::default();
    config
        .profiles
        .insert("review".to_owned(), SessionProfileConfig::default());
    config
        .profiles
        .insert("ship".to_owned(), SessionProfileConfig::default());
    let args = Args::try_parse_from(["jcode", "--profile", "missing", "run", "hello"])
        .expect("unknown profile arguments should parse");

    let error = super::resolve_run_options(&args, &config)
        .expect_err("unknown profile selection must stop before provider setup");
    let message = error.to_string();
    assert!(
        message.contains("review"),
        "diagnostic should name requested profile: {message}"
    );
    assert!(
        message.contains("ship"),
        "diagnostic should list available profile choices: {message}"
    );
    assert!(
        message.contains("available"),
        "diagnostic should explain correction path: {message}"
    );
    assert!(
        message.contains("config.toml"),
        "diagnostic should identify configuration location: {message}"
    );
}

#[test]
fn invalid_profile_values_fail_without_constructing_run_options() {
    for profile in [
        SessionProfileConfig {
            provider: Some("not-a-provider".to_owned()),
            ..SessionProfileConfig::default()
        },
        SessionProfileConfig {
            reasoning_effort: Some("turbo".to_owned()),
            ..SessionProfileConfig::default()
        },
        SessionProfileConfig {
            model: Some("\u{7f}".to_owned()),
            ..SessionProfileConfig::default()
        },
    ] {
        let config = config_with_profile(profile);
        let args = profile_args(&[]);
        let error = super::resolve_run_options(&args, &config)
            .expect_err("invalid profile values must fail before provider setup");
        let message = error.to_string();
        assert!(
            message.contains("review"),
            "diagnostic should name profile: {message}"
        );
        assert!(
            message.contains("provider")
                || message.contains("reasoning_effort")
                || message.contains("model"),
            "diagnostic should identify offending field: {message}"
        );
    }
}

#[test]
fn unavailable_profile_skill_fails_before_run_options_are_constructed() {
    let config = config_with_profile(SessionProfileConfig {
        skills: vec!["skill-that-is-not-installed".to_owned()],
        ..SessionProfileConfig::default()
    });
    let args = profile_args(&[]);
    let error = super::resolve_run_options(&args, &config)
        .expect_err("unavailable profile skills must fail before provider setup");
    let message = error.to_string();
    assert!(
        message.contains("review"),
        "diagnostic should name profile: {message}"
    );
    assert!(
        message.contains("skill-that-is-not-installed"),
        "diagnostic should name missing skill: {message}"
    );
    assert!(
        !message.contains("secret"),
        "diagnostic must not expose skill contents: {message}"
    );
}

#[test]
fn interactive_startup_projection_matches_profile_effective_values() {
    let config = config_with_profile(profile_with_values());
    let args = interactive_args(&[]);
    let options = super::resolve_interactive_options(&args, &config)
        .expect("interactive profile should resolve")
        .expect("selected profile should produce startup options");

    assert_eq!(options.profile_name.as_deref(), Some("review"));
    assert_eq!(options.provider, ProviderChoice::Openai);
    assert_eq!(options.model.as_deref(), Some("profile-model"));
    assert_eq!(options.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(options.provider_profile.as_deref(), Some("profile-gateway"));
    assert_eq!(
        options.tool_selection,
        ToolConfig {
            profile: "full".to_owned(),
            enabled: vec!["edit".to_owned()],
            disabled: vec!["write".to_owned()],
            ..ToolConfig::default()
        }
        .selection()
    );

    let startup = options.startup_metadata();
    assert_eq!(startup.profile_name.as_deref(), Some("review"));
    assert_eq!(startup.provider.as_deref(), Some("openai"));
    assert_eq!(startup.model.as_deref(), Some("profile-model"));
    assert_eq!(startup.allowed_tools, Some(vec!["edit".to_owned()]));
    assert_eq!(startup.disabled_tools, vec!["write".to_owned()]);
    assert_eq!(startup.instructions, None);
}

#[test]
fn interactive_no_profile_keeps_legacy_startup_path() {
    let args = Args::try_parse_from(["jcode"]).expect("omitted profile should parse");
    assert!(
        super::resolve_interactive_options(&args, &Config::default())
            .expect("no-profile resolution should not inspect config profiles")
            .is_none()
    );
}

#[test]
fn inspection_list_is_stable_and_marks_only_the_requested_profile() {
    let mut config = Config::default();
    config
        .profiles
        .insert("zeta".to_owned(), SessionProfileConfig::default());
    config
        .profiles
        .insert("alpha".to_owned(), SessionProfileConfig::default());

    let report = super::profile_list(&config, Some("alpha"));
    assert_eq!(report.current.as_deref(), Some("alpha"));
    assert_eq!(
        report
            .profiles
            .iter()
            .map(|entry| (entry.name.as_str(), entry.active))
            .collect::<Vec<_>>(),
        vec![("alpha", true), ("zeta", false)]
    );
}

#[test]
fn profile_show_redacts_instruction_bodies_and_reports_policy_fields() {
    let mut config = Config::default();
    config.profiles.insert(
        "safe".to_owned(),
        SessionProfileConfig {
            provider: Some("openai".to_owned()),
            model: Some("fixture-model".to_owned()),
            skills_mode: Some(SkillsMode::Allowlist),
            skills: vec!["review".to_owned()],
            disabled_skills: vec!["secret".to_owned()],
            instructions: Some("never print fixture-secret-body".to_owned()),
            ..SessionProfileConfig::default()
        },
    );

    let report = super::profile_show(&config, "safe").expect("profile should be found");
    let encoded = serde_json::to_string(&report).expect("report should serialize");
    assert_eq!(report.skills_mode.as_deref(), Some("allowlist"));
    assert!(report.instructions_present);
    assert_eq!(
        report.instructions_chars,
        "never print fixture-secret-body".len()
    );
    assert!(!encoded.contains("fixture-secret-body"));
    assert!(encoded.contains("fixture-model"));
}

#[test]
fn current_inspection_reports_effective_base_and_no_profile_state() {
    let mut config = Config::default();
    config.provider.default_provider = Some("openai".to_owned());
    config.provider.default_model = Some("base-model".to_owned());
    let inspection = super::profile_inspection(&config, None)
        .expect("no-profile inspection should be provider-free");

    assert_eq!(inspection.profile_name, None);
    assert_eq!(
        inspection
            .effective
            .provider_model_reasoning
            .provider
            .as_deref(),
        Some("openai")
    );
    assert_eq!(
        inspection
            .effective
            .provider_model_reasoning
            .model
            .as_deref(),
        Some("base-model")
    );
    assert_eq!(
        inspection.sources.get("provider"),
        Some(&FieldSource::BaseConfig)
    );
    assert_eq!(
        inspection.sources.get("skill_policy"),
        Some(&FieldSource::BuiltInDefault)
    );
    let encoded = serde_json::to_string(&inspection).expect("inspection should serialize");
    assert!(!encoded.contains("api_key"));
}

#[test]
fn inspection_unknown_profile_has_available_name_guidance() {
    let mut config = Config::default();
    config
        .profiles
        .insert("review".to_owned(), SessionProfileConfig::default());
    let error = super::profile_show(&config, "missing").expect_err("unknown should fail");
    let message = error.to_string();
    assert!(message.contains("missing"));
    assert!(message.contains("review"));
    assert!(message.contains("available"));
}

#[test]
fn inspection_source_labels_keep_environment_above_profile() {
    let _env_lock = crate::storage::lock_test_env();
    let previous = std::env::var_os("JCODE_MODEL");
    crate::env::set_var("JCODE_MODEL", "environment-model");

    let mut config = Config::default();
    config.provider.default_model = Some("environment-model".to_owned());
    config.profiles.insert(
        "review".to_owned(),
        SessionProfileConfig {
            model: Some("profile-model".to_owned()),
            ..SessionProfileConfig::default()
        },
    );
    let inspection = super::profile_inspection(&config, Some("review"))
        .expect("environment-overridden profile should inspect");
    assert_eq!(
        inspection
            .effective
            .provider_model_reasoning
            .model
            .as_deref(),
        Some("environment-model")
    );
    assert_eq!(
        inspection.sources.get("model"),
        Some(&FieldSource::Environment)
    );

    if let Some(value) = previous {
        crate::env::set_var("JCODE_MODEL", value);
    } else {
        crate::env::remove_var("JCODE_MODEL");
    }
}
