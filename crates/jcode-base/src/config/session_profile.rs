//! Per-run named session profile resolution.
//!
//! This module intentionally contains only the immutable, data-only overlay
//! produced from [`Config`]. Provider and skill validation belong to their
//! existing owners; tool policy composition accepts immutable registry names
//! without mutating persisted configuration or a process-global registry.

use super::{Config, SessionProfileConfig, SkillsMode, ToolConfig, ToolSelection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

const MAX_HANDOFF_INSTRUCTIONS_FILE_BYTES: u64 = 64 * 1024;

/// Effective fresh-session handoff policy for one session.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedHandoffPolicy {
    pub enabled: bool,
    pub agent_enabled: bool,
    pub agent_requires_confirmation: bool,
    pub auto_start: bool,
    pub max_chain_transitions: usize,
    pub poke_enabled: bool,
    pub poke_soft_floor: f32,
    pub poke_hard_threshold: f32,
    pub copy_todos: bool,
    pub instructions: Option<String>,
}

/// Prompt context selected by one named session profile.
///
/// The names and rendered prompts are kept separately so the prompt layer can
/// validate/render skills at the session boundary while retaining the exact
/// configured order. Empty overlays are used by the legacy no-profile path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionPromptOverlay {
    /// Skill names selected by the profile, in configuration order.
    pub skill_names: Vec<String>,
    /// Rendered skill prompts for the current session.
    pub skill_prompts: Vec<String>,
    /// Additional profile instructions, if non-empty.
    pub instructions: Option<String>,
}

/// Session-local skill policy derived from one profile and an available skill
/// name snapshot. The policy is data-only so every skill consumer can share
/// the same filtered view without mutating the process-global registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SkillPolicy {
    /// Explicit mode, or `None` for the legacy no-profile behavior.
    pub mode: Option<SkillsMode>,
    /// Canonical profile-selected skill names, in first-seen order.
    pub selected_skills: Vec<String>,
    /// Canonical subtractive deny-list, in first-seen order.
    pub disabled_skills: Vec<String>,
    /// Effective names for the available-skill snapshot used to build this
    /// policy. Sorted for deterministic prompt and inspection output.
    pub effective_skills: Vec<String>,
}

impl SkillPolicy {
    /// Build a policy against an immutable available-name snapshot.
    pub fn for_available<SI, SD, SA, SS, DS, AS>(
        profile_name: &str,
        mode: Option<SkillsMode>,
        selected_skills: SI,
        disabled_skills: SD,
        available_names: SA,
    ) -> anyhow::Result<Self>
    where
        SI: IntoIterator<Item = SS>,
        SD: IntoIterator<Item = DS>,
        SA: IntoIterator<Item = AS>,
        SS: AsRef<str>,
        DS: AsRef<str>,
        AS: AsRef<str>,
    {
        let selected_skills = canonical_skill_names(profile_name, "skills", selected_skills)?;
        let disabled_skills =
            canonical_skill_names(profile_name, "disabled_skills", disabled_skills)?;
        let mut available = available_names
            .into_iter()
            .map(|name| name.as_ref().trim().to_owned())
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>();
        available.sort_unstable();
        available.dedup();

        let available_set = available.iter().collect::<HashSet<_>>();
        for (field, names) in [
            ("skills", &selected_skills),
            ("disabled_skills", &disabled_skills),
        ] {
            for name in names {
                if !available_set.contains(name) {
                    anyhow::bail!(
                        "Session profile '{}' references unknown skill '{}' in {}; available skills: {}",
                        profile_name,
                        name,
                        field,
                        display_skill_names(&available)
                    );
                }
            }
        }

        let mut effective = match mode {
            Some(SkillsMode::None) => Vec::new(),
            Some(SkillsMode::Allowlist) => selected_skills.clone(),
            Some(SkillsMode::All) | None => available,
        };
        let disabled = disabled_skills.iter().collect::<HashSet<_>>();
        effective.retain(|name| !disabled.contains(name));
        effective.sort_unstable();
        effective.dedup();

        Ok(Self {
            mode,
            selected_skills,
            disabled_skills,
            effective_skills: effective,
        })
    }

    /// Build the policy before a registry exists. This validates/canonicalizes
    /// profile-owned names; callers should use [`Self::for_available`] at the
    /// registry boundary to validate availability and derive effective names.
    pub fn from_profile(
        profile_name: &str,
        mode: Option<SkillsMode>,
        selected_skills: &[String],
        disabled_skills: &[String],
    ) -> anyhow::Result<Self> {
        Ok(Self {
            mode,
            selected_skills: canonical_skill_names(profile_name, "skills", selected_skills)?,
            disabled_skills: canonical_skill_names(
                profile_name,
                "disabled_skills",
                disabled_skills,
            )?,
            effective_skills: Vec::new(),
        })
    }

    pub fn allows(&self, name: &str) -> bool {
        self.effective_skills
            .iter()
            .any(|candidate| candidate == name)
    }
}

fn canonical_skill_names<I, S>(
    profile_name: &str,
    field: &str,
    names: I,
) -> anyhow::Result<Vec<String>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for raw in names {
        let name = raw.as_ref().trim();
        if name.is_empty() {
            anyhow::bail!(
                "Session profile '{}' has an empty skill name in {}; provide a configured skill name",
                profile_name,
                field
            );
        }
        if seen.insert(name.to_owned()) {
            result.push(name.to_owned());
        }
    }
    Ok(result)
}

fn display_skill_names(names: &[String]) -> String {
    if names.is_empty() {
        "(none)".to_owned()
    } else {
        names
            .iter()
            .map(|name| format!("'{name}'"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Non-secret source label for a resolved field.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FieldSource {
    Profile,
    Environment,
    BaseConfig,
    BuiltInDefault,
    Restored,
    None,
}

/// Safe prompt overlay metadata persisted in a resolved snapshot. Instruction
/// bodies are intentionally omitted; only presence and length are retained.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SessionPromptOverlaySnapshot {
    pub skill_names: Vec<String>,
    pub instructions_present: bool,
    pub instructions_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ProviderModelReasoningSnapshot {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub provider_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ToolPolicySnapshot {
    pub profile: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub disabled_tools: Vec<String>,
}

/// Credential-free effective profile values used for restore/drift checks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ResolvedProfileSnapshot {
    pub profile_name: Option<String>,
    pub provider_model_reasoning: ProviderModelReasoningSnapshot,
    pub tool_policy: ToolPolicySnapshot,
    pub skill_policy: SkillPolicy,
    pub prompt_overlay: SessionPromptOverlaySnapshot,
    pub fingerprint: String,
}

/// Safe restore outcome persisted with a session so the client can explain
/// profile drift without re-reading or printing profile secrets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ProfileRestoreStatus {
    #[default]
    Legacy,
    Matching,
    Missing {
        profile_name: String,
    },
    Changed {
        profile_name: String,
        changed_fields: Vec<String>,
    },
    ExplicitNone,
}

impl ProfileRestoreStatus {
    pub fn warning(&self) -> Option<String> {
        match self {
            Self::Missing { profile_name } => Some(format!(
                "Saved profile '{profile_name}' is missing; the restored snapshot remains active. Choose a profile explicitly to replace it."
            )),
            Self::Changed {
                profile_name,
                changed_fields,
            } => {
                let fields = if changed_fields.is_empty() {
                    "effective values".to_owned()
                } else {
                    changed_fields.join(", ")
                };
                Some(format!(
                    "Saved profile '{profile_name}' changed ({fields}); the restored snapshot remains active. Choose a profile explicitly to adopt current values."
                ))
            }
            _ => None,
        }
    }
}

impl ResolvedProfileSnapshot {
    pub fn with_fingerprint(mut self) -> Self {
        self.fingerprint = snapshot_fingerprint(&self);
        self
    }

    pub fn is_secret_free(&self) -> bool {
        // The snapshot stores instruction presence/length only; it has no
        // field capable of carrying the instruction body or credentials.
        true
    }

    /// Return stable, non-secret field labels that differ between snapshots.
    pub fn changed_fields(&self, current: &Self) -> Vec<String> {
        let mut changed = Vec::new();
        if self.provider_model_reasoning.provider != current.provider_model_reasoning.provider {
            changed.push("provider".to_owned());
        }
        if self.provider_model_reasoning.model != current.provider_model_reasoning.model {
            changed.push("model".to_owned());
        }
        if self.provider_model_reasoning.reasoning_effort
            != current.provider_model_reasoning.reasoning_effort
        {
            changed.push("reasoning_effort".to_owned());
        }
        if self.provider_model_reasoning.provider_profile
            != current.provider_model_reasoning.provider_profile
        {
            changed.push("provider_profile".to_owned());
        }
        if self.tool_policy != current.tool_policy {
            changed.push("tool_policy".to_owned());
        }
        if self.skill_policy != current.skill_policy {
            changed.push("skill_policy".to_owned());
        }
        if self.prompt_overlay != current.prompt_overlay {
            changed.push("prompt_overlay".to_owned());
        }
        changed
    }
}

/// Safe command/debug projection of one resolved profile.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ProfileInspectionResult {
    pub profile_name: Option<String>,
    pub effective: ResolvedProfileSnapshot,
    pub sources: BTreeMap<String, FieldSource>,
    pub warnings: Vec<String>,
}

impl SessionPromptOverlay {
    /// Return whether this overlay contributes no prompt context.
    pub fn is_empty(&self) -> bool {
        self.skill_names.is_empty() && self.skill_prompts.is_empty() && self.instructions.is_none()
    }
}

/// Immutable profile values selected for one run/session.
///
/// This value owns copies of persisted strings and lists. Resolving a profile
/// therefore cannot mutate [`Config`], its profile map, or any shared runtime
/// registry. Later runtime layers may validate and apply these values through
/// their canonical provider, tool, and prompt owners.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedSessionProfile {
    /// Exact selected profile name, or `None` for the legacy path.
    pub profile_name: Option<String>,
    /// Provider override supplied by the selected profile.
    pub provider: Option<String>,
    /// Model override supplied by the selected profile.
    pub model: Option<String>,
    /// Reasoning effort override supplied by the selected profile.
    pub reasoning_effort: Option<String>,
    /// Named provider-profile override supplied by the selected profile.
    pub provider_profile: Option<String>,
    /// Tool profile baseline supplied by the selected profile.
    pub tool_profile: Option<String>,
    /// Tool allow-list supplied by the selected profile.
    pub tools: Vec<String>,
    /// Tool deny-list supplied by the selected profile.
    pub disabled_tools: Vec<String>,
    /// Canonical skill policy. Effective names are populated once an
    /// available registry snapshot is supplied.
    pub skill_policy: SkillPolicy,
    /// Prompt context supplied by the selected profile.
    pub prompt_overlay: SessionPromptOverlay,
}

impl ResolvedSessionProfile {
    fn from_config(name: &str, profile: &SessionProfileConfig) -> anyhow::Result<Self> {
        Ok(Self {
            profile_name: Some(name.to_owned()),
            provider: profile.provider.clone(),
            model: profile.model.clone(),
            reasoning_effort: profile.reasoning_effort.clone(),
            provider_profile: profile.provider_profile.clone(),
            tool_profile: profile.tool_profile.clone(),
            tools: profile.tools.clone(),
            disabled_tools: profile.disabled_tools.clone(),
            skill_policy: SkillPolicy::from_profile(
                name,
                profile.skills_mode,
                &profile.skills,
                &profile.disabled_skills,
            )?,
            prompt_overlay: SessionPromptOverlay {
                skill_names: profile.skills.clone(),
                skill_prompts: Vec::new(),
                instructions: profile
                    .instructions
                    .as_deref()
                    .filter(|instructions| !instructions.trim().is_empty())
                    .map(str::to_owned),
            },
        })
    }

    /// Overlay the profile's tool fields on the effective base configuration.
    ///
    /// ToolConfig remains the single owner of profile baselines, aliases,
    /// wildcard handling, and allow/deny composition. Empty profile lists mean
    /// that the corresponding base value remains effective.
    pub fn tool_config(&self, base: &ToolConfig) -> ToolConfig {
        let mut tools = base.clone();
        if let Some(profile) = self.tool_profile.as_deref() {
            tools.profile = profile.to_owned();
        }
        if !self.tools.is_empty() {
            tools.enabled = self.tools.clone();
        }
        if !self.disabled_tools.is_empty() {
            tools.disabled = self.disabled_tools.clone();
        }
        tools
    }

    /// Resolve the effective tool policy using the existing ToolConfig rules.
    pub fn tool_selection(&self, base: &ToolConfig) -> ToolSelection {
        self.tool_config(base).selection()
    }

    /// Validate profile tool names against the names exposed by a constructed
    /// runtime Registry.
    ///
    /// The config crate intentionally receives an immutable name snapshot
    /// rather than depending on the runtime tool crate. This keeps the layer
    /// boundary intact while making validation occur immediately before Agent
    /// construction, after built-in and MCP tools have been registered.
    pub fn validate_tool_names<I, S>(&self, available_names: I) -> anyhow::Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let Some(profile_name) = self.profile_name.as_deref() else {
            return Ok(());
        };

        validate_tool_profile(profile_name, self.tool_profile.as_deref())?;

        let available = available_names
            .into_iter()
            .map(|name| normalize_tool_name(name.as_ref()))
            .filter(|name| !name.is_empty())
            .collect::<HashSet<_>>();

        for (field, values) in [
            ("tools", self.tools.as_slice()),
            ("disabled_tools", self.disabled_tools.as_slice()),
        ] {
            for raw in values {
                let normalized = normalize_tool_name(raw);
                if normalized.is_empty() || is_tool_wildcard(&normalized) {
                    continue;
                }
                if !available.contains(&normalized) {
                    return Err(invalid_tool_name_error(
                        profile_name,
                        field,
                        raw,
                        available.iter().map(String::as_str),
                    ));
                }
            }
        }

        Ok(())
    }

    /// Resolve skill names against an immutable available registry snapshot.
    pub fn with_available_skills<I, S>(mut self, available_names: I) -> anyhow::Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.skill_policy = SkillPolicy::for_available(
            self.profile_name.as_deref().unwrap_or("(none)"),
            self.skill_policy.mode,
            self.skill_policy.selected_skills.clone(),
            self.skill_policy.disabled_skills.clone(),
            available_names,
        )?;
        Ok(self)
    }

    /// Create a credential-free snapshot suitable for session metadata and
    /// diagnostics. Raw profile instructions are represented only by a safe
    /// presence/length summary.
    pub fn snapshot(&self, base: &ToolConfig) -> ResolvedProfileSnapshot {
        let selection = self.tool_selection(base);
        let mut allowed_tools = selection.allowed_tools.map(|tools| {
            let mut tools = tools.into_iter().collect::<Vec<_>>();
            tools.sort_unstable();
            tools
        });
        let mut disabled_tools = selection.disabled_tools.into_iter().collect::<Vec<_>>();
        disabled_tools.sort_unstable();
        let prompt_overlay = SessionPromptOverlaySnapshot {
            skill_names: self.skill_policy.selected_skills.clone(),
            instructions_present: self.prompt_overlay.instructions.is_some(),
            instructions_chars: self
                .prompt_overlay
                .instructions
                .as_deref()
                .map_or(0, str::len),
        };
        let mut snapshot = ResolvedProfileSnapshot {
            profile_name: self.profile_name.clone(),
            provider_model_reasoning: ProviderModelReasoningSnapshot {
                provider: self.provider.clone(),
                model: self.model.clone(),
                reasoning_effort: self.reasoning_effort.clone(),
                provider_profile: self.provider_profile.clone(),
            },
            tool_policy: ToolPolicySnapshot {
                profile: Some(selection_profile_name(&self.tool_profile, base)),
                allowed_tools: allowed_tools.take(),
                disabled_tools,
            },
            skill_policy: self.skill_policy.clone(),
            prompt_overlay,
            fingerprint: String::new(),
        };
        snapshot.fingerprint = snapshot_fingerprint(&snapshot);
        snapshot
    }

    pub fn inspection(&self, base: &ToolConfig) -> ProfileInspectionResult {
        let mut sources = BTreeMap::new();
        for field in [
            "provider",
            "model",
            "reasoning_effort",
            "provider_profile",
            "tool_policy",
            "skill_policy",
            "prompt_overlay",
        ] {
            sources.insert(
                field.to_owned(),
                if self.profile_name.is_some() {
                    FieldSource::Profile
                } else {
                    FieldSource::BaseConfig
                },
            );
        }
        ProfileInspectionResult {
            profile_name: self.profile_name.clone(),
            effective: self.snapshot(base),
            sources,
            warnings: Vec::new(),
        }
    }
}

fn selection_profile_name(profile: &Option<String>, base: &ToolConfig) -> String {
    profile.clone().unwrap_or_else(|| base.profile.clone())
}

fn snapshot_fingerprint(snapshot: &ResolvedProfileSnapshot) -> String {
    let mut canonical = snapshot.clone();
    canonical.fingerprint.clear();
    let bytes = match serde_json::to_vec(&canonical) {
        Ok(bytes) => bytes,
        Err(error) => format!("{canonical:?}:{error}").into_bytes(),
    };
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

impl Config {
    /// Resolve the global handoff policy plus optional per-profile overrides.
    /// Instruction sources are appended in deterministic order: global inline,
    /// global file, profile inline, then profile file.
    pub fn resolve_handoff_policy(
        &self,
        profile_name: Option<&str>,
        config_dir: Option<&Path>,
    ) -> anyhow::Result<ResolvedHandoffPolicy> {
        let profile = match profile_name {
            Some(name) => Some(
                self.profiles
                    .get(name)
                    .ok_or_else(|| anyhow::anyhow!("Unknown session profile '{name}'"))?,
            ),
            None => None,
        };
        let profile_handoff = profile.and_then(|profile| profile.handoff.as_ref());

        let enabled = profile_handoff
            .and_then(|handoff| handoff.enabled)
            .unwrap_or(self.handoff.enabled);
        let agent_enabled = profile_handoff
            .and_then(|handoff| handoff.agent_enabled)
            .unwrap_or(self.handoff.agent_enabled);
        let agent_requires_confirmation = profile_handoff
            .and_then(|handoff| handoff.agent_requires_confirmation)
            .unwrap_or(self.handoff.agent_requires_confirmation);
        let auto_start = profile_handoff
            .and_then(|handoff| handoff.auto_start)
            .unwrap_or(self.handoff.auto_start);
        let max_chain_transitions = profile_handoff
            .and_then(|handoff| handoff.max_chain_transitions)
            .unwrap_or(self.handoff.max_chain_transitions);
        let poke_enabled = profile_handoff
            .and_then(|handoff| handoff.poke_enabled)
            .unwrap_or(self.handoff.poke_enabled);
        let poke_soft_floor = profile_handoff
            .and_then(|handoff| handoff.poke_soft_floor)
            .unwrap_or(self.handoff.poke_soft_floor);
        let poke_hard_threshold = profile_handoff
            .and_then(|handoff| handoff.poke_hard_threshold)
            .unwrap_or(self.handoff.poke_hard_threshold);
        let copy_todos = profile_handoff
            .and_then(|handoff| handoff.copy_todos)
            .unwrap_or(self.handoff.copy_todos);

        if max_chain_transitions == 0 {
            anyhow::bail!("handoff.max_chain_transitions must be at least 1");
        }
        if !(0.0..=1.0).contains(&poke_soft_floor)
            || !(0.0..=1.0).contains(&poke_hard_threshold)
            || poke_soft_floor > poke_hard_threshold
        {
            anyhow::bail!("handoff poke thresholds must satisfy 0 <= soft <= hard <= 1");
        }

        if !enabled {
            return Ok(ResolvedHandoffPolicy {
                enabled,
                agent_enabled,
                agent_requires_confirmation,
                auto_start,
                max_chain_transitions,
                poke_enabled,
                poke_soft_floor,
                poke_hard_threshold,
                copy_todos,
                instructions: None,
            });
        }

        let mut instruction_parts = Vec::new();
        push_nonempty(&mut instruction_parts, self.handoff.instructions.as_deref());
        if let Some(path) = self.handoff.instructions_file.as_deref() {
            push_nonempty(
                &mut instruction_parts,
                Some(&read_handoff_instructions(path, config_dir)?),
            );
        }
        if let Some(handoff) = profile_handoff {
            push_nonempty(&mut instruction_parts, handoff.instructions.as_deref());
            if let Some(path) = handoff.instructions_file.as_deref() {
                push_nonempty(
                    &mut instruction_parts,
                    Some(&read_handoff_instructions(path, config_dir)?),
                );
            }
        }

        Ok(ResolvedHandoffPolicy {
            enabled,
            agent_enabled,
            agent_requires_confirmation,
            auto_start,
            max_chain_transitions,
            poke_enabled,
            poke_soft_floor,
            poke_hard_threshold,
            copy_todos,
            instructions: (!instruction_parts.is_empty()).then(|| instruction_parts.join("\n\n")),
        })
    }

    /// Resolve an exact profile name into an immutable per-run overlay.
    ///
    /// `None` returns immediately without looking at the profile map. A
    /// supplied name is never trimmed or otherwise normalized: empty,
    /// whitespace-only, and unknown names fail instead of silently selecting a
    /// different profile.
    pub fn resolve_session_profile(
        &self,
        profile_name: Option<&str>,
    ) -> anyhow::Result<ResolvedSessionProfile> {
        let Some(profile_name) = profile_name else {
            return Ok(ResolvedSessionProfile::default());
        };

        if profile_name.trim().is_empty() {
            anyhow::bail!(
                "Session profile name cannot be empty or whitespace-only; choose one of: {}",
                available_profile_names(&self.profiles)
            );
        }

        let Some(profile) = self.profiles.get(profile_name) else {
            anyhow::bail!(
                "Unknown session profile '{}'; available profiles: {}",
                profile_name,
                available_profile_names(&self.profiles)
            );
        };

        let resolved = ResolvedSessionProfile::from_config(profile_name, profile)?;
        validate_profile_provider(self, profile_name, resolved.provider.as_deref())?;
        validate_profile_model(
            self,
            profile_name,
            resolved.provider_profile.as_deref(),
            resolved.model.as_deref(),
        )?;
        validate_profile_reasoning_effort(profile_name, resolved.reasoning_effort.as_deref())?;
        validate_provider_profile_combination(
            profile_name,
            resolved.provider.as_deref(),
            resolved.provider_profile.as_deref(),
        )?;
        validate_tool_profile(profile_name, resolved.tool_profile.as_deref())?;
        validate_profile_tool_names(profile_name, &resolved.tools, "tools")?;
        validate_profile_tool_names(profile_name, &resolved.disabled_tools, "disabled_tools")?;
        Ok(resolved)
    }

    /// Resolve a profile while preserving the existing environment-over-config
    /// precedence for fields that are also exposed through environment
    /// variables.
    ///
    /// `Config::load` applies these environment values before a run resolver is
    /// called, so the profile overlay must not replace a field whose source is
    /// the environment. The returned value only carries profile-owned fields;
    /// callers continue to read the effective environment/base values from the
    /// supplied `Config`.
    pub fn resolve_session_profile_with_environment(
        &self,
        profile_name: Option<&str>,
    ) -> anyhow::Result<ResolvedSessionProfile> {
        let mut resolved = self.resolve_session_profile(profile_name)?;
        if profile_name.is_none() {
            return Ok(resolved);
        }

        let environment = SessionProfileEnvironment::current();
        if environment.provider {
            resolved.provider = None;
        }
        if environment.model {
            resolved.model = None;
        }
        if environment.reasoning_effort {
            resolved.reasoning_effort = None;
        }
        if environment.provider_profile {
            resolved.provider_profile = None;
        }
        if environment.tool_profile {
            resolved.tool_profile = None;
        }
        if environment.tools {
            resolved.tools.clear();
        }
        if environment.disabled_tools {
            resolved.disabled_tools.clear();
        }

        Ok(resolved)
    }

    /// Resolve a profile and validate its skill policy against one immutable
    /// registry name snapshot. This is the owning trust boundary for skill
    /// availability and does not initialize a provider or mutate config.
    pub fn resolve_session_profile_with_skills<I, S>(
        &self,
        profile_name: Option<&str>,
        available_names: I,
    ) -> anyhow::Result<ResolvedSessionProfile>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.resolve_session_profile(profile_name)?
            .with_available_skills(available_names)
    }
}

fn push_nonempty(parts: &mut Vec<String>, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        parts.push(value.to_owned());
    }
}

fn read_handoff_instructions(path: &str, config_dir: Option<&Path>) -> anyhow::Result<String> {
    let path = PathBuf::from(path);
    let path = if path.is_absolute() {
        path
    } else {
        config_dir.unwrap_or_else(|| Path::new(".")).join(path)
    };
    let metadata = std::fs::metadata(&path).map_err(|error| {
        anyhow::anyhow!(
            "Could not read handoff instructions file '{}': {error}",
            path.display()
        )
    })?;
    if metadata.len() > MAX_HANDOFF_INSTRUCTIONS_FILE_BYTES {
        anyhow::bail!(
            "Handoff instructions file '{}' exceeds the 64 KiB limit",
            path.display()
        );
    }
    std::fs::read_to_string(&path).map_err(|error| {
        anyhow::anyhow!(
            "Could not read handoff instructions file '{}': {error}",
            path.display()
        )
    })
}

/// Return the active named provider profile selected by the existing provider
/// bootstrap environment, if any.
pub fn active_environment_provider_profile() -> Option<String> {
    let active = std::env::var_os("JCODE_PROVIDER_PROFILE_ACTIVE").is_some()
        || std::env::var_os("JCODE_NAMED_PROVIDER_PROFILE").is_some();
    if !active {
        return None;
    }

    [
        "JCODE_PROVIDER_PROFILE_NAME",
        "JCODE_NAMED_PROVIDER_PROFILE",
    ]
    .into_iter()
    .find_map(|key| {
        let Ok(value) = std::env::var(key) else {
            return None;
        };
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_owned())
    })
}

#[derive(Debug, Clone, Copy, Default)]
struct SessionProfileEnvironment {
    provider: bool,
    model: bool,
    reasoning_effort: bool,
    provider_profile: bool,
    tool_profile: bool,
    tools: bool,
    disabled_tools: bool,
}

impl SessionProfileEnvironment {
    fn current() -> Self {
        Self {
            provider: env_non_empty("JCODE_PROVIDER"),
            model: std::env::var_os("JCODE_MODEL").is_some(),
            reasoning_effort: env_non_empty("JCODE_OPENAI_REASONING_EFFORT")
                || env_non_empty("JCODE_ANTHROPIC_REASONING_EFFORT"),
            provider_profile: active_environment_provider_profile().is_some(),
            tool_profile: std::env::var_os("JCODE_TOOL_PROFILE").is_some(),
            tools: std::env::var_os("JCODE_TOOLS").is_some(),
            disabled_tools: std::env::var_os("JCODE_DISABLED_TOOLS").is_some(),
        }
    }
}

fn env_non_empty(key: &str) -> bool {
    match std::env::var(key) {
        Ok(value) => !value.trim().is_empty(),
        Err(_) => false,
    }
}

fn available_profile_names(profiles: &BTreeMap<String, SessionProfileConfig>) -> String {
    if profiles.is_empty() {
        "(none)".to_owned()
    } else {
        profiles
            .keys()
            .map(|name| format!("'{name}'"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn validate_profile_provider(
    config: &Config,
    profile_name: &str,
    provider: Option<&str>,
) -> anyhow::Result<()> {
    let Some(provider) = provider else {
        return Ok(());
    };
    let provider = provider.trim();
    if provider.is_empty() {
        anyhow::bail!(
            "Session profile '{}' has invalid provider ''; choose auto or a configured provider",
            profile_name
        );
    }

    let known = provider.eq_ignore_ascii_case("auto")
        || crate::provider_catalog::resolve_login_provider(provider).is_some()
        || crate::provider_catalog::openai_compatible_profile_by_id(provider).is_some()
        || config.providers.contains_key(provider);
    if known {
        return Ok(());
    }

    let mut available = crate::provider_catalog::login_providers()
        .iter()
        .map(|provider| provider.id.to_owned())
        .collect::<Vec<_>>();
    available.extend(config.providers.keys().cloned());
    available.push("auto".to_owned());
    available.sort_unstable();
    available.dedup();
    anyhow::bail!(
        "Session profile '{}' has invalid provider '{}'; choose one of: {}",
        profile_name,
        provider,
        available.join(", ")
    )
}

fn validate_profile_model(
    config: &Config,
    profile_name: &str,
    provider_profile: Option<&str>,
    model: Option<&str>,
) -> anyhow::Result<()> {
    let Some(model) = model else {
        return Ok(());
    };
    let model = model.trim();
    if model.is_empty() || model.chars().any(char::is_control) {
        anyhow::bail!(
            "Session profile '{}' has invalid model '{}'; provide a non-empty model identifier",
            profile_name,
            model
        );
    }

    if let Some(provider_profile) = provider_profile
        && let Some(named_provider) = config.providers.get(provider_profile)
    {
        let mut available = named_provider
            .models
            .iter()
            .map(|model| model.id.trim())
            .filter(|model| !model.is_empty())
            .collect::<Vec<_>>();
        if let Some(default_model) = named_provider.default_model.as_deref() {
            let default_model = default_model.trim();
            if !default_model.is_empty() {
                available.push(default_model);
            }
        }
        available.sort_unstable();
        available.dedup();
        if !available.is_empty() && !available.contains(&model) {
            anyhow::bail!(
                "Session profile '{}' has unsupported model '{}' for provider_profile '{}'; choose one of: {}",
                profile_name,
                model,
                provider_profile,
                available.join(", ")
            );
        }
    }
    Ok(())
}

fn validate_profile_reasoning_effort(
    profile_name: &str,
    reasoning_effort: Option<&str>,
) -> anyhow::Result<()> {
    let Some(reasoning_effort) = reasoning_effort else {
        return Ok(());
    };
    let normalized = reasoning_effort.trim().to_ascii_lowercase();
    const ALLOWED: &[&str] = &[
        "none",
        "minimal",
        "low",
        "medium",
        "high",
        "xhigh",
        "max",
        "swarm",
        "swarm-deep",
    ];
    if ALLOWED.contains(&normalized.as_str()) {
        return Ok(());
    }
    anyhow::bail!(
        "Session profile '{}' has invalid reasoning_effort '{}'; choose one of {}",
        profile_name,
        reasoning_effort,
        ALLOWED.join(", ")
    )
}

fn validate_provider_profile_combination(
    profile_name: &str,
    provider: Option<&str>,
    provider_profile: Option<&str>,
) -> anyhow::Result<()> {
    let Some(provider_profile) = provider_profile else {
        return Ok(());
    };
    if provider_profile.trim().is_empty() {
        anyhow::bail!(
            "Session profile '{}' has invalid provider_profile '{}'; provide a configured profile name",
            profile_name,
            provider_profile
        );
    }

    let incompatible = provider.is_some_and(|provider| {
        matches!(
            provider.trim().to_ascii_lowercase().as_str(),
            "claude" | "anthropic" | "anthropic-api" | "gemini" | "gemini-api"
        )
    });
    if incompatible {
        let provider_name = provider.unwrap_or("");
        anyhow::bail!(
            "Session profile '{}' combines provider '{}' with provider_profile '{}'; use a compatible provider or remove provider_profile",
            profile_name,
            provider_name,
            provider_profile
        );
    }
    Ok(())
}

fn normalize_tool_name(name: &str) -> String {
    jcode_tool_types::resolve_tool_name(name.trim().trim_matches('"')).to_owned()
}

fn is_tool_wildcard(name: &str) -> bool {
    name == "*" || name.eq_ignore_ascii_case("all")
}

fn validate_tool_profile(profile_name: &str, tool_profile: Option<&str>) -> anyhow::Result<()> {
    let Some(tool_profile) = tool_profile else {
        return Ok(());
    };
    let normalized = tool_profile.trim().to_ascii_lowercase();
    if matches!(
        normalized.as_str(),
        "" | "full" | "acp" | "minimal" | "lite" | "small" | "none" | "off" | "disabled"
    ) {
        return Ok(());
    }

    anyhow::bail!(
        "Session profile '{}' has invalid tool_profile '{}'; choose one of full, acp, minimal, lite, small, or none",
        profile_name,
        tool_profile
    )
}

fn validate_profile_tool_names(
    profile_name: &str,
    values: &[String],
    field: &str,
) -> anyhow::Result<()> {
    let known = builtin_tool_names();
    for raw in values {
        let normalized = normalize_tool_name(raw);
        if normalized.is_empty() || is_tool_wildcard(&normalized) {
            continue;
        }
        // MCP tools are registered dynamically. Their exact names are checked
        // against the constructed Registry by `validate_tool_names`.
        if normalized.starts_with("mcp__") {
            continue;
        }
        if !known.contains(&normalized.as_str()) {
            return Err(invalid_tool_name_error(
                profile_name,
                field,
                raw,
                known.iter().copied(),
            ));
        }
    }
    Ok(())
}

fn invalid_tool_name_error<'a>(
    profile_name: &str,
    field: &str,
    raw: &str,
    available: impl IntoIterator<Item = &'a str>,
) -> anyhow::Error {
    let mut names = available.into_iter().collect::<Vec<_>>();
    names.sort_unstable();
    let suggestions = closest_tool_names(raw, &names);
    let guidance = if suggestions.is_empty() {
        "check the configured Registry tool names".to_owned()
    } else {
        format!("did you mean {}", suggestions.join(", "))
    };
    anyhow::anyhow!(
        "Session profile '{}' has invalid {} tool '{}'; {}",
        profile_name,
        field,
        raw,
        guidance
    )
}

fn builtin_tool_names() -> [&'static str; 29] {
    [
        "read",
        "write",
        "agentgrep",
        "side_panel",
        "edit",
        "multiedit",
        "patch",
        "apply_patch",
        "ls",
        "bash",
        "browser",
        "open",
        "webfetch",
        "websearch",
        "invalid",
        "todo",
        "bg",
        "swarm",
        "session_search",
        "memory",
        "initiative",
        "gmail",
        "schedule",
        "selfdev",
        "skill_manage",
        "batch",
        "conversation_search",
        "integration_tools",
        "macos_computer_use",
    ]
}

fn closest_tool_names(needle: &str, available: &[&str]) -> Vec<String> {
    let needle = normalize_tool_name(needle).to_ascii_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut scored = available
        .iter()
        .filter_map(|candidate| {
            let candidate_lower = candidate.to_ascii_lowercase();
            let score = if candidate_lower == needle {
                0
            } else if candidate_lower.starts_with(&needle) || needle.starts_with(&candidate_lower) {
                1
            } else if candidate_lower.contains(&needle) || needle.contains(&candidate_lower) {
                2
            } else {
                let distance = edit_distance(&needle, &candidate_lower);
                let threshold = (needle.len().max(candidate_lower.len()) / 3).max(2);
                (distance <= threshold).then_some(3 + distance)?
            };
            Some((score, *candidate))
        })
        .collect::<Vec<_>>();
    scored.sort_unstable();
    scored
        .into_iter()
        .take(3)
        .map(|(_, name)| name.to_owned())
        .collect()
}

fn edit_distance(left: &str, right: &str) -> usize {
    let mut row = (0..=right.len()).collect::<Vec<_>>();
    for (index, left_byte) in left.bytes().enumerate() {
        let mut next = vec![index + 1; right.len() + 1];
        for (right_index, right_byte) in right.bytes().enumerate() {
            next[right_index + 1] = (row[right_index + 1] + 1)
                .min(next[right_index] + 1)
                .min(row[right_index] + usize::from(left_byte != right_byte));
        }
        row = next;
    }
    row[right.len()]
}
