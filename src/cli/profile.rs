//! CLI-facing session-profile conversion contract.
//!
//! Profile values become an immutable per-run bundle, while the persisted
//! config and process environment remain untouched.

use clap::ValueEnum;
use serde::Serialize;
use std::collections::BTreeMap;

use crate::cli::args::Args;
use crate::cli::provider_init::ProviderChoice;
use crate::config::{
    Config, FieldSource, ProfileInspectionResult, ResolvedSessionProfile, SessionProfileConfig,
    SessionPromptOverlay, SkillsMode, ToolConfig, ToolSelection,
    active_environment_provider_profile,
};
use crate::protocol::SessionProfileStartup;
use crate::skill::SkillRegistry;

/// One stable, secret-free entry returned by `jcode profile list`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct ProfileListEntry {
    pub(crate) name: String,
    pub(crate) active: bool,
}

/// Stable profile list projection used by plain and structured CLI output.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct ProfileListReport {
    pub(crate) current: Option<String>,
    pub(crate) profiles: Vec<ProfileListEntry>,
}

/// Configured profile fields safe for inspection. Instruction bodies and
/// provider credentials are intentionally represented by presence/length
/// metadata only.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct ProfileShowReport {
    pub(crate) name: String,
    pub(crate) provider: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) provider_profile: Option<String>,
    pub(crate) tool_profile: Option<String>,
    pub(crate) tools: Vec<String>,
    pub(crate) disabled_tools: Vec<String>,
    pub(crate) skills_mode: Option<String>,
    pub(crate) skills: Vec<String>,
    pub(crate) disabled_skills: Vec<String>,
    pub(crate) instructions_present: bool,
    pub(crate) instructions_chars: usize,
}

pub(crate) fn profile_list(config: &Config, current: Option<&str>) -> ProfileListReport {
    let current = current
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned);
    let profiles = config
        .profiles
        .keys()
        .map(|name| ProfileListEntry {
            active: current.as_deref() == Some(name.as_str()),
            name: name.clone(),
        })
        .collect();
    ProfileListReport { current, profiles }
}

pub(crate) fn profile_show(config: &Config, name: &str) -> anyhow::Result<ProfileShowReport> {
    let profile = config.profiles.get(name).ok_or_else(|| {
        anyhow::anyhow!(
            "Unknown session profile '{}'; available profiles: {}",
            name,
            available_profile_names(config)
        )
    })?;
    Ok(ProfileShowReport::from_config(name, profile))
}

impl ProfileShowReport {
    fn from_config(name: &str, profile: &SessionProfileConfig) -> Self {
        Self {
            name: name.to_owned(),
            provider: profile.provider.clone(),
            model: profile.model.clone(),
            reasoning_effort: profile.reasoning_effort.clone(),
            provider_profile: profile.provider_profile.clone(),
            tool_profile: profile.tool_profile.clone(),
            tools: profile.tools.clone(),
            disabled_tools: profile.disabled_tools.clone(),
            skills_mode: profile
                .skills_mode
                .map(SkillsMode::as_str)
                .map(str::to_owned),
            skills: profile.skills.clone(),
            disabled_skills: profile.disabled_skills.clone(),
            instructions_present: profile
                .instructions
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty()),
            instructions_chars: profile.instructions.as_deref().map_or(0, str::len),
        }
    }
}

/// Resolve the effective, credential-free values used by inspection commands.
/// This is an adapter over the canonical profile resolver; it never starts a
/// provider and never mutates config or session state.
pub(crate) fn profile_inspection(
    config: &Config,
    selected: Option<&str>,
) -> anyhow::Result<ProfileInspectionResult> {
    let resolved = config.resolve_session_profile_with_environment(selected)?;
    let original = resolved.clone();
    let mut effective = resolved;
    apply_base_effective_values(config, &mut effective);
    let mut inspection = effective.inspection(&config.tools);
    inspection.sources = inspection_sources(config, selected, &original);
    Ok(inspection)
}

fn apply_base_effective_values(config: &Config, resolved: &mut ResolvedSessionProfile) {
    let provider = resolved
        .provider
        .clone()
        .or_else(|| config.provider.default_provider.clone())
        .or_else(|| Some("auto".to_owned()));
    let model = resolved
        .model
        .clone()
        .or_else(|| config.provider.default_model.clone());
    let provider_profile = resolved
        .provider_profile
        .clone()
        .or_else(active_environment_provider_profile);
    let reasoning_effort = resolved.reasoning_effort.clone().or_else(|| {
        let provider = provider.as_deref().unwrap_or("");
        if provider.eq_ignore_ascii_case("openai") || provider.eq_ignore_ascii_case("openai-api") {
            config.provider.openai_reasoning_effort.clone()
        } else if provider.eq_ignore_ascii_case("claude")
            || provider.eq_ignore_ascii_case("anthropic-api")
        {
            config.provider.anthropic_reasoning_effort.clone()
        } else {
            None
        }
    });
    resolved.provider = provider;
    resolved.model = model;
    resolved.provider_profile = provider_profile;
    resolved.reasoning_effort = reasoning_effort;
}

fn inspection_sources(
    config: &Config,
    selected: Option<&str>,
    original: &ResolvedSessionProfile,
) -> BTreeMap<String, FieldSource> {
    let profile = selected.and_then(|name| config.profiles.get(name));
    let mut sources = BTreeMap::new();
    sources.insert(
        "provider".to_owned(),
        field_source(
            profile.and_then(|profile| profile.provider.as_ref()),
            original.provider.is_none() && env_non_empty("JCODE_PROVIDER"),
            config.provider.default_provider.is_some(),
        ),
    );
    sources.insert(
        "model".to_owned(),
        field_source(
            profile.and_then(|profile| profile.model.as_ref()),
            original.model.is_none() && std::env::var_os("JCODE_MODEL").is_some(),
            config.provider.default_model.is_some(),
        ),
    );
    sources.insert(
        "reasoning_effort".to_owned(),
        field_source(
            profile.and_then(|profile| profile.reasoning_effort.as_ref()),
            original.reasoning_effort.is_none()
                && (env_non_empty("JCODE_OPENAI_REASONING_EFFORT")
                    || env_non_empty("JCODE_ANTHROPIC_REASONING_EFFORT")),
            config.provider.openai_reasoning_effort.is_some()
                || config.provider.anthropic_reasoning_effort.is_some(),
        ),
    );
    sources.insert(
        "provider_profile".to_owned(),
        field_source(
            profile.and_then(|profile| profile.provider_profile.as_ref()),
            original.provider_profile.is_none() && active_environment_provider_profile().is_some(),
            false,
        ),
    );
    let profile_tools = profile.is_some_and(|profile| {
        profile.tool_profile.is_some()
            || !profile.tools.is_empty()
            || !profile.disabled_tools.is_empty()
    });
    sources.insert(
        "tool_policy".to_owned(),
        field_source_flag(
            profile_tools,
            !profile_tools
                && (std::env::var_os("JCODE_TOOL_PROFILE").is_some()
                    || std::env::var_os("JCODE_TOOLS").is_some()
                    || std::env::var_os("JCODE_DISABLED_TOOLS").is_some()),
            true,
        ),
    );
    let profile_skills = profile.is_some_and(|profile| {
        profile.skills_mode.is_some()
            || !profile.skills.is_empty()
            || !profile.disabled_skills.is_empty()
    });
    sources.insert(
        "skill_policy".to_owned(),
        if profile_skills {
            FieldSource::Profile
        } else {
            FieldSource::BuiltInDefault
        },
    );
    sources.insert(
        "prompt_overlay".to_owned(),
        if profile.is_some_and(|profile| {
            profile
                .instructions
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
                || !profile.skills.is_empty()
        }) {
            FieldSource::Profile
        } else {
            FieldSource::BuiltInDefault
        },
    );
    sources
}

fn field_source<T>(profile_value: Option<&T>, environment: bool, base_config: bool) -> FieldSource {
    if environment {
        FieldSource::Environment
    } else if profile_value.is_some() {
        FieldSource::Profile
    } else if base_config {
        FieldSource::BaseConfig
    } else {
        FieldSource::BuiltInDefault
    }
}

fn field_source_flag(profile: bool, environment: bool, base_config: bool) -> FieldSource {
    if environment {
        FieldSource::Environment
    } else if profile {
        FieldSource::Profile
    } else if base_config {
        FieldSource::BaseConfig
    } else {
        FieldSource::BuiltInDefault
    }
}

fn env_non_empty(key: &str) -> bool {
    std::env::var(key).is_ok_and(|value| !value.trim().is_empty())
}

fn available_profile_names(config: &Config) -> String {
    if config.profiles.is_empty() {
        "(none)".to_owned()
    } else {
        config
            .profiles
            .keys()
            .map(|name| format!("'{name}'"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Effective values passed from profile resolution into one `jcode run`.
///
/// This is intentionally data-only. Provider initialization, Agent creation,
/// and prompt rendering consume this value in their owning layers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProfileRunOptions {
    pub(crate) profile_name: Option<String>,
    profile_tool_profile: Option<String>,
    profile_tool_names: Vec<String>,
    profile_disabled_tool_names: Vec<String>,
    profile_skills_mode: Option<SkillsMode>,
    profile_disabled_skills: Vec<String>,
    pub(crate) provider: ProviderChoice,
    pub(crate) model: Option<String>,
    pub(crate) provider_profile: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) tool_selection: ToolSelection,
    pub(crate) prompt_overlay: SessionPromptOverlay,
}

impl ProfileRunOptions {
    /// Convert the resolved, non-secret session policy into the optional wire
    /// overlay consumed by a new interactive Agent.
    pub(crate) fn startup_metadata(&self) -> SessionProfileStartup {
        let mut allowed_tools = self
            .tool_selection
            .allowed_tools
            .as_ref()
            .map(|tools| tools.iter().cloned().collect::<Vec<_>>());
        if let Some(tools) = allowed_tools.as_mut() {
            tools.sort_unstable();
        }
        let mut disabled_tools = self
            .tool_selection
            .disabled_tools
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        disabled_tools.sort_unstable();
        SessionProfileStartup {
            profile_name: self.profile_name.clone(),
            provider: Some(self.provider.as_arg_value().to_owned()),
            model: self.model.clone(),
            provider_profile: self.provider_profile.clone(),
            reasoning_effort: self.reasoning_effort.clone(),
            allowed_tools,
            disabled_tools,
            skill_names: self.prompt_overlay.skill_names.clone(),
            skills_mode: self.profile_skills_mode,
            disabled_skills: self.profile_disabled_skills.clone(),
            skill_prompts: self.prompt_overlay.skill_prompts.clone(),
            instructions: self.prompt_overlay.instructions.clone(),
        }
    }

    pub(crate) fn validate_tool_names<I, S>(&self, available_names: I) -> anyhow::Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let Some(profile_name) = self.profile_name.as_deref() else {
            return Ok(());
        };
        let resolved = ResolvedSessionProfile {
            profile_name: Some(profile_name.to_owned()),
            tool_profile: self.profile_tool_profile.clone(),
            tools: self.profile_tool_names.clone(),
            disabled_tools: self.profile_disabled_tool_names.clone(),
            ..ResolvedSessionProfile::default()
        };
        resolved.validate_tool_names(available_names)
    }

    /// Merge one immutable profile overlay onto the values already selected by
    /// the existing CLI/config path.
    ///
    /// The caller supplies the current effective base values, so an absent
    /// profile remains byte-for-byte on the legacy path. Profile values only
    /// replace their corresponding field and the resulting tool selection is
    /// still derived by `ToolConfig`, preserving its profile/allow/deny
    /// semantics.
    pub(crate) fn from_resolved_profile(
        base_provider: ProviderChoice,
        base_model: Option<&str>,
        base_provider_profile: Option<&str>,
        base_reasoning_effort: Option<&str>,
        base_tools: &ToolConfig,
        resolved: &ResolvedSessionProfile,
    ) -> anyhow::Result<Self> {
        let provider = match resolved.provider.as_deref() {
            Some(provider) => ProviderChoice::from_str(provider, true).map_err(|error| {
                let profile_name = resolved.profile_name.as_deref().unwrap_or("(unnamed)");
                anyhow::anyhow!(
                    "Session profile '{}' has invalid provider '{}': {}",
                    profile_name,
                    provider,
                    error
                )
            })?,
            None => base_provider,
        };

        let model = resolved
            .model
            .clone()
            .or_else(|| base_model.map(str::to_owned));
        let provider_profile = resolved
            .provider_profile
            .clone()
            .or_else(|| base_provider_profile.map(str::to_owned));
        let reasoning_effort = resolved
            .reasoning_effort
            .clone()
            .or_else(|| base_reasoning_effort.map(str::to_owned));

        Ok(Self {
            profile_name: resolved.profile_name.clone(),
            profile_tool_profile: resolved.tool_profile.clone(),
            profile_tool_names: resolved.tools.clone(),
            profile_disabled_tool_names: resolved.disabled_tools.clone(),
            profile_skills_mode: resolved.skill_policy.mode,
            profile_disabled_skills: resolved.skill_policy.disabled_skills.clone(),
            provider,
            model,
            provider_profile,
            reasoning_effort,
            tool_selection: resolved.tool_selection(base_tools),
            prompt_overlay: resolved.prompt_overlay.clone(),
        })
    }
}

pub(crate) fn named_provider_choice(
    config: &Config,
    provider_profile: &str,
) -> anyhow::Result<ProviderChoice> {
    let profile = config.providers.get(provider_profile).ok_or_else(|| {
        anyhow::anyhow!(
            "Unknown provider profile '{}'. Add [providers.{}] to config.toml.",
            provider_profile,
            provider_profile
        )
    })?;
    Ok(
        if matches!(
            profile.provider_type,
            crate::config::NamedProviderType::AnthropicCompatible
        ) {
            ProviderChoice::AnthropicApi
        } else {
            ProviderChoice::OpenaiCompatible
        },
    )
}

pub(crate) fn validate_explicit_provider_selectors(args: &Args) -> anyhow::Result<()> {
    let Some(provider_profile) = args.provider_profile.as_deref() else {
        return Ok(());
    };
    if args.provider == ProviderChoice::Auto {
        return Ok(());
    }

    anyhow::bail!(
        "--provider {} conflicts with --provider-profile {}; remove one provider selector",
        args.provider.as_arg_value(),
        provider_profile
    )
}

pub(crate) fn validate_initial_provider_selectors(args: &Args) -> anyhow::Result<bool> {
    let explicit_provider_profile = args.provider_profile.is_some();
    if explicit_provider_profile && args.provider != ProviderChoice::Auto {
        validate_explicit_provider_selectors(args)?;
    }
    Ok(explicit_provider_profile)
}

pub(crate) fn activate_named_provider_profile(
    provider_profile: &str,
) -> anyhow::Result<ProviderChoice> {
    let config = Config::load_strict()?;
    let provider = named_provider_choice(&config, provider_profile)?;
    crate::provider_catalog::apply_named_provider_profile_env(provider_profile)?;
    crate::env::set_var("JCODE_PROVIDER_PROFILE_NAME", provider_profile);
    crate::env::set_var("JCODE_PROVIDER_PROFILE_ACTIVE", "1");
    Ok(provider)
}

/// Resolve a selected profile before `jcode run` initializes a provider.
///
/// This consumes an owned strict [`Config`] supplied by dispatch and returns
/// an immutable per-invocation bundle. The no-profile path does no lookup or
/// configuration work, preserving legacy dispatch behavior.
pub(crate) fn resolve_run_options(
    args: &Args,
    config: &Config,
) -> anyhow::Result<Option<ProfileRunOptions>> {
    let Some(profile_name) = args.profile.as_deref() else {
        return Ok(None);
    };

    validate_explicit_provider_selectors(args)?;

    let mut resolved = config
        .resolve_session_profile_with_environment(Some(profile_name))
        .map_err(annotate_profile_resolution_error)?;
    if !resolved.prompt_overlay.skill_names.is_empty() {
        let working_dir = std::env::current_dir()?;
        let registry =
            SkillRegistry::load_for_working_dir(Some(&working_dir)).map_err(|error| {
                anyhow::anyhow!(
                    "Session profile '{}' could not load its selected skills from {}: {}",
                    profile_name,
                    working_dir.display(),
                    error
                )
            })?;
        resolved.prompt_overlay.skill_prompts =
            registry.render_profile_skills(profile_name, &resolved.prompt_overlay.skill_names)?;
    }
    // Explicit invocation values win over matching profile fields without
    // mutating the persisted profile or the process-cached Config.
    if args.provider != ProviderChoice::Auto {
        resolved.provider = None;
        resolved.provider_profile = None;
    }
    if args.model.is_some() {
        resolved.model = None;
    }
    if args.provider_profile.is_some() {
        resolved.provider = None;
        resolved.provider_profile = None;
    }
    if args.reasoning_effort.is_some() {
        resolved.reasoning_effort = None;
    }

    let base_model = args
        .model
        .as_deref()
        .or(config.provider.default_model.as_deref());
    let environment_provider_profile = active_environment_provider_profile();
    let base_provider_profile = args.provider_profile.as_deref().or_else(|| {
        (args.provider == ProviderChoice::Auto)
            .then_some(environment_provider_profile.as_deref())
            .flatten()
    });
    let effective_provider_profile = resolved
        .provider_profile
        .as_deref()
        .or(base_provider_profile);
    let base_provider = if args.provider != ProviderChoice::Auto {
        args.provider
    } else if let Some(provider_profile) = effective_provider_profile {
        named_provider_choice(config, provider_profile)?
    } else if let Some(provider_name) = config
        .provider
        .default_provider
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        // Custom names under [providers.<name>] remain with the existing
        // auto-provider resolver; built-in names can seed this run bundle.
        match ProviderChoice::from_str(provider_name, true) {
            Ok(provider) => provider,
            Err(_) => ProviderChoice::Auto,
        }
    } else {
        ProviderChoice::Auto
    };
    let reasoning_provider = if base_provider == ProviderChoice::Auto {
        match resolved.provider.as_deref() {
            Some(provider) => match ProviderChoice::from_str(provider, true) {
                Ok(provider) => provider,
                Err(_) => base_provider,
            },
            None => base_provider,
        }
    } else {
        base_provider
    };
    let base_reasoning_effort = args
        .reasoning_effort
        .as_deref()
        .or(match reasoning_provider {
            ProviderChoice::Openai | ProviderChoice::OpenaiApi => {
                config.provider.openai_reasoning_effort.as_deref()
            }
            ProviderChoice::Claude | ProviderChoice::AnthropicApi => {
                config.provider.anthropic_reasoning_effort.as_deref()
            }
            _ => None,
        });

    let mut base_tools = config.tools.clone();
    if let Some(tool_profile) = args.tool_profile.as_deref() {
        base_tools.profile = tool_profile.to_owned();
        resolved.tool_profile = None;
    }
    if let Some(tools) = args.tools.as_deref() {
        base_tools.enabled = tools
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect();
        resolved.tools.clear();
    }
    if let Some(disabled_tools) = args.disabled_tools.as_deref() {
        base_tools.disabled = disabled_tools
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect();
        resolved.disabled_tools.clear();
    }
    if args.disable_base_tools {
        base_tools.disable_base_tools = true;
    }

    Ok(Some(ProfileRunOptions::from_resolved_profile(
        base_provider,
        base_model,
        base_provider_profile,
        base_reasoning_effort,
        &base_tools,
        &resolved,
    )?))
}

fn annotate_profile_resolution_error(error: anyhow::Error) -> anyhow::Error {
    let message = error.to_string();
    if message.starts_with("Unknown session profile") {
        let location = Config::path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "config.toml".to_owned());
        anyhow::anyhow!("{message} Configuration location: {location}")
    } else {
        error
    }
}

/// Apply selected run values to this invocation without saving configuration.
pub(crate) fn apply_run_options(args: &mut Args) -> anyhow::Result<Option<ProfileRunOptions>> {
    if args.profile.is_none()
        || !matches!(&args.command, Some(crate::cli::args::Command::Run { .. }))
    {
        return Ok(None);
    }
    let config = Config::load_strict()?;
    if let Some(options) = resolve_run_options(args, &config)? {
        args.provider = options.provider;
        args.model = options.model.clone();
        args.provider_profile = options.provider_profile.clone();
        args.reasoning_effort = options.reasoning_effort.clone();
        return Ok(Some(options));
    }
    Ok(None)
}

/// Resolve and apply a profile for an interactive TUI startup. This is kept
/// separate from `apply_run_options` so the established headless dispatch path
/// remains unchanged when no interactive profile was requested.
pub(crate) fn resolve_interactive_options(
    args: &Args,
    config: &Config,
) -> anyhow::Result<Option<ProfileRunOptions>> {
    if args.profile.is_none() || args.command.is_some() {
        return Ok(None);
    }
    resolve_run_options(args, config)
}

pub(crate) fn apply_interactive_options(
    args: &mut Args,
) -> anyhow::Result<Option<ProfileRunOptions>> {
    if args.profile.is_none() || args.command.is_some() {
        return Ok(None);
    }
    let config = Config::load_strict()?;
    if let Some(options) = resolve_interactive_options(args, &config)? {
        args.provider = options.provider;
        args.model = options.model.clone();
        args.provider_profile = options.provider_profile.clone();
        args.reasoning_effort = options.reasoning_effort.clone();
        return Ok(Some(options));
    }
    Ok(None)
}

#[cfg(test)]
#[path = "profile/tests.rs"]
mod tests;
