//! Shared fixtures for named session-profile configuration tests.
//!
//! The helpers in this module deliberately stay data-only. They build small,
//! secret-free TOML snippets and parse them through the existing [`Config`]
//! serde path; provider setup, skill loading, and live sessions belong in the
//! tests that consume these fixtures.

use super::{Config, ToolConfig};
use crate::config::SkillsMode;
use crate::config::ToolSelection;
use std::path::PathBuf;

#[test]
fn handoff_defaults_are_enabled_and_bounded() {
    let policy = Config::default()
        .resolve_handoff_policy(None, None)
        .expect("resolve default handoff policy");
    assert!(policy.enabled);
    assert!(policy.agent_enabled);
    assert!(!policy.agent_requires_confirmation);
    assert!(policy.auto_start);
    assert_eq!(policy.max_chain_transitions, 8);
    assert_eq!(policy.instructions, None);
}

#[test]
fn handoff_profile_overrides_policy_and_appends_instruction_sources() {
    let dir = tempfile::TempDir::new().expect("create instruction directory");
    std::fs::write(dir.path().join("global.md"), "global file").unwrap();
    std::fs::write(dir.path().join("profile.md"), "profile file").unwrap();
    let config = config_from_toml(
        r#"
[handoff]
instructions = "global inline"
instructions_file = "global.md"

[profiles.focus]
[profiles.focus.handoff]
agent_enabled = false
auto_start = false
max_chain_transitions = 3
instructions = "profile inline"
instructions_file = "profile.md"
"#,
    );

    let policy = config
        .resolve_handoff_policy(Some("focus"), Some(dir.path()))
        .expect("resolve profile handoff policy");
    assert!(policy.enabled);
    assert!(!policy.agent_enabled);
    assert!(!policy.auto_start);
    assert_eq!(policy.max_chain_transitions, 3);
    assert_eq!(
        policy.instructions.as_deref(),
        Some("global inline\n\nglobal file\n\nprofile inline\n\nprofile file")
    );
}

#[test]
fn disabled_handoff_does_not_read_instruction_files() {
    let config = config_from_toml(
        r#"
[handoff]
enabled = false
instructions_file = "missing.md"
"#,
    );
    let policy = config
        .resolve_handoff_policy(None, None)
        .expect("disabled policy should not touch the filesystem");
    assert!(!policy.enabled);
    assert_eq!(policy.instructions, None);
}

/// Build a TOML document containing one `[profiles.<name>]` table.
///
/// Callers provide only the profile fields needed by a focused test. Keeping
/// the table wrapper here makes the field names and profile selection shape
/// consistent across the config test modules.
pub(super) fn profile_toml(name: &str, fields: &str) -> String {
    assert!(
        !name.trim().is_empty(),
        "fixture profile names must not be empty"
    );
    assert!(
        !name
            .chars()
            .any(|character| matches!(character, '\n' | '\r')),
        "fixture profile names must be a single TOML key"
    );

    let fields = fields.trim_end();
    if fields.is_empty() {
        format!("[profiles.{name}]\n")
    } else {
        format!("[profiles.{name}]\n{fields}\n")
    }
}

/// Parse a fixture document through the existing config serde boundary.
pub(super) fn config_from_toml(source: &str) -> Config {
    toml::from_str(source).expect("config fixture should parse")
}

/// A temporary config document plus the parsed value used by config tests.
///
/// The directory owns the file for the lifetime of the fixture, so tests can
/// pass [`ConfigFileFixture::path`] to file-oriented helpers without touching
/// the user's real `JCODE_HOME` or a shared daemon.
pub(super) struct ConfigFileFixture {
    pub(super) _temp_dir: tempfile::TempDir,
    pub(super) _path: PathBuf,
    pub(super) config: Config,
}

fn config_file_fixture(source: &str) -> ConfigFileFixture {
    let temp_dir = tempfile::TempDir::new().expect("create config fixture directory");
    let path = temp_dir.path().join("config.toml");
    std::fs::write(&path, source).expect("write config fixture");

    ConfigFileFixture {
        _temp_dir: temp_dir,
        _path: path,
        config: config_from_toml(source),
    }
}

/// Materialize and parse a profile fixture without invoking providers.
pub(super) fn profile_config_fixture(name: &str, fields: &str) -> ConfigFileFixture {
    let source = profile_toml(name, fields);
    config_file_fixture(&source)
}

/// Build a profile policy table for tests that exercise the additive
/// `skills_mode`/`disabled_skills` contract.
///
/// The fields are intentionally emitted as TOML text rather than parsed here:
/// the policy fields are owned by the config contract and this fixture remains
/// usable while focused policy tests evolve that contract.
pub(super) fn profile_policy_toml(
    name: &str,
    skills_mode: &str,
    selected_skills: &[&str],
    disabled_skills: &[&str],
) -> String {
    assert!(matches!(skills_mode, "all" | "allowlist" | "none"));
    let selected = toml_string_array(selected_skills);
    let disabled = toml_string_array(disabled_skills);
    profile_toml(
        name,
        &format!(
            "skills_mode = \"{skills_mode}\"\nskills = {selected}\ndisabled_skills = {disabled}\n"
        ),
    )
}

fn toml_string_array(values: &[&str]) -> String {
    let values = values
        .iter()
        .map(|value| {
            let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"{escaped}\"")
        })
        .collect::<Vec<_>>();
    format!("[{}]", values.join(", "))
}

/// Build a config fixture containing credential-like values and instruction
/// content so redaction tests can exercise those paths without provider work.
///
/// The fixture deliberately does not implement `Debug` and callers should
/// inspect presence/length rather than formatting the values.
pub(super) fn secret_bearing_profile_fixture(name: &str) -> ConfigFileFixture {
    let profile = profile_toml(
        name,
        "provider_profile = \"fixture-gateway\"\ninstructions = \"fixture instruction body contains a secret-like value\"\n",
    );
    let source = format!(
        "[providers.fixture-gateway]\ntype = \"openai-compatible\"\nbase_url = \"https://fixture.invalid/v1\"\napi_key = \"fixture-provider-secret\"\n\n{profile}"
    );
    config_file_fixture(&source)
}

#[test]
fn absent_profiles_section_preserves_legacy_config_defaults() {
    let config = config_from_toml("# profiles are intentionally absent\n");
    let defaults = Config::default();

    assert_eq!(
        toml::to_string(&config).expect("config should serialize"),
        toml::to_string(&defaults).expect("default config should serialize"),
        "omitting [profiles] must preserve the legacy effective config shape"
    );
}

#[test]
fn absent_profiles_section_does_not_serialize_an_empty_profiles_table() {
    let config = config_from_toml(
        r#"
[provider]
default_model = "legacy-model"
"#,
    );
    let rendered = toml::to_string_pretty(&config).expect("config should serialize");

    assert!(
        rendered.contains("[provider]"),
        "existing config sections must remain serializable"
    );
    assert!(
        rendered.contains("default_model = \"legacy-model\""),
        "existing provider values must survive the round trip"
    );
    assert!(
        !rendered.lines().any(|line| line.trim() == "[profiles]"),
        "an omitted profiles section must not become an empty table"
    );
    assert!(
        !rendered
            .lines()
            .any(|line| line.trim_start().starts_with("[profiles.")),
        "an omitted profiles section must not create nested profile tables"
    );
}

#[test]
fn complete_profile_fields_deserialize_and_round_trip_unchanged() {
    let fixture = profile_config_fixture(
        "review",
        r#"
provider = "openai"
model = "gpt-5.6-luna"
reasoning_effort = "high"
provider_profile = "team-gateway"
tool_profile = "minimal"
tools = ["read", "write"]
disabled_tools = ["bash"]
skills = ["rust", "testing"]
instructions = "Keep the review focused and actionable."
"#,
    );
    let profile = fixture
        .config
        .profiles
        .get("review")
        .expect("the named profile should load");

    assert_eq!(profile.provider.as_deref(), Some("openai"));
    assert_eq!(profile.model.as_deref(), Some("gpt-5.6-luna"));
    assert_eq!(profile.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(profile.provider_profile.as_deref(), Some("team-gateway"));
    assert_eq!(profile.tool_profile.as_deref(), Some("minimal"));
    assert_eq!(profile.tools, vec!["read".to_string(), "write".to_string()]);
    assert_eq!(profile.disabled_tools, vec!["bash".to_string()]);
    assert_eq!(
        profile.skills,
        vec!["rust".to_string(), "testing".to_string()]
    );
    assert_eq!(
        profile.instructions.as_deref(),
        Some("Keep the review focused and actionable.")
    );

    let rendered = toml::to_string_pretty(&fixture.config).expect("profile should serialize");
    let round_tripped = config_from_toml(&rendered);
    let round_trip_profile = round_tripped
        .profiles
        .get("review")
        .expect("the serialized profile should load again");

    assert_eq!(round_trip_profile.provider, profile.provider);
    assert_eq!(round_trip_profile.model, profile.model);
    assert_eq!(
        round_trip_profile.reasoning_effort,
        profile.reasoning_effort
    );
    assert_eq!(
        round_trip_profile.provider_profile,
        profile.provider_profile
    );
    assert_eq!(round_trip_profile.tool_profile, profile.tool_profile);
    assert_eq!(round_trip_profile.tools, profile.tools);
    assert_eq!(round_trip_profile.disabled_tools, profile.disabled_tools);
    assert_eq!(round_trip_profile.skills, profile.skills);
    assert_eq!(round_trip_profile.instructions, profile.instructions);
}

#[test]
fn omitted_profile_fields_default_and_empty_values_are_skipped_on_serialization() {
    let defaults = config_from_toml(&profile_toml("defaults", ""));
    let default_profile = defaults
        .profiles
        .get("defaults")
        .expect("the empty profile table should load");

    assert_eq!(default_profile.provider, None);
    assert_eq!(default_profile.model, None);
    assert_eq!(default_profile.reasoning_effort, None);
    assert_eq!(default_profile.provider_profile, None);
    assert_eq!(default_profile.tool_profile, None);
    assert!(default_profile.tools.is_empty());
    assert!(default_profile.disabled_tools.is_empty());
    assert!(default_profile.skills.is_empty());
    assert_eq!(default_profile.instructions, None);

    let empty_values = config_from_toml(&profile_toml(
        "empty",
        "tools = []\ndisabled_tools = []\nskills = []\ninstructions = \"\"\n",
    ));
    let rendered = toml::to_string_pretty(&empty_values).expect("profile should serialize");
    let profile_section = rendered
        .split("[profiles.empty]")
        .nth(1)
        .and_then(|section| section.split("\n[").next())
        .expect("serialized profile should have a named table");

    assert!(!profile_section.contains("tools"));
    assert!(!profile_section.contains("disabled_tools"));
    assert!(!profile_section.contains("skills"));
    assert!(!profile_section.contains("instructions"));
}

#[test]
fn selected_profile_lookup_matches_exact_name_and_preserves_persisted_values() {
    let fixture = profile_config_fixture(
        "review",
        r#"
provider = "openai"
model = "gpt-5.6-luna"
reasoning_effort = "high"
provider_profile = "team-gateway"
tool_profile = "minimal"
tools = ["read", "write"]
disabled_tools = ["bash"]
skills = ["rust"]
instructions = "Keep the review focused."
"#,
    );

    let selected = fixture
        .config
        .profiles
        .get("review")
        .expect("an exact profile name should select the persisted entry");
    let selected_again = fixture
        .config
        .profiles
        .get("review")
        .expect("repeating an exact lookup should select the same entry");

    assert!(
        std::ptr::eq(selected, selected_again),
        "profile selection should borrow immutable persisted data rather than copy or mutate it"
    );
    assert_eq!(selected.provider.as_deref(), Some("openai"));
    assert_eq!(selected.model.as_deref(), Some("gpt-5.6-luna"));
    assert_eq!(selected.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(selected.provider_profile.as_deref(), Some("team-gateway"));
    assert_eq!(selected.tool_profile.as_deref(), Some("minimal"));
    assert_eq!(selected.tools, ["read", "write"]);
    assert_eq!(selected.disabled_tools, ["bash"]);
    assert_eq!(selected.skills, ["rust"]);
    assert_eq!(
        selected.instructions.as_deref(),
        Some("Keep the review focused.")
    );
}

#[test]
fn omitted_profile_selection_retains_the_legacy_no_profile_state() {
    let fixture = profile_config_fixture("review", "model = \"profile-model\"\n");
    let requested_profile: Option<&str> = None;

    let selected = requested_profile.and_then(|name| fixture.config.profiles.get(name));

    assert!(
        selected.is_none(),
        "an omitted profile must not perform a lookup or select a configured profile"
    );
    assert_eq!(fixture.config.profiles.len(), 1);
    assert_eq!(
        fixture
            .config
            .profiles
            .get("review")
            .and_then(|profile| profile.model.as_deref()),
        Some("profile-model"),
        "omitting selection must leave persisted profiles untouched"
    );
}

fn selected_profile_tool_selection(source: &str) -> ToolSelection {
    let fixture = profile_config_fixture("review", source);
    let profile = fixture
        .config
        .resolve_session_profile(Some("review"))
        .expect("the profile should resolve");
    ToolConfig {
        profile: profile.tool_profile.unwrap_or_default(),
        enabled: profile.tools,
        disabled: profile.disabled_tools,
        ..ToolConfig::default()
    }
    .selection()
}

#[test]
fn profile_tool_profile_supplies_the_toolconfig_baseline() {
    let selection = selected_profile_tool_selection("tool_profile = \"minimal\"\n");

    let expected = ToolConfig {
        profile: "minimal".to_owned(),
        ..ToolConfig::default()
    }
    .selection();

    assert_eq!(selection, expected);
    let allowed = selection
        .allowed_tools
        .expect("minimal is a finite baseline");
    assert!(allowed.contains("read"));
    assert!(allowed.contains("write"));
    assert!(!allowed.contains("browser"));
}

#[test]
fn profile_allow_list_restricts_the_baseline_and_deny_list_removes_tools() {
    let selection = selected_profile_tool_selection(
        r#"
tool_profile = "minimal"
tools = ["shell", "read_file", "browser"]
disabled_tools = ["functions.shell"]
"#,
    );

    let allowed = selection
        .allowed_tools
        .expect("an explicit profile allow-list is finite");
    assert_eq!(
        allowed,
        std::collections::HashSet::from(["read".to_owned(), "browser".to_owned(),])
    );
    assert_eq!(
        selection.disabled_tools,
        std::collections::HashSet::from(["bash".to_owned()])
    );
}

#[test]
fn profile_tool_aliases_are_canonicalized_before_selection() {
    let selection = selected_profile_tool_selection(
        r#"
tools = ["shell_exec", "file_read", "functions.file_grep"]
disabled_tools = ["read_file"]
"#,
    );

    let allowed = selection
        .allowed_tools
        .expect("the profile allow-list is finite");
    assert_eq!(
        allowed,
        std::collections::HashSet::from(["bash".to_owned(), "agentgrep".to_owned(),])
    );
    assert_eq!(
        selection.disabled_tools,
        std::collections::HashSet::from(["read".to_owned()])
    );
}

#[test]
fn profile_star_and_all_allow_every_tool_but_keep_deny_list_removals() {
    for sentinel in ["*", "all"] {
        let source = format!("tools = [\"{sentinel}\"]\ndisabled_tools = [\"functions.bash\"]\n");
        let selection = selected_profile_tool_selection(&source);

        assert!(
            selection.allowed_tools.is_none(),
            "{sentinel:?} must preserve the unrestricted ToolConfig selection"
        );
        assert_eq!(
            selection.disabled_tools,
            std::collections::HashSet::from(["bash".to_owned()])
        );
    }
}

#[test]
fn profile_unknown_tool_names_are_rejected_before_selection() {
    for (field, source) in [
        ("tools", "tools = [\"not-a-real-tool\"]\n"),
        ("disabled_tools", "disabled_tools = [\"not-a-real-tool\"]\n"),
    ] {
        let fixture = profile_config_fixture("review", source);
        let error = fixture
            .config
            .resolve_session_profile(Some("review"))
            .expect_err("unknown profile tools must fail before Agent setup");
        let message = error.to_string();

        assert!(
            message.contains("review"),
            "diagnostic should name profile: {message}"
        );
        assert!(
            message.contains(field),
            "diagnostic should name field: {message}"
        );
        assert!(
            message.contains("not-a-real-tool"),
            "diagnostic should name invalid tool: {message}"
        );
    }
}

#[test]
fn selected_profile_lookup_does_not_trim_or_fallback_for_empty_or_whitespace_names() {
    let fixture = profile_config_fixture("review", "model = \"profile-model\"\n");

    for requested_profile in ["", " ", "\t", " review ", "missing"] {
        assert!(
            fixture.config.profiles.get(requested_profile).is_none(),
            "profile lookup must not silently normalize or fall back for {requested_profile:?}"
        );
    }

    assert_eq!(
        fixture
            .config
            .profiles
            .get("review")
            .and_then(|profile| profile.model.as_deref()),
        Some("profile-model"),
        "only the exact configured key should select the profile"
    );
}

#[test]
fn resolver_returns_an_immutable_selected_profile_overlay() {
    let fixture = profile_config_fixture(
        "review",
        r#"
provider = "openai"
model = "profile-model"
reasoning_effort = "high"
provider_profile = "team-gateway"
tool_profile = "minimal"
tools = ["read", "write"]
disabled_tools = ["bash"]
skills = ["rust"]
instructions = "Keep the review focused."
"#,
    );
    let before = toml::to_string(&fixture.config).expect("config should serialize");

    let resolved = fixture
        .config
        .resolve_session_profile(Some("review"))
        .expect("an exact profile should resolve");

    assert_eq!(resolved.profile_name.as_deref(), Some("review"));
    assert_eq!(resolved.provider.as_deref(), Some("openai"));
    assert_eq!(resolved.model.as_deref(), Some("profile-model"));
    assert_eq!(resolved.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(resolved.provider_profile.as_deref(), Some("team-gateway"));
    assert_eq!(resolved.tool_profile.as_deref(), Some("minimal"));
    assert_eq!(resolved.tools, ["read", "write"]);
    assert_eq!(resolved.disabled_tools, ["bash"]);
    assert_eq!(resolved.prompt_overlay.skill_names, ["rust"]);
    assert_eq!(
        resolved.prompt_overlay.instructions.as_deref(),
        Some("Keep the review focused.")
    );
    assert_eq!(
        toml::to_string(&fixture.config).expect("config should serialize"),
        before,
        "resolving a profile must not mutate persisted config"
    );
}

#[test]
fn resolver_without_a_profile_returns_the_legacy_empty_overlay() {
    let fixture = profile_config_fixture("review", "model = \"profile-model\"\n");

    let resolved = fixture
        .config
        .resolve_session_profile(None)
        .expect("omitting a profile should keep the legacy path successful");

    assert!(resolved.profile_name.is_none());
    assert!(resolved.provider.is_none());
    assert!(resolved.model.is_none());
    assert!(resolved.reasoning_effort.is_none());
    assert!(resolved.provider_profile.is_none());
    assert!(resolved.tool_profile.is_none());
    assert!(resolved.tools.is_empty());
    assert!(resolved.disabled_tools.is_empty());
    assert!(resolved.prompt_overlay.is_empty());
}

#[test]
fn environment_overrides_mask_matching_selected_profile_fields() {
    let _env_lock = crate::storage::lock_test_env();
    let keys = [
        "JCODE_PROVIDER",
        "JCODE_MODEL",
        "JCODE_OPENAI_REASONING_EFFORT",
        "JCODE_TOOL_PROFILE",
        "JCODE_TOOLS",
        "JCODE_DISABLED_TOOLS",
        "JCODE_PROVIDER_PROFILE_NAME",
        "JCODE_PROVIDER_PROFILE_ACTIVE",
        "JCODE_NAMED_PROVIDER_PROFILE",
    ];
    let previous = keys
        .iter()
        .map(|key| (*key, std::env::var_os(key)))
        .collect::<Vec<_>>();
    for key in keys {
        crate::env::remove_var(key);
    }
    crate::env::set_var("JCODE_PROVIDER", "claude");
    crate::env::set_var("JCODE_MODEL", "environment-model");
    crate::env::set_var("JCODE_OPENAI_REASONING_EFFORT", "environment-reasoning");
    crate::env::set_var("JCODE_TOOL_PROFILE", "minimal");
    crate::env::set_var("JCODE_TOOLS", "read,write");
    crate::env::set_var("JCODE_DISABLED_TOOLS", "bash");
    crate::env::set_var("JCODE_PROVIDER_PROFILE_NAME", "environment-gateway");
    crate::env::set_var("JCODE_PROVIDER_PROFILE_ACTIVE", "1");

    let fixture = profile_config_fixture(
        "review",
        r#"
provider = "openai"
model = "profile-model"
reasoning_effort = "high"
provider_profile = "profile-gateway"
tool_profile = "full"
tools = ["edit"]
disabled_tools = ["write"]
"#,
    );
    let resolved = fixture
        .config
        .resolve_session_profile_with_environment(Some("review"))
        .expect("selected profile should resolve");

    assert!(resolved.provider.is_none());
    assert!(resolved.model.is_none());
    assert!(resolved.reasoning_effort.is_none());
    assert!(resolved.provider_profile.is_none());
    assert!(resolved.tool_profile.is_none());
    assert!(resolved.tools.is_empty());
    assert!(resolved.disabled_tools.is_empty());

    for (key, value) in previous {
        if let Some(value) = value {
            crate::env::set_var(key, value);
        } else {
            crate::env::remove_var(key);
        }
    }
}

#[test]
fn selected_profile_rejects_empty_or_unknown_names_with_available_choices() {
    let mut config = config_from_toml(&profile_toml("review", "model = \"profile-model\"\n"));
    config.profiles.insert(
        "ship".to_owned(),
        crate::config::SessionProfileConfig::default(),
    );

    for requested in [Some(""), Some(" \t"), Some("missing")] {
        let error = config
            .resolve_session_profile(requested)
            .expect_err("invalid profile names must fail before runtime setup");
        let message = error.to_string();
        assert!(
            message.contains("profile"),
            "diagnostic should identify profiles: {message}"
        );
        if let Some(requested) = requested.filter(|name| !name.trim().is_empty()) {
            assert!(
                message.contains(requested),
                "diagnostic should name request: {message}"
            );
        }
        assert!(
            message.contains("review"),
            "diagnostic should list available choices: {message}"
        );
        assert!(
            message.contains("ship"),
            "diagnostic should list available choices: {message}"
        );
    }
}

#[test]
fn malformed_profile_fields_fail_at_toml_boundary_with_key_context() {
    for (key, value) in [
        ("tools", "\"read\""),
        ("disabled_tools", "\"bash\""),
        ("skills", "\"testing\""),
        ("instructions", "[\"not\", \"a\", \"string\"]"),
    ] {
        let source = profile_toml("review", &format!("{key} = {value}\n"));
        let error = toml::from_str::<Config>(&source)
            .expect_err("malformed profile fields must fail strict deserialization");
        let message = error.to_string();
        assert!(
            message.contains(key),
            "diagnostic should identify key {key}: {message}"
        );
        assert!(
            !message.contains("api_key"),
            "diagnostics must not expose secrets: {message}"
        );
    }
}

#[test]
fn selected_profile_rejects_invalid_scalar_values_before_overlay() {
    for (key, value, expected_hint) in [
        ("provider", "not-a-provider", "provider"),
        ("model", "", "model"),
        ("reasoning_effort", "turbo", "reasoning_effort"),
        ("tool_profile", "not-a-tool-profile", "tool_profile"),
    ] {
        let source = profile_toml("review", &format!("{key} = \"{value}\"\n"));
        let config = config_from_toml(&source);
        let error = config
            .resolve_session_profile(Some("review"))
            .expect_err("invalid profile values must fail before runtime setup");
        let message = error.to_string();
        assert!(
            message.contains("review"),
            "diagnostic should name profile: {message}"
        );
        assert!(
            message.contains(expected_hint),
            "diagnostic should name field: {message}"
        );
        if !value.is_empty() {
            assert!(
                message.contains(value),
                "diagnostic should name invalid value: {message}"
            );
        }
    }
}

#[test]
fn selected_profile_rejects_unavailable_skills_without_partial_overlay() {
    let config = config_from_toml(&profile_toml(
        "review",
        "skills = [\"skill-that-is-not-installed\"]\n",
    ));
    let resolved = config
        .resolve_session_profile(Some("review"))
        .expect("skill names remain data-only until the effective registry is available");
    let registry = crate::skill::SkillRegistry::default();
    let error = registry
        .render_profile_skills("review", &resolved.prompt_overlay.skill_names)
        .expect_err("an unavailable selected skill must fail before provider contact");
    let message = error.to_string();
    assert!(
        message.contains("review"),
        "diagnostic should name profile: {message}"
    );
    assert!(
        message.contains("skill-that-is-not-installed"),
        "diagnostic should name skill: {message}"
    );
    assert!(
        resolved.prompt_overlay.skill_prompts.is_empty(),
        "no partial prompt overlay is allowed"
    );
    assert!(
        !message.contains("secret"),
        "skill diagnostics must not expose prompt contents: {message}"
    );
}

#[test]
fn contradictory_provider_profile_values_are_rejected_without_mutating_config() {
    let config = config_from_toml(&profile_toml(
        "review",
        "provider = \"claude\"\nprovider_profile = \"gateway\"\n",
    ));
    let before = toml::to_string(&config).expect("config should serialize");
    let error = config
        .resolve_session_profile(Some("review"))
        .expect_err("contradictory provider fields must fail before startup");
    let message = error.to_string();
    assert!(
        message.contains("review"),
        "diagnostic should name profile: {message}"
    );
    assert!(
        message.contains("provider_profile"),
        "diagnostic should name field: {message}"
    );
    assert_eq!(
        toml::to_string(&config).expect("config should serialize"),
        before
    );
}

#[test]
fn profile_policy_fixture_builder_covers_supported_modes() {
    for mode in ["all", "allowlist", "none"] {
        let source = profile_policy_toml("review", mode, &["rust", "testing"], &["secret"]);
        assert!(source.contains(&format!("skills_mode = \"{mode}\"")));
        assert!(source.contains("skills = [\"rust\", \"testing\"]"));
        assert!(source.contains("disabled_skills = [\"secret\"]"));
    }
}

#[test]
fn skills_policy_fields_parse_supported_modes_and_dedupe_deterministically() {
    let fixture = profile_config_fixture(
        "review",
        r#"
skills_mode = "allowlist"
skills = ["rust", "rust", "review"]
disabled_skills = ["secret", "secret", "rust"]
"#,
    );
    let profile = fixture
        .config
        .profiles
        .get("review")
        .expect("profile should parse");

    assert_eq!(profile.skills_mode, Some(SkillsMode::Allowlist));
    assert_eq!(profile.skills, ["rust", "rust", "review"]);
    assert_eq!(profile.disabled_skills, ["secret", "secret", "rust"]);

    let resolved = fixture
        .config
        .resolve_session_profile(Some("review"))
        .expect("supported policy should resolve");
    assert_eq!(resolved.skill_policy.mode, Some(SkillsMode::Allowlist));
    assert_eq!(resolved.skill_policy.selected_skills, ["rust", "review"]);
    assert_eq!(resolved.skill_policy.disabled_skills, ["secret", "rust"]);
}

#[test]
fn omitted_skills_policy_preserves_legacy_serialized_shape_and_resolution() {
    let config = config_from_toml(&profile_toml("review", "model = \"profile-model\"\n"));
    let profile = config.profiles.get("review").expect("profile should parse");
    assert!(profile.skills_mode.is_none());
    assert!(profile.disabled_skills.is_empty());

    let rendered = toml::to_string(&config).expect("config should serialize");
    assert!(!rendered.contains("skills_mode"));
    assert!(!rendered.contains("disabled_skills"));

    let resolved = config
        .resolve_session_profile(Some("review"))
        .expect("legacy profile should resolve");
    assert!(resolved.skill_policy.mode.is_none());
    assert!(resolved.skill_policy.effective_skills.is_empty());
}

#[test]
fn unsupported_skills_mode_fails_at_the_profile_key_boundary() {
    let error =
        toml::from_str::<Config>(&profile_toml("review", "skills_mode = \"selected-only\"\n"))
            .expect_err("unsupported skills policy modes must be rejected");
    let message = error.to_string();
    assert!(message.contains("skills_mode"), "diagnostic: {message}");
    assert!(message.contains("selected-only"), "diagnostic: {message}");
}

#[test]
fn canonical_policy_resolves_modes_and_disabled_intersections() {
    for (mode, expected) in [
        ("all", vec!["alpha", "gamma"]),
        ("allowlist", vec!["alpha"]),
        ("none", Vec::new()),
    ] {
        let fixture = profile_config_fixture(
            "review",
            &format!(
                "skills_mode = \"{mode}\"\nskills = [\"alpha\", \"beta\"]\ndisabled_skills = [\"beta\"]\n"
            ),
        );
        let resolved = fixture
            .config
            .resolve_session_profile_with_skills(Some("review"), ["alpha", "beta", "gamma"])
            .expect("policy should resolve against the available registry");
        assert_eq!(resolved.skill_policy.effective_skills, expected);
    }
}

#[test]
fn resolved_snapshot_is_stable_and_contains_no_secret_or_instruction_body() {
    let fixture = secret_bearing_profile_fixture("review");
    let resolved = fixture
        .config
        .resolve_session_profile(Some("review"))
        .expect("fixture profile should resolve");
    let snapshot = resolved.snapshot(&fixture.config.tools);
    let encoded = serde_json::to_string(&snapshot).expect("snapshot should serialize");

    assert!(snapshot.is_secret_free());
    assert!(encoded.contains("fixture-gateway"));
    assert!(encoded.contains("instructions_present"));
    assert!(!encoded.contains("fixture-provider-secret"));
    assert!(!encoded.contains("fixture instruction body contains a secret-like value"));
    assert!(snapshot.fingerprint.starts_with("sha256:"));

    let same = fixture
        .config
        .resolve_session_profile(Some("review"))
        .expect("same fixture should resolve")
        .snapshot(&fixture.config.tools);
    assert_eq!(snapshot.fingerprint, same.fingerprint);

    let mut changed = fixture.config.clone();
    changed
        .profiles
        .get_mut("review")
        .expect("profile exists")
        .model = Some("changed-model".to_owned());
    let changed_snapshot = changed
        .resolve_session_profile(Some("review"))
        .expect("safe model change should resolve")
        .snapshot(&changed.tools);
    assert_ne!(snapshot.fingerprint, changed_snapshot.fingerprint);
}

#[test]
fn inspection_reports_safe_profile_sources_without_provider_work() {
    let fixture = profile_config_fixture(
        "review",
        "provider = \"openai\"\nmodel = \"profile-model\"\nskills_mode = \"none\"\n",
    );
    let resolved = fixture
        .config
        .resolve_session_profile(Some("review"))
        .expect("profile should resolve");
    let inspection = resolved.inspection(&fixture.config.tools);

    assert_eq!(inspection.profile_name.as_deref(), Some("review"));
    assert_eq!(
        inspection.sources.get("model"),
        Some(&crate::config::FieldSource::Profile)
    );
    assert_eq!(
        inspection.effective.skill_policy.mode,
        Some(SkillsMode::None)
    );
    assert!(inspection.warnings.is_empty());
}

#[test]
fn secret_bearing_profile_fixture_is_constructed_without_provider_work() {
    let fixture = secret_bearing_profile_fixture("review");
    let provider = fixture
        .config
        .providers
        .get("fixture-gateway")
        .expect("fixture gateway should be configured");

    assert!(provider.api_key.is_some());
    assert_eq!(
        fixture
            .config
            .profiles
            .get("review")
            .and_then(|profile| profile.instructions.as_deref()),
        Some("fixture instruction body contains a secret-like value")
    );
}
