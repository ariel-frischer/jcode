//! Named session-profile loading and resolution for headless `jcode run`.
//!
//! This module owns strict, source-aware profile validation and resolution while
//! composing tool policy through the existing [`super::ToolConfig`] semantics.
//! Diagnostics produced here must remain actionable without exposing credentials
//! or complete user-authored profile instructions.

use super::{SessionProfileConfig, ToolConfig};
use anyhow::{Result, bail};
use std::collections::BTreeMap;

pub const OMITTED_PROFILE_BUDGET_CALLS: usize = 10_000;
pub const OMITTED_PROFILE_BUDGET_MS: u128 = 100;
pub const SELECTED_PROFILE_BUDGET_CALLS: usize = 1_000;
pub const SELECTED_PROFILE_BUDGET_MS: u128 = 500;

/// Provenance of one resolved profile value, ordered from weakest to strongest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileValueSource {
    BuiltInDefault,
    BaseConfig,
    Profile,
    Environment,
    Invocation,
}

/// A value paired with the source that supplied it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcedValue<T> {
    pub value: T,
    pub source: ProfileValueSource,
}

/// Preserves whether clap supplied a value explicitly or only supplied its
/// parser default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvocationValue<T> {
    Omitted,
    Explicit(T),
}

impl<T> InvocationValue<T> {
    pub fn into_option(self) -> Option<T> {
        match self {
            Self::Omitted => None,
            Self::Explicit(value) => Some(value),
        }
    }
}

pub fn resolve_sourced<T>(
    invocation: Option<T>,
    environment: Option<T>,
    profile: Option<T>,
    base_config: Option<T>,
    built_in_default: T,
) -> SourcedValue<T> {
    if let Some(value) = invocation {
        SourcedValue {
            value,
            source: ProfileValueSource::Invocation,
        }
    } else if let Some(value) = environment {
        SourcedValue {
            value,
            source: ProfileValueSource::Environment,
        }
    } else if let Some(value) = profile {
        SourcedValue {
            value,
            source: ProfileValueSource::Profile,
        }
    } else if let Some(value) = base_config {
        SourcedValue {
            value,
            source: ProfileValueSource::BaseConfig,
        }
    } else {
        SourcedValue {
            value: built_in_default,
            source: ProfileValueSource::BuiltInDefault,
        }
    }
}

/// Apply profile tool fields without introducing a second tool-policy model.
pub fn overlay_profile_tools(
    base: &ToolConfig,
    tool_profile: Option<&str>,
    enabled: &[String],
    disabled: &[String],
) -> ToolConfig {
    let mut overlaid = base.clone();
    if let Some(profile) = tool_profile {
        overlaid.profile = profile.to_string();
    }
    if !enabled.is_empty() {
        overlaid.enabled = enabled.to_vec();
    }
    overlaid.disabled.extend(disabled.iter().cloned());
    overlaid
}

/// Resolve a persisted tool reference through the same canonical alias table
/// used by [`ToolConfig`].
pub fn normalize_tool_reference(name: &str) -> &str {
    jcode_tool_types::resolve_tool_name(name.trim())
}

/// Format a diagnostic without reproducing user-authored values. This is used
/// for sensitive fields such as instructions and remains safe for all fields.
pub fn safe_profile_diagnostic(
    profile_name: &str,
    field_name: &str,
    supplied_value: Option<&str>,
) -> String {
    let presence = supplied_value.map_or("not supplied", |_| "value supplied");
    format!("profile '{profile_name}' has an invalid '{field_name}' field ({presence})")
}

/// Validate profile structure and enum-like values without consulting the
/// provider catalog, tool registry, filesystem, or installed skills.
pub fn validate_profile_definitions(
    profiles: &BTreeMap<String, SessionProfileConfig>,
) -> Result<()> {
    for (name, profile) in profiles {
        if name.is_empty() || name.trim() != name {
            bail!(
                "invalid session profile name {name:?}: names must be non-empty and contain no leading or trailing whitespace"
            );
        }

        for (field, value) in [
            ("provider", profile.provider.as_deref()),
            ("model", profile.model.as_deref()),
            ("reasoning_effort", profile.reasoning_effort.as_deref()),
            ("provider_profile", profile.provider_profile.as_deref()),
            ("tool_profile", profile.tool_profile.as_deref()),
            ("instructions", profile.instructions.as_deref()),
        ] {
            if value.is_some_and(|value| value.trim().is_empty()) {
                bail!(
                    "{}; remove the field or supply a non-empty value",
                    safe_profile_diagnostic(name, field, value)
                );
            }
        }

        if let Some(effort) = profile.reasoning_effort.as_deref()
            && !matches!(
                effort.trim().to_ascii_lowercase().as_str(),
                "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
            )
        {
            bail!(
                "profile '{name}' has unsupported reasoning_effort '{effort}'; use one of none, minimal, low, medium, high, xhigh, or max"
            );
        }

        if let Some(tool_profile) = profile.tool_profile.as_deref()
            && !matches!(
                tool_profile.trim().to_ascii_lowercase().as_str(),
                "full" | "acp" | "minimal" | "lite" | "small" | "none" | "off" | "disabled"
            )
        {
            bail!(
                "profile '{name}' has unsupported tool_profile '{tool_profile}'; use full, acp, minimal/lite, or none"
            );
        }

        for (field, values) in [
            ("tools", &profile.tools),
            ("disabled_tools", &profile.disabled_tools),
            ("skills", &profile.skills),
        ] {
            if values.iter().any(|value| value.trim().is_empty()) {
                bail!(
                    "profile '{name}' has an empty entry in '{field}'; remove it or supply a non-empty name"
                );
            }
        }
    }
    Ok(())
}

/// Select a profile by its exact persisted key.
pub fn select_profile<'a>(
    profiles: &'a BTreeMap<String, SessionProfileConfig>,
    name: &str,
) -> Result<&'a SessionProfileConfig> {
    profiles.get(name).ok_or_else(|| {
        let choices = if profiles.is_empty() {
            "none configured".to_string()
        } else {
            profiles.keys().cloned().collect::<Vec<_>>().join(", ")
        };
        anyhow::anyhow!("unknown session profile '{name}'; available profiles: {choices}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{SessionProfileConfig, ToolConfig};
    use std::collections::{BTreeMap, HashSet};

    #[test]
    fn resolution_uses_field_by_field_source_precedence() {
        let resolved = resolve_sourced(
            Some("invocation".to_string()),
            Some("environment".to_string()),
            Some("profile".to_string()),
            Some("base".to_string()),
            "default".to_string(),
        );
        assert_eq!(resolved.value, "invocation");
        assert_eq!(resolved.source, ProfileValueSource::Invocation);

        let resolved = resolve_sourced(
            None,
            Some("environment".to_string()),
            Some("profile".to_string()),
            Some("base".to_string()),
            "default".to_string(),
        );
        assert_eq!(resolved.value, "environment");
        assert_eq!(resolved.source, ProfileValueSource::Environment);

        let resolved = resolve_sourced(
            None,
            None,
            Some("profile".to_string()),
            Some("base".to_string()),
            "default".to_string(),
        );
        assert_eq!(resolved.value, "profile");
        assert_eq!(resolved.source, ProfileValueSource::Profile);

        let resolved = resolve_sourced(
            None,
            None,
            None,
            Some("base".to_string()),
            "default".to_string(),
        );
        assert_eq!(resolved.value, "base");
        assert_eq!(resolved.source, ProfileValueSource::BaseConfig);

        let resolved = resolve_sourced(None, None, None, None, "default".to_string());
        assert_eq!(resolved.value, "default");
        assert_eq!(resolved.source, ProfileValueSource::BuiltInDefault);
    }

    #[test]
    fn explicit_invocation_values_are_distinct_from_omitted_parser_defaults() {
        let omitted = InvocationValue::<String>::Omitted;
        let explicit = InvocationValue::Explicit("auto".to_string());

        assert_eq!(omitted.into_option(), None);
        assert_eq!(explicit.into_option().as_deref(), Some("auto"));
    }

    #[test]
    fn profile_tools_overlay_existing_tool_config_semantics() {
        let base = ToolConfig {
            profile: "minimal".to_string(),
            disabled: vec!["write".to_string()],
            ..ToolConfig::default()
        };
        let overlaid = overlay_profile_tools(
            &base,
            Some("full"),
            &["read".to_string(), "bash".to_string()],
            &["bash".to_string()],
        );
        let selection = overlaid.selection();

        assert_eq!(
            selection.allowed_tools,
            Some(HashSet::from(["read".to_string()]))
        );
        assert!(selection.disabled_tools.contains("bash"));
    }

    #[test]
    fn wildcard_and_omitted_profile_tool_fields_preserve_existing_semantics() {
        let base = ToolConfig {
            profile: "minimal".to_string(),
            disabled: vec!["write".to_string()],
            ..ToolConfig::default()
        };
        assert_eq!(
            overlay_profile_tools(&base, None, &[], &[]).selection(),
            base.selection()
        );

        let wildcard = overlay_profile_tools(&base, None, &["*".to_string()], &[]);
        assert!(wildcard.selection().allowed_tools.is_none());
        assert!(wildcard.selection().disabled_tools.contains("write"));
    }

    #[test]
    fn diagnostics_redact_instruction_contents_and_credentials() {
        let diagnostic = safe_profile_diagnostic(
            "review",
            "instructions",
            Some("SECRET_CREDENTIAL_MARKER Review everything"),
        );
        assert!(diagnostic.contains("review"));
        assert!(diagnostic.contains("instructions"));
        assert!(!diagnostic.contains("SECRET"));
        assert!(!diagnostic.contains("CREDENTIAL_MARKER"));
        assert!(!diagnostic.contains("Review everything"));
    }

    #[test]
    fn static_validation_rejects_invalid_names_scalars_and_enum_values() {
        for name in ["", " ", " review", "review "] {
            let profiles = BTreeMap::from([(name.to_string(), SessionProfileConfig::default())]);
            assert!(
                validate_profile_definitions(&profiles).is_err(),
                "name={name:?}"
            );
        }

        for (field, profile) in [
            (
                "provider",
                SessionProfileConfig {
                    provider: Some(String::new()),
                    ..Default::default()
                },
            ),
            (
                "model",
                SessionProfileConfig {
                    model: Some("  ".to_string()),
                    ..Default::default()
                },
            ),
            (
                "instructions",
                SessionProfileConfig {
                    instructions: Some(String::new()),
                    ..Default::default()
                },
            ),
            (
                "reasoning_effort",
                SessionProfileConfig {
                    reasoning_effort: Some("turbo".to_string()),
                    ..Default::default()
                },
            ),
            (
                "tool_profile",
                SessionProfileConfig {
                    tool_profile: Some("dangerous".to_string()),
                    ..Default::default()
                },
            ),
        ] {
            let profiles = BTreeMap::from([("review".to_string(), profile)]);
            let error = validate_profile_definitions(&profiles)
                .unwrap_err()
                .to_string();
            assert!(error.contains(field), "{error}");
        }
    }

    #[test]
    fn malformed_profile_lists_fail_during_deserialization() {
        let malformed = "[profiles.review]\ntools = \"read\"\n";
        assert!(toml::from_str::<crate::config::Config>(malformed).is_err());
    }

    #[test]
    fn unselected_environment_dependent_references_are_statically_valid() {
        let profiles = BTreeMap::from([(
            "offline".to_string(),
            SessionProfileConfig {
                provider: Some("not-installed-provider".to_string()),
                model: Some("not-installed-model".to_string()),
                provider_profile: Some("not-installed-profile".to_string()),
                tools: vec!["not-installed-tool".to_string()],
                skills: vec!["not-installed-skill".to_string()],
                ..Default::default()
            },
        )]);
        validate_profile_definitions(&profiles).expect("references are checked only when selected");
    }

    #[test]
    fn exact_lookup_has_no_alias_case_folding_or_fallback() {
        let profiles = BTreeMap::from([(
            "Review".to_string(),
            SessionProfileConfig {
                model: Some("model-a".to_string()),
                ..Default::default()
            },
        )]);

        assert!(select_profile(&profiles, "Review").is_ok());
        for name in ["review", " Review", "Review ", "default"] {
            let error = select_profile(&profiles, name).unwrap_err().to_string();
            assert!(error.contains(name));
            assert!(error.contains("Review"));
        }
    }

    #[test]
    fn unknown_selection_lists_available_profiles_without_profile_contents() {
        let profiles = BTreeMap::from([
            (
                "review".to_string(),
                SessionProfileConfig {
                    instructions: Some("SECRET review instructions".to_string()),
                    ..Default::default()
                },
            ),
            ("research".to_string(), SessionProfileConfig::default()),
        ]);
        let error = select_profile(&profiles, "missing")
            .unwrap_err()
            .to_string();
        assert!(error.contains("missing"));
        assert!(error.contains("review"));
        assert!(error.contains("research"));
        assert!(!error.contains("SECRET"));
        assert!(!error.contains("instructions"));
    }
}
