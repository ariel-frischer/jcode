use super::*;
use crate::config::{
    LIBRARIAN_MAX_ITEM_TOKENS, LIBRARIAN_MAX_NORMALIZED_FILE_TOKENS, LIBRARIAN_MAX_RECEIPT_BYTES,
    LIBRARIAN_MAX_TOOL_CATEGORY_TOKENS, LibrarianConfigError, LibrarianInvocationOverrides,
    LibrarianRouteIdentity, resolve_librarian_config,
};

const LIBRARIAN_ENV_KEYS: &[&str] = &[
    "JCODE_SESSION_LIBRARIAN_PROVIDER",
    "JCODE_SESSION_LIBRARIAN_MODEL",
    "JCODE_SESSION_LIBRARIAN_REASONING_EFFORT",
    "JCODE_SESSION_LIBRARIAN_MAX_INPUT_TOKENS",
    "JCODE_SESSION_LIBRARIAN_MAX_OUTPUT_TOKENS",
    "JCODE_SESSION_LIBRARIAN_MAX_REQUESTS",
    "JCODE_SESSION_LIBRARIAN_MAX_COST_USD",
    "JCODE_SESSION_LIBRARIAN_DEADLINE_SECONDS",
];

struct LibrarianEnvGuard {
    saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl LibrarianEnvGuard {
    fn new() -> Self {
        let saved = LIBRARIAN_ENV_KEYS
            .iter()
            .map(|&key| {
                let value = std::env::var_os(key);
                crate::env::remove_var(key);
                (key, value)
            })
            .collect();
        Self { saved }
    }
}

impl Drop for LibrarianEnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.saved.drain(..) {
            if let Some(value) = value {
                crate::env::set_var(key, value);
            } else {
                crate::env::remove_var(key);
            }
        }
    }
}

fn supported(route: &LibrarianRouteIdentity) -> bool {
    matches!(
        (route.provider.as_str(), route.model.as_str()),
        ("openai-oauth", "gpt-5.6-luna")
            | ("openai-api", "gpt-5.6-sol")
            | ("openai-api", "gpt-5.6-luna")
            | ("claude-oauth", "claude-opus-4-1")
    )
}

fn active_route() -> LibrarianRouteIdentity {
    LibrarianRouteIdentity {
        provider: "claude-oauth".to_string(),
        model: "claude-opus-4-1".to_string(),
        reasoning_effort: "high".to_string(),
    }
}

#[test]
fn librarian_defaults_are_conservative_and_exact() {
    let config = Config::default();
    let resolved = resolve_librarian_config(
        &config,
        &LibrarianInvocationOverrides::default(),
        &active_route(),
        supported,
    )
    .expect("built-in librarian defaults should resolve");

    assert_eq!(resolved.route.provider, "openai-oauth");
    assert_eq!(resolved.route.model, "gpt-5.6-luna");
    assert_eq!(resolved.route.reasoning_effort, "xhigh");
    assert_eq!(resolved.budgets.max_input_tokens, 12_000);
    assert_eq!(resolved.budgets.max_output_tokens, 2_500);
    assert_eq!(resolved.budgets.max_requests, 1);
    assert_eq!(resolved.budgets.max_cost_micros, 500_000);
    assert_eq!(resolved.budgets.deadline_seconds, 120);
}

#[test]
fn librarian_persisted_values_override_built_in_defaults() {
    let config: Config = toml::from_str(
        r#"
[session_librarian]
provider = "openai-api"
model = "gpt-5.6-sol"
reasoning_effort = "medium"
max_input_tokens = "9000"
max_output_tokens = "1800"
max_requests = "1"
max_cost_usd = "0.375"
deadline_seconds = "75"
"#,
    )
    .expect("librarian persisted values should deserialize");

    let resolved = resolve_librarian_config(
        &config,
        &LibrarianInvocationOverrides::default(),
        &active_route(),
        supported,
    )
    .expect("valid persisted librarian values should resolve");

    assert_eq!(resolved.route.provider, "openai-api");
    assert_eq!(resolved.route.model, "gpt-5.6-sol");
    assert_eq!(resolved.route.reasoning_effort, "medium");
    assert_eq!(resolved.budgets.max_input_tokens, 9_000);
    assert_eq!(resolved.budgets.max_output_tokens, 1_800);
    assert_eq!(resolved.budgets.max_requests, 1);
    assert_eq!(resolved.budgets.max_cost_micros, 375_000);
    assert_eq!(resolved.budgets.deadline_seconds, 75);
}

#[test]
fn librarian_precedence_is_invocation_then_environment_then_persisted_then_default() {
    let _lock = crate::storage::lock_test_env();
    let _env = LibrarianEnvGuard::new();
    crate::env::set_var("JCODE_SESSION_LIBRARIAN_PROVIDER", "openai-api");
    crate::env::set_var("JCODE_SESSION_LIBRARIAN_MODEL", "gpt-5.6-sol");
    crate::env::set_var("JCODE_SESSION_LIBRARIAN_REASONING_EFFORT", "medium");
    crate::env::set_var("JCODE_SESSION_LIBRARIAN_MAX_INPUT_TOKENS", "8000");
    crate::env::set_var("JCODE_SESSION_LIBRARIAN_MAX_OUTPUT_TOKENS", "1600");
    crate::env::set_var("JCODE_SESSION_LIBRARIAN_MAX_COST_USD", "0.25");

    let mut config: Config = toml::from_str(
        r#"
[session_librarian]
provider = "claude-oauth"
model = "claude-opus-4-1"
reasoning_effort = "high"
max_input_tokens = "7000"
max_output_tokens = "1400"
max_cost_usd = "0.20"
deadline_seconds = "90"
"#,
    )
    .expect("persisted librarian values should deserialize");
    config.apply_env_overrides();

    let invocation = LibrarianInvocationOverrides {
        model: Some("gpt-5.6-luna".to_string()),
        max_output_tokens: Some("1200".to_string()),
        deadline_seconds: Some("60".to_string()),
        ..Default::default()
    };
    let resolved = resolve_librarian_config(&config, &invocation, &active_route(), supported)
        .expect("precedence matrix should resolve");

    assert_eq!(resolved.route.provider, "openai-api", "environment wins");
    assert_eq!(resolved.route.model, "gpt-5.6-luna", "invocation wins");
    assert_eq!(
        resolved.route.reasoning_effort, "medium",
        "environment wins"
    );
    assert_eq!(resolved.budgets.max_input_tokens, 8_000, "environment wins");
    assert_eq!(resolved.budgets.max_output_tokens, 1_200, "invocation wins");
    assert_eq!(
        resolved.budgets.max_requests, 1,
        "default remains available"
    );
    assert_eq!(
        resolved.budgets.max_cost_micros, 250_000,
        "environment wins"
    );
    assert_eq!(resolved.budgets.deadline_seconds, 60, "invocation wins");
}

#[test]
fn librarian_unset_values_fall_through_but_explicit_empty_values_are_invalid() {
    let config = Config::default();
    let resolved = resolve_librarian_config(
        &config,
        &LibrarianInvocationOverrides::default(),
        &active_route(),
        supported,
    )
    .expect("unset values should fall through to defaults");
    assert_eq!(resolved.route.provider, "openai-oauth");

    for invocation in [
        LibrarianInvocationOverrides {
            provider: Some(String::new()),
            ..Default::default()
        },
        LibrarianInvocationOverrides {
            model: Some("  ".to_string()),
            ..Default::default()
        },
        LibrarianInvocationOverrides {
            max_cost_usd: Some(String::new()),
            ..Default::default()
        },
    ] {
        assert!(
            resolve_librarian_config(&config, &invocation, &active_route(), supported).is_err(),
            "an explicitly empty override must not silently fall back"
        );
    }
}

#[test]
fn librarian_invalid_budget_values_fail_closed() {
    let config = Config::default();
    let cases = [
        ("max_input_tokens", "0"),
        ("max_input_tokens", "-1"),
        ("max_output_tokens", "many"),
        ("max_requests", "0"),
        ("max_requests", "2"),
        ("max_cost_usd", "0"),
        ("max_cost_usd", "NaN"),
        ("max_cost_usd", "0.5000001"),
        ("deadline_seconds", "0"),
    ];

    for (field, value) in cases {
        let mut invocation = LibrarianInvocationOverrides::default();
        match field {
            "max_input_tokens" => invocation.max_input_tokens = Some(value.to_string()),
            "max_output_tokens" => invocation.max_output_tokens = Some(value.to_string()),
            "max_requests" => invocation.max_requests = Some(value.to_string()),
            "max_cost_usd" => invocation.max_cost_usd = Some(value.to_string()),
            "deadline_seconds" => invocation.deadline_seconds = Some(value.to_string()),
            _ => unreachable!(),
        }

        let error = resolve_librarian_config(&config, &invocation, &active_route(), supported)
            .expect_err("invalid hard budgets must fail before provider work");
        assert!(
            matches!(error, LibrarianConfigError::InvalidBudget { .. }),
            "{field}={value:?} returned {error:?}"
        );
    }
}

#[test]
fn librarian_cost_budget_uses_exact_integer_micros() {
    let config = Config::default();
    for (usd, expected_micros) in [("0.50", 500_000), ("0.375", 375_000), ("0.000001", 1)] {
        let invocation = LibrarianInvocationOverrides {
            max_cost_usd: Some(usd.to_string()),
            ..Default::default()
        };
        let resolved = resolve_librarian_config(&config, &invocation, &active_route(), supported)
            .expect("six-decimal USD values should convert exactly");
        assert_eq!(resolved.budgets.max_cost_micros, expected_micros);
    }
}

#[test]
fn librarian_unsupported_route_is_rejected_before_use() {
    let config = Config::default();
    let invocation = LibrarianInvocationOverrides {
        provider: Some("openrouter".to_string()),
        model: Some("unknown/model".to_string()),
        ..Default::default()
    };

    let error = resolve_librarian_config(&config, &invocation, &active_route(), supported)
        .expect_err("unsupported routes must fail before provider work");
    assert!(matches!(
        error,
        LibrarianConfigError::UnsupportedRoute { .. }
    ));
}

#[test]
fn librarian_route_resolution_does_not_inherit_or_mutate_the_active_session_route() {
    let config = Config::default();
    let active = active_route();
    let before = active.clone();

    let resolved = resolve_librarian_config(
        &config,
        &LibrarianInvocationOverrides::default(),
        &active,
        supported,
    )
    .expect("independent default librarian route should resolve");

    assert_eq!(active, before, "active session route must remain unchanged");
    assert_ne!(
        resolved.route, active,
        "librarian must not inherit active route"
    );
    assert_eq!(resolved.route.provider, "openai-oauth");
}

#[test]
fn librarian_fixed_local_caps_precede_the_configurable_global_cap() {
    assert_eq!(LIBRARIAN_MAX_RECEIPT_BYTES, 1024);
    assert_eq!(LIBRARIAN_MAX_ITEM_TOKENS, 768);
    assert_eq!(LIBRARIAN_MAX_NORMALIZED_FILE_TOKENS, 1200);
    assert_eq!(LIBRARIAN_MAX_TOOL_CATEGORY_TOKENS, 2000);

    let config = Config::default();
    let invocation = LibrarianInvocationOverrides {
        max_input_tokens: Some("500".to_string()),
        ..Default::default()
    };
    let resolved = resolve_librarian_config(&config, &invocation, &active_route(), supported)
        .expect("a smaller global cap remains valid");

    assert_eq!(resolved.budgets.max_input_tokens, 500);
    assert_eq!(resolved.admission_caps.max_item_tokens, 500);
    assert_eq!(resolved.admission_caps.max_normalized_file_tokens, 500);
    assert_eq!(resolved.admission_caps.max_tool_category_tokens, 500);
    assert_eq!(resolved.admission_caps.max_receipt_bytes, 1024);
}
