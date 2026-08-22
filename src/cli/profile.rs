//! Local composition of a selected profile for one Jcode agent session.
//!
//! Profiles apply to interactive TUI launches and local plain, JSON, and NDJSON
//! one-shot runs. Administrative commands remain outside this session-scoped policy.

use anyhow::{Context, Result};
use clap::ValueEnum;

use super::provider_init::ProviderChoice;

#[derive(Debug, Clone, Default)]
pub struct RunProfileOverrides {
    pub provider: Option<ProviderChoice>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub provider_profile: Option<String>,
    pub tool_profile: Option<String>,
    pub tools: Option<Vec<String>>,
    pub disabled_tools: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct ResolvedRunProfile {
    pub name: String,
    pub provider: ProviderChoice,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub provider_profile: Option<String>,
    pub tools: crate::config::ToolConfig,
    pub skills: Vec<String>,
    pub instructions: Option<String>,
    pub prompt_overlay: crate::prompt::SessionPromptOverlay,
    profile_tool_references: Vec<String>,
    apply_reasoning_effort: bool,
}

pub struct ResolvedRunInvocation {
    pub provider: ProviderChoice,
    pub model: Option<String>,
    pub profile: Option<ResolvedRunProfile>,
}

fn non_empty_env(name: &str) -> Result<Option<String>> {
    let value = match std::env::var(name) {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => return Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            anyhow::bail!("environment variable {name} is not valid UTF-8")
        }
    };
    let value = value.trim().to_string();
    Ok((!value.is_empty()).then_some(value))
}

fn parse_provider(value: &str, profile_name: &str) -> Result<ProviderChoice> {
    ProviderChoice::from_str(value, true).map_err(|_| {
        anyhow::anyhow!(
            "profile '{profile_name}' has unsupported provider '{value}'; use a value accepted by --provider"
        )
    })
}

fn parse_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn reasoning_effort_env() -> Result<Option<String>> {
    match non_empty_env("JCODE_OPENAI_REASONING_EFFORT")? {
        some @ Some(_) => Ok(some),
        None => non_empty_env("JCODE_ANTHROPIC_REASONING_EFFORT"),
    }
}

#[derive(Debug, Clone, Default)]
struct RunProfileEnvironment {
    provider: Option<String>,
    model: Option<String>,
    reasoning_effort: Option<String>,
    provider_profile: Option<String>,
    tool_profile: Option<String>,
    tools: Option<Vec<String>>,
    disabled_tools: Option<Vec<String>>,
}

impl RunProfileEnvironment {
    fn current() -> Result<Self> {
        Ok(Self {
            provider: non_empty_env("JCODE_PROVIDER")?,
            model: non_empty_env("JCODE_MODEL")?,
            reasoning_effort: reasoning_effort_env()?,
            provider_profile: non_empty_env("JCODE_PROVIDER_PROFILE_NAME")?,
            tool_profile: non_empty_env("JCODE_TOOL_PROFILE")?,
            tools: non_empty_env("JCODE_TOOLS")?.map(|value| parse_list(&value)),
            disabled_tools: non_empty_env("JCODE_DISABLED_TOOLS")?.map(|value| parse_list(&value)),
        })
    }
}

fn compose_run_profile(
    name: &str,
    config: &crate::config::Config,
    profile: &crate::config::SessionProfileConfig,
    environment: RunProfileEnvironment,
    overrides: RunProfileOverrides,
) -> Result<ResolvedRunProfile> {
    let provider = crate::config::session_profile::resolve_sourced(
        overrides
            .provider
            .as_ref()
            .map(|provider| provider.as_arg_value().to_string()),
        environment.provider,
        profile.provider.clone(),
        config.provider.default_provider.clone(),
        "auto".to_string(),
    );
    let provider = parse_provider(&provider.value, name)?;

    let model = overrides
        .model
        .or(environment.model)
        .or_else(|| profile.model.clone())
        .or_else(|| config.provider.default_model.clone());
    let reasoning_effort = crate::config::session_profile::resolve_sourced(
        overrides.reasoning_effort.map(Some),
        environment.reasoning_effort.map(Some),
        profile.reasoning_effort.clone().map(Some),
        config
            .provider
            .openai_reasoning_effort
            .clone()
            .or_else(|| config.provider.anthropic_reasoning_effort.clone())
            .map(Some),
        None,
    );
    // Base provider configuration is applied during provider initialization.
    // Reapply only a profile, environment, or explicit invocation override.
    let apply_reasoning_effort = matches!(
        reasoning_effort.source,
        crate::config::session_profile::ProfileValueSource::Invocation
            | crate::config::session_profile::ProfileValueSource::Environment
            | crate::config::session_profile::ProfileValueSource::Profile
    );
    let reasoning_effort = reasoning_effort.value;
    let provider_profile = overrides
        .provider_profile
        .or(environment.provider_profile)
        .or_else(|| profile.provider_profile.clone());

    let mut tools = crate::config::session_profile::overlay_profile_tools(
        &config.tools,
        profile.tool_profile.as_deref(),
        &profile.tools,
        &profile.disabled_tools,
    );
    let mut profile_tool_references = Vec::new();
    if environment.tools.is_none() && overrides.tools.is_none() {
        profile_tool_references.extend(profile.tools.iter().cloned());
    }
    if environment.disabled_tools.is_none() && overrides.disabled_tools.is_none() {
        profile_tool_references.extend(profile.disabled_tools.iter().cloned());
    }
    if let Some(value) = overrides.tool_profile.or(environment.tool_profile) {
        tools.profile = value;
    }
    if let Some(value) = overrides.tools.or(environment.tools) {
        tools.enabled = value;
    }
    if let Some(value) = overrides.disabled_tools.or(environment.disabled_tools) {
        tools.disabled = value;
    }

    let mut selected_skills = Vec::with_capacity(profile.skills.len());
    if !profile.skills.is_empty() {
        let working_dir =
            std::env::current_dir().context("failed to resolve profile working directory")?;
        let skill_registry = crate::skill::SkillRegistry::load_for_working_dir(Some(&working_dir))
            .context("failed to load installed skills for selected session profile")?;
        for skill_name in &profile.skills {
            let skill = skill_registry.get(skill_name).ok_or_else(|| {
                let available = skill_registry
                    .list()
                    .into_iter()
                    .map(|skill| skill.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                anyhow::anyhow!(
                    "profile '{name}' references missing skill '{skill_name}'; install it or choose one of: {}",
                    if available.is_empty() { "none installed" } else { &available }
                )
            })?;
            selected_skills.push((skill_name.clone(), skill.get_prompt()));
        }
    }
    let prompt_overlay = crate::prompt::SessionPromptOverlay {
        instructions: profile.instructions.clone(),
        selected_skills,
    };

    Ok(ResolvedRunProfile {
        name: name.to_string(),
        provider,
        model,
        reasoning_effort,
        provider_profile,
        tools,
        skills: profile.skills.clone(),
        instructions: profile.instructions.clone(),
        prompt_overlay,
        profile_tool_references,
        apply_reasoning_effort,
    })
}

pub fn selected_reasoning_effort(profile: &ResolvedRunProfile) -> Option<&str> {
    profile
        .apply_reasoning_effort
        .then_some(profile.reasoning_effort.as_deref())
        .flatten()
}

pub fn validate_selected_tool_references(
    profile: &ResolvedRunProfile,
    available_tools: &[String],
) -> Result<()> {
    for name in &profile.profile_tool_references {
        if matches!(name.trim(), "*" | "all") {
            continue;
        }
        let normalized = crate::config::session_profile::normalize_tool_reference(name);
        if !available_tools
            .iter()
            .any(|available| available.eq_ignore_ascii_case(normalized))
        {
            anyhow::bail!(
                "profile '{}' references unknown tool '{}'; install or enable it, or choose one of: {}",
                profile.name,
                name,
                available_tools.join(", ")
            );
        }
    }
    Ok(())
}

/// Resolve one selected profile. The omitted-profile path returns before config
/// loading, profile lookup, tool composition, or environment inspection.
pub fn resolve_run_profile(
    selected_name: Option<&str>,
    overrides: RunProfileOverrides,
) -> Result<Option<ResolvedRunProfile>> {
    let Some(name) = selected_name else {
        return Ok(None);
    };

    let config = crate::config::Config::load_strict()
        .context("failed to load session profiles from config.toml")?;
    let profile = crate::config::session_profile::select_profile(&config.profiles, name)?;

    compose_run_profile(
        name,
        &config,
        profile,
        RunProfileEnvironment::current()?,
        overrides,
    )
    .map(Some)
}

pub(crate) fn resolve_run_invocation(
    selected_name: Option<&str>,
    args: &super::args::Args,
) -> Result<ResolvedRunInvocation> {
    let split_list = |value: &str| {
        value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    let profile = resolve_run_profile(
        selected_name,
        RunProfileOverrides {
            provider: args.provider_was_explicit.then_some(args.provider),
            model: args.model.clone(),
            reasoning_effort: args.reasoning_effort.clone(),
            provider_profile: args.provider_profile.clone(),
            tool_profile: args.tool_profile.clone(),
            tools: args.tools.as_deref().map(split_list),
            disabled_tools: args.disabled_tools.as_deref().map(split_list),
        },
    )?;
    if let Some(provider_profile) = profile
        .as_ref()
        .and_then(|profile| profile.provider_profile.as_deref())
    {
        crate::provider_catalog::apply_named_provider_profile_env(provider_profile)?;
        crate::env::set_var("JCODE_PROVIDER_PROFILE_NAME", provider_profile);
        crate::env::set_var("JCODE_PROVIDER_PROFILE_ACTIVE", "1");
    }
    let provider = profile
        .as_ref()
        .map(|profile| profile.provider)
        .unwrap_or(args.provider);
    let model = profile
        .as_ref()
        .and_then(|profile| profile.model.clone())
        .or(args.model.clone());

    Ok(ResolvedRunInvocation {
        provider,
        model,
        profile,
    })
}

pub(crate) fn apply_selected_profile_to_args(
    args: &mut super::args::Args,
) -> Result<Option<ResolvedRunProfile>> {
    let selected_name = args.profile.clone();
    let resolved = resolve_run_invocation(selected_name.as_deref(), args)?;
    let Some(profile) = resolved.profile else {
        return Ok(None);
    };

    args.provider = resolved.provider;
    args.provider_was_explicit = true;
    args.model = resolved.model;
    args.reasoning_effort = selected_reasoning_effort(&profile).map(str::to_string);
    args.provider_profile = profile.provider_profile.clone();
    args.tool_profile = Some(profile.tools.profile.clone());
    args.tools = Some(profile.tools.enabled.join(","));
    args.disabled_tools = Some(profile.tools.disabled.join(","));
    args.disable_base_tools = profile.tools.disable_base_tools;

    let overlay = serde_json::to_string(&profile.prompt_overlay)
        .context("failed to serialize selected session profile prompt overlay")?;
    crate::env::set_var(crate::prompt::SESSION_PROMPT_OVERLAY_ENV, overlay);
    Ok(Some(profile))
}

pub(crate) fn apply_agent_session_options(args: &mut super::args::Args) -> Result<()> {
    if let Some(profile_name) = args
        .provider_profile
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        crate::provider_catalog::apply_named_provider_profile_env(profile_name)?;
        crate::env::set_var("JCODE_PROVIDER_PROFILE_NAME", profile_name);
        crate::env::set_var("JCODE_PROVIDER_PROFILE_ACTIVE", "1");
        args.provider = ProviderChoice::OpenaiCompatible;
    }

    if let Some(reasoning_effort) = args.reasoning_effort.as_deref() {
        crate::env::set_var("JCODE_OPENAI_REASONING_EFFORT", reasoning_effort);
        crate::env::set_var("JCODE_ANTHROPIC_REASONING_EFFORT", reasoning_effort);
    }
    if let Some(tool_profile) = args.tool_profile.as_deref() {
        crate::env::set_var("JCODE_TOOL_PROFILE", tool_profile);
    }
    if let Some(tools) = args.tools.as_deref() {
        crate::env::set_var("JCODE_TOOLS", tools);
    }
    if let Some(disabled_tools) = args.disabled_tools.as_deref() {
        crate::env::set_var("JCODE_DISABLED_TOOLS", disabled_tools);
    }
    if args.disable_base_tools {
        crate::env::set_var("JCODE_DISABLE_BASE_TOOLS", "1");
    }
    if let Some(mcp_tools) = args.mcp_tools.as_deref() {
        crate::env::set_var("JCODE_MCP_TOOLS", mcp_tools);
    }
    if let Some(threshold) = args.mcp_tools_token_threshold {
        crate::env::set_var("JCODE_MCP_TOOLS_TOKEN_THRESHOLD", threshold.to_string());
    }
    if args.tool_profile.is_some()
        || args.tools.is_some()
        || args.disabled_tools.is_some()
        || args.disable_base_tools
        || args.mcp_tools.is_some()
        || args.mcp_tools_token_threshold.is_some()
    {
        crate::config::invalidate_config_cache();
    }

    Ok(())
}

pub(crate) async fn run_profiled_single_message(
    args: &super::args::Args,
    message: &str,
    json: bool,
    ndjson: bool,
    profile: Option<ResolvedRunProfile>,
) -> Result<()> {
    super::commands::run_single_message_command(
        args.provider,
        args.model.clone(),
        args.resume.as_deref(),
        message,
        json,
        ndjson,
        profile,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn non_utf8_profile_environment_value_is_rejected() {
        use std::os::unix::ffi::OsStringExt;

        let _guard = crate::storage::lock_test_env();
        const NAME: &str = "JCODE_TEST_NON_UTF8_PROFILE_ENV";
        crate::env::set_var(NAME, std::ffi::OsString::from_vec(vec![0xff]));
        let result = non_empty_env(NAME);
        crate::env::remove_var(NAME);

        let error = result.expect_err("non-UTF-8 environment value must fail");
        assert!(error.to_string().contains(NAME));
        assert!(error.to_string().contains("not valid UTF-8"));
    }

    #[cfg(unix)]
    #[test]
    fn openai_reasoning_environment_precedes_invalid_anthropic_fallback() {
        use std::os::unix::ffi::OsStringExt;

        let _guard = crate::storage::lock_test_env();
        let openai = std::env::var_os("JCODE_OPENAI_REASONING_EFFORT");
        let anthropic = std::env::var_os("JCODE_ANTHROPIC_REASONING_EFFORT");
        crate::env::set_var("JCODE_OPENAI_REASONING_EFFORT", "high");
        crate::env::set_var(
            "JCODE_ANTHROPIC_REASONING_EFFORT",
            std::ffi::OsString::from_vec(vec![0xff]),
        );

        let resolved = reasoning_effort_env();
        match openai {
            Some(value) => crate::env::set_var("JCODE_OPENAI_REASONING_EFFORT", value),
            None => crate::env::remove_var("JCODE_OPENAI_REASONING_EFFORT"),
        }
        match anthropic {
            Some(value) => crate::env::set_var("JCODE_ANTHROPIC_REASONING_EFFORT", value),
            None => crate::env::remove_var("JCODE_ANTHROPIC_REASONING_EFFORT"),
        }

        assert_eq!(
            resolved
                .expect("higher-precedence value should win")
                .as_deref(),
            Some("high")
        );
    }

    fn complete_profile() -> crate::config::SessionProfileConfig {
        crate::config::SessionProfileConfig {
            provider: Some("openrouter".to_string()),
            model: Some("profile-model".to_string()),
            reasoning_effort: Some("medium".to_string()),
            provider_profile: Some("profile-gateway".to_string()),
            tool_profile: Some("minimal".to_string()),
            tools: vec!["read".to_string()],
            disabled_tools: vec!["bash".to_string()],
            ..Default::default()
        }
    }

    #[test]
    fn canonical_precedence_is_invocation_environment_profile_base_default() {
        let mut config = crate::config::Config::default();
        config.provider.default_provider = Some("anthropic".to_string());
        config.provider.default_model = Some("base-model".to_string());
        config.provider.openai_reasoning_effort = Some("low".to_string());
        config.tools.profile = "full".to_string();
        config.tools.enabled = vec!["write".to_string()];

        let resolved = compose_run_profile(
            "review",
            &config,
            &complete_profile(),
            RunProfileEnvironment {
                provider: Some("openai".to_string()),
                model: Some("environment-model".to_string()),
                reasoning_effort: Some("high".to_string()),
                provider_profile: Some("environment-gateway".to_string()),
                tool_profile: Some("acp".to_string()),
                tools: Some(vec!["agentgrep".to_string()]),
                disabled_tools: Some(vec!["write".to_string()]),
            },
            RunProfileOverrides {
                provider: Some(ProviderChoice::Auto),
                model: Some("invocation-model".to_string()),
                reasoning_effort: Some("xhigh".to_string()),
                provider_profile: Some("invocation-gateway".to_string()),
                tool_profile: Some("none".to_string()),
                tools: Some(vec!["read".to_string(), "agentgrep".to_string()]),
                disabled_tools: Some(vec!["bash".to_string()]),
            },
        )
        .unwrap();

        assert_eq!(resolved.provider.as_arg_value(), "auto");
        assert_eq!(resolved.model.as_deref(), Some("invocation-model"));
        assert_eq!(resolved.reasoning_effort.as_deref(), Some("xhigh"));
        assert_eq!(
            resolved.provider_profile.as_deref(),
            Some("invocation-gateway")
        );
        assert_eq!(resolved.tools.profile, "none");
        assert_eq!(resolved.tools.enabled, ["read", "agentgrep"]);
        assert_eq!(resolved.tools.disabled, ["bash"]);
    }

    #[test]
    fn lower_precedence_sources_fill_only_omitted_fields() {
        let mut config = crate::config::Config::default();
        config.provider.default_model = Some("base-model".to_string());
        let mut profile = complete_profile();
        profile.model = None;

        let resolved = compose_run_profile(
            "review",
            &config,
            &profile,
            RunProfileEnvironment {
                provider: Some("openai".to_string()),
                ..Default::default()
            },
            RunProfileOverrides::default(),
        )
        .unwrap();

        assert_eq!(resolved.provider.as_arg_value(), "openai");
        assert_eq!(resolved.model.as_deref(), Some("base-model"));
        assert_eq!(resolved.reasoning_effort.as_deref(), Some("medium"));
        assert_eq!(resolved.tools.profile, "minimal");
        assert_eq!(resolved.tools.enabled, ["read"]);
        assert!(resolved.tools.disabled.contains(&"bash".to_string()));
    }

    #[test]
    fn selected_missing_skill_fails_with_profile_field_and_correction() {
        let config = crate::config::Config::default();
        let profile = crate::config::SessionProfileConfig {
            skills: vec!["definitely-not-installed-session-profile-skill".to_string()],
            ..Default::default()
        };
        let error = compose_run_profile(
            "review",
            &config,
            &profile,
            RunProfileEnvironment::default(),
            RunProfileOverrides::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("review"));
        assert!(error.contains("definitely-not-installed-session-profile-skill"));
        assert!(error.contains("install"));
    }

    #[test]
    fn selected_tool_references_accept_existing_canonical_aliases() {
        let config = crate::config::Config::default();
        let profile = crate::config::SessionProfileConfig {
            tools: vec!["grep".to_string()],
            disabled_tools: vec!["shell_exec".to_string()],
            ..Default::default()
        };
        let resolved = compose_run_profile(
            "review",
            &config,
            &profile,
            RunProfileEnvironment::default(),
            RunProfileOverrides::default(),
        )
        .unwrap();

        validate_selected_tool_references(
            &resolved,
            &["agentgrep".to_string(), "bash".to_string()],
        )
        .expect("ToolConfig aliases should validate against canonical registry names");
    }

    #[test]
    fn base_reasoning_value_remains_owned_by_provider_initialization() {
        let mut config = crate::config::Config::default();
        config.provider.openai_reasoning_effort = Some("low".to_string());
        let resolved = compose_run_profile(
            "review",
            &config,
            &crate::config::SessionProfileConfig::default(),
            RunProfileEnvironment::default(),
            RunProfileOverrides::default(),
        )
        .unwrap();
        assert_eq!(resolved.reasoning_effort.as_deref(), Some("low"));
        assert_eq!(selected_reasoning_effort(&resolved), None);
    }
}
