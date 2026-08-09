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
    let base_provider = if args.provider != ProviderChoice::Auto {
        args.provider
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

    // Explicit invocation values win over matching profile fields without
    // mutating the persisted profile or the process-cached Config.
    if args.provider != ProviderChoice::Auto {
        resolved.provider = None;
    }
    if args.model.is_some() {
        resolved.model = None;
    }
    if args.provider_profile.is_some() {
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
    let base_provider_profile = args
        .provider_profile
        .as_deref()
        .or(environment_provider_profile.as_deref());
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
mod tests {
    use super::ProfileRunOptions;
    use crate::cli::args::Args;
    use crate::cli::provider_init::ProviderChoice;
    use crate::config::{Config, FieldSource, SessionProfileConfig, SkillsMode, ToolConfig};
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
        }
    }

    fn config_with_profile(profile: SessionProfileConfig) -> Config {
        let mut config = Config::default();
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
            "--provider-profile",
            "explicit-gateway",
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
        assert_eq!(
            options.provider_profile.as_deref(),
            Some("explicit-gateway")
        );
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
    fn profile_precedence_selected_profile_wins_over_base_when_invocation_is_unset() {
        let _env_lock = crate::storage::lock_test_env();
        let _provider_profile_env = ProviderProfileEnvGuard::set("environment-gateway");
        let mut config = config_with_profile(profile_with_values());
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
}
