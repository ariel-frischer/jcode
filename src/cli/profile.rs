//! Local composition of a selected session profile for one headless `jcode run`.
//!
//! Profile selection is intentionally limited to the local plain, JSON, and
//! NDJSON one-shot paths. It does not alter daemon protocol, ACP, SDK, interactive,
//! persistence, child-session, or swarm behavior.

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

fn non_empty_env(name: &str) -> Option<String> {
    let value = match std::env::var(name) {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => return None,
        Err(std::env::VarError::NotUnicode(_)) => return None,
    };
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
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
    fn current() -> Self {
        Self {
            provider: non_empty_env("JCODE_PROVIDER"),
            model: non_empty_env("JCODE_MODEL"),
            reasoning_effort: non_empty_env("JCODE_OPENAI_REASONING_EFFORT")
                .or_else(|| non_empty_env("JCODE_ANTHROPIC_REASONING_EFFORT")),
            provider_profile: non_empty_env("JCODE_PROVIDER_PROFILE_NAME"),
            tool_profile: non_empty_env("JCODE_TOOL_PROFILE"),
            tools: non_empty_env("JCODE_TOOLS").map(|value| parse_list(&value)),
            disabled_tools: non_empty_env("JCODE_DISABLED_TOOLS").map(|value| parse_list(&value)),
        }
    }
}

fn compose_run_profile(
    name: &str,
    config: &crate::config::Config,
    profile: &crate::config::SessionProfileConfig,
    environment: RunProfileEnvironment,
    overrides: RunProfileOverrides,
) -> Result<ResolvedRunProfile> {
    let provider_text = overrides
        .provider
        .as_ref()
        .map(|provider| provider.as_arg_value().to_string())
        .or(environment.provider)
        .or_else(|| profile.provider.clone())
        .or_else(|| config.provider.default_provider.clone())
        .unwrap_or_else(|| "auto".to_string());
    let provider = parse_provider(&provider_text, name)?;

    let model = overrides
        .model
        .or(environment.model)
        .or_else(|| profile.model.clone())
        .or_else(|| config.provider.default_model.clone());
    let apply_reasoning_effort = overrides.reasoning_effort.is_some()
        || environment.reasoning_effort.is_some()
        || profile.reasoning_effort.is_some();
    let reasoning_effort = overrides
        .reasoning_effort
        .or(environment.reasoning_effort)
        .or_else(|| profile.reasoning_effort.clone())
        .or_else(|| config.provider.openai_reasoning_effort.clone())
        .or_else(|| config.provider.anthropic_reasoning_effort.clone());
    let provider_profile = overrides
        .provider_profile
        .or(environment.provider_profile)
        .or_else(|| profile.provider_profile.clone());

    let mut tools = config.tools.clone();
    let mut profile_tool_references = Vec::new();
    if environment.tool_profile.is_none() && overrides.tool_profile.is_none() {
        if let Some(profile_name) = profile.tool_profile.as_deref() {
            tools.profile = profile_name.to_string();
        }
    }
    if environment.tools.is_none() && overrides.tools.is_none() && !profile.tools.is_empty() {
        tools.enabled = profile.tools.clone();
        profile_tool_references.extend(profile.tools.iter().cloned());
    }
    if environment.disabled_tools.is_none()
        && overrides.disabled_tools.is_none()
        && !profile.disabled_tools.is_empty()
    {
        tools
            .disabled
            .extend(profile.disabled_tools.iter().cloned());
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
        .then(|| profile.reasoning_effort.as_deref())
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
        let normalized = name.trim().to_ascii_lowercase();
        if !available_tools
            .iter()
            .any(|available| available.eq_ignore_ascii_case(&normalized))
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
        RunProfileEnvironment::current(),
        overrides,
    )
    .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn maintained_profile_resolution_efficiency_budgets() {
        use crate::config::session_profile::{
            OMITTED_PROFILE_BUDGET_CALLS, OMITTED_PROFILE_BUDGET_MS, SELECTED_PROFILE_BUDGET_CALLS,
            SELECTED_PROFILE_BUDGET_MS,
        };
        use std::time::Instant;

        let omitted_start = Instant::now();
        for _ in 0..OMITTED_PROFILE_BUDGET_CALLS {
            assert!(
                resolve_run_profile(None, RunProfileOverrides::default())
                    .unwrap()
                    .is_none()
            );
        }
        assert!(
            omitted_start.elapsed().as_millis() <= OMITTED_PROFILE_BUDGET_MS,
            "omitted-profile fast path exceeded {} ms: {:?}",
            OMITTED_PROFILE_BUDGET_MS,
            omitted_start.elapsed()
        );

        let config = crate::config::Config::default();
        let profile = complete_profile();
        let selected_start = Instant::now();
        for _ in 0..SELECTED_PROFILE_BUDGET_CALLS {
            compose_run_profile(
                "review",
                &config,
                &profile,
                RunProfileEnvironment::default(),
                RunProfileOverrides::default(),
            )
            .unwrap();
        }
        assert!(
            selected_start.elapsed().as_millis() <= SELECTED_PROFILE_BUDGET_MS,
            "selected-profile resolution exceeded {} ms: {:?}",
            SELECTED_PROFILE_BUDGET_MS,
            selected_start.elapsed()
        );
    }
}
